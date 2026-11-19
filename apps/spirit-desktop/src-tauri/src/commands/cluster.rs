//! Cluster command adapters: form, health, promote, backup policy and worker removal.
//!
//! Every remote rule, SSH command and VOWP argv lives in `spirit-core::cluster`. This
//! module only:
//!
//! - keeps the **safe** saved cluster shape (Home, worker host ids and node names, the
//!   backup policy and the latest promotion) under the app-data directory,
//! - maps renderer inputs to core requests,
//! - re-checks the owner's explicit confirmation and VOWP's 60-second schedule floor,
//!   and
//! - projects core health and outcomes into path-free view models.
//!
//! The saved record deliberately contains no backup destination, project checkout,
//! new-home directory or SSH target. Those values exist only in the one request that
//! uses them. `backupPolicy` and `removeWorker` are local saved-config operations; core
//! has no remote command for either, so they never touch a server.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use spirit_core::cluster::{
    BackupPolicy, ClusterAgentState, ClusterConfig, ClusterFormRequest, ClusterFormed,
    ClusterHealth, ClusterHostHealth, ClusterHostRole, ClusterProject, ClusterService,
    ClusterTotalsSource, ClusterWorker, ClusterWorkerProject, ClusterWorkerRole, PromoteOutcome,
    PromoteRequest,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::HostId;
use spirit_core::time::Timestamp;
use spirit_core::wire::{NodeName, ProjectName};
use tauri::{Manager, State};

use crate::error::CommandResult;
use crate::state::AppState;

const CLUSTER_RECORD_FILE: &str = "cluster.json";
const MIN_BACKUP_EVERY_SECS: u64 = 60;
const MAX_BACKUP_TARGET: usize = 4096;

/// The private, path-free record StackSpirit saves after forming a cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClusterRecord {
    config: ClusterConfig,
    formed_at: Timestamp,
    #[serde(default)]
    promotion: Option<PromotionRecord>,
}

/// One completed promotion. `home_dir` is deliberately absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromotionRecord {
    new_home: String,
    node_name: String,
    backup_at: Timestamp,
    promoted_at: Timestamp,
}

/// The app-private cluster record file. Saving is atomic so a reader never sees half a
/// document, and the file is owner-only on Unix.
struct ClusterStore {
    path: PathBuf,
}

impl ClusterStore {
    fn for_app(app: &tauri::AppHandle) -> CommandResult<Self> {
        let directory = app.path().app_data_dir().map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not resolve the StackSpirit app-data directory",
            )
        })?;
        Ok(Self {
            path: directory.join(CLUSTER_RECORD_FILE),
        })
    }

    fn load(&self) -> CommandResult<Option<ClusterRecord>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&self.path).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not read the saved cluster record",
            )
        })?;
        let record: ClusterRecord = serde_json::from_str(&text)
            .map_err(|err| SpiritError::parse("saved cluster record", err))?;
        record.config.validate()?;
        Ok(Some(record))
    }

    fn save(&self, record: &ClusterRecord) -> CommandResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| {
                SpiritError::new(
                    SpiritErrorCode::Io,
                    "could not create the StackSpirit app-data directory",
                )
            })?;
        }
        let bytes = serde_json::to_vec_pretty(record).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not encode the saved cluster record",
            )
        })?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not write the saved cluster record",
            )
        })?;
        set_owner_only(&temporary);
        fs::rename(&temporary, &self.path).map_err(|_| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not replace the saved cluster record",
            )
        })?;
        Ok(())
    }
}

#[cfg(unix)]
fn set_owner_only(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only(_path: &std::path::Path) {}

/// One requested worker server.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterWorkerInput {
    pub host: String,
    pub node_name: String,
    pub role: ClusterWorkerRole,
}

/// The Home project whose `vowp.toml` receives the `[offload]` settings.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterProjectInput {
    pub name: String,
    pub root: String,
}

/// The backup interval and the owner's retention expectation.
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupPolicyInput {
    pub every_secs: u64,
    pub keep: u32,
}

/// One optional worker path map.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterWorkerProjectInput {
    pub worker: String,
    pub project: String,
    pub root: String,
    #[serde(default)]
    pub worktree_root: Option<String>,
    #[serde(default)]
    pub allow: Vec<String>,
}

/// Everything the renderer sends to form a cluster.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterFormInput {
    pub home: String,
    pub workers: Vec<ClusterWorkerInput>,
    pub backup: BackupPolicyInput,
    pub home_project: ClusterProjectInput,
    #[serde(default)]
    pub worker_projects: Vec<ClusterWorkerProjectInput>,
    pub backup_target: String,
    pub confirmed: bool,
}

impl std::fmt::Debug for ClusterFormInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClusterFormInput")
            .field("home", &self.home)
            .field("workers", &self.workers.len())
            .field("backup", &self.backup.every_secs)
            .field("home_project", &self.home_project.name)
            .field("home_project_root", &"[remote-path]")
            .field("backup_target", &"[remote-target]")
            .field("confirmed", &self.confirmed)
            .finish()
    }
}

/// A promotion request as the renderer sends it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterPromoteInput {
    pub target: String,
    pub backup_dir: String,
    #[serde(default)]
    pub new_home: Option<String>,
    pub confirmation: String,
}

impl std::fmt::Debug for ClusterPromoteInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClusterPromoteInput")
            .field("target", &self.target)
            .field("backup_dir", &"[remote-path]")
            .field("new_home", &self.new_home.as_ref().map(|_| "[remote-path]"))
            .finish()
    }
}

/// A safe backup policy plus VOWP's schedule specification.
#[derive(Debug, Clone, Serialize)]
pub struct BackupPolicyView {
    pub every_secs: u64,
    pub keep: u32,
    pub schedule: String,
}

/// One configured worker without any path or credential.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterWorkerView {
    pub host: String,
    pub node_name: String,
    pub role: String,
    pub agent_state: String,
}

/// The saved cluster shape.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterConfigView {
    pub home: String,
    pub workers: Vec<ClusterWorkerView>,
    pub backup: BackupPolicyView,
}

/// The safe result of forming a cluster.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterFormedView {
    pub config: ClusterConfigView,
    pub backup_schedule: String,
    pub offload_config_changed: bool,
    pub formed_at: Timestamp,
}

/// One safe aggregate-cluster health row.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterHostHealthView {
    pub host: String,
    pub label: String,
    pub role: String,
    pub node_name: Option<String>,
    pub agent_state: String,
    pub status: String,
    pub presence: String,
    pub registered: bool,
    pub slots_busy: u32,
    pub slots_max: u32,
    pub mapped_projects: Vec<String>,
    pub last_seen_at: Option<Timestamp>,
    pub last_error: Option<String>,
}

/// The combined cluster read model.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterHealthView {
    pub home_node: String,
    pub online_workers: u32,
    pub home_target_workers: u32,
    pub worker_slots: u32,
    pub pooled_target_workers: u32,
    pub busy_workers: u32,
    pub cluster_totals_present: bool,
    pub totals_source: String,
    pub hosts: Vec<ClusterHostHealthView>,
    pub warnings: Vec<String>,
    pub checked_at: Timestamp,
}

/// One saved promotion, without the target home directory.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterPromotionView {
    pub new_home: String,
    pub node_name: String,
    pub backup_at: Timestamp,
    pub promoted_at: Timestamp,
}

/// The saved cluster plus the latest read.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterStateView {
    pub configured: bool,
    pub config: Option<ClusterConfigView>,
    pub health: Option<ClusterHealthView>,
    pub health_error: Option<SpiritError>,
    pub promotion: Option<ClusterPromotionView>,
}

impl ClusterStateView {
    fn unconfigured() -> Self {
        Self {
            configured: false,
            config: None,
            health: None,
            health_error: None,
            promotion: None,
        }
    }
}

/// The new Home identity returned by a promotion.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterPromoteOutcomeView {
    pub new_home: String,
    pub node_name: String,
    pub backup_at: Timestamp,
    pub promoted_at: Timestamp,
}

fn service(state: &AppState) -> ClusterService {
    ClusterService::new(
        state.hosts(),
        state.runner(),
        state.known_hosts().to_path_buf(),
    )
}

fn policy_view(policy: &BackupPolicy) -> BackupPolicyView {
    BackupPolicyView {
        every_secs: policy.every.as_secs(),
        keep: policy.keep,
        schedule: policy
            .schedule_spec()
            .unwrap_or_else(|_| format!("every {}s", policy.every.as_secs())),
    }
}

fn config_view(config: &ClusterConfig) -> ClusterConfigView {
    ClusterConfigView {
        home: config.home.0.clone(),
        workers: config
            .workers
            .iter()
            .map(|worker| ClusterWorkerView {
                host: worker.host.0.clone(),
                node_name: worker.node_name.clone(),
                role: worker.role.as_str().to_owned(),
                agent_state: worker.agent_state.as_str().to_owned(),
            })
            .collect(),
        backup: policy_view(&config.backup),
    }
}

fn formed_view(formed: &ClusterFormed) -> ClusterFormedView {
    ClusterFormedView {
        config: config_view(&formed.config),
        backup_schedule: formed.backup_schedule.clone(),
        offload_config_changed: formed.offload_config_changed,
        formed_at: formed.formed_at,
    }
}

fn host_role(role: ClusterHostRole) -> &'static str {
    match role {
        ClusterHostRole::Home => "home",
        ClusterHostRole::Worker => "worker",
        ClusterHostRole::Backup => "backup",
    }
}

fn totals_source(source: ClusterTotalsSource) -> &'static str {
    match source {
        ClusterTotalsSource::VowpCluster => "vowp_cluster",
        ClusterTotalsSource::Computed => "computed",
    }
}

fn host_view(row: &ClusterHostHealth) -> ClusterHostHealthView {
    ClusterHostHealthView {
        host: row.host.0.clone(),
        label: row.label.clone(),
        role: host_role(row.role).to_owned(),
        node_name: row.node_name.clone(),
        agent_state: row.agent_state.as_str().to_owned(),
        status: row.status.as_str().to_owned(),
        presence: row.presence.as_str().to_owned(),
        registered: row.registered,
        slots_busy: row.slots_busy,
        slots_max: row.slots_max,
        mapped_projects: row.mapped_projects.clone(),
        last_seen_at: row.last_seen_at,
        last_error: row.last_error.clone(),
    }
}

fn health_view(health: &ClusterHealth) -> ClusterHealthView {
    ClusterHealthView {
        home_node: health.home_node.clone(),
        online_workers: health.online_workers,
        home_target_workers: health.home_target_workers,
        worker_slots: health.worker_slots,
        pooled_target_workers: health.pooled_target_workers,
        busy_workers: health.busy_workers,
        cluster_totals_present: health.cluster_totals_present,
        totals_source: totals_source(health.totals_source).to_owned(),
        hosts: health.hosts.iter().map(host_view).collect(),
        warnings: health.warnings.clone(),
        checked_at: health.checked_at,
    }
}

fn promotion_view(promotion: &PromotionRecord) -> ClusterPromotionView {
    ClusterPromotionView {
        new_home: promotion.new_home.clone(),
        node_name: promotion.node_name.clone(),
        backup_at: promotion.backup_at,
        promoted_at: promotion.promoted_at,
    }
}

/// Validates the one remote destination the form supplies, before core stores or runs it.
///
/// Core accepts any argv-safe target because VOWP does; this adapter also refuses a
/// credential-shaped target so a password or token can never enter the request, the
/// saved record or a log.
fn validate_backup_target(target: &str) -> Result<(), SpiritError> {
    let value = target.trim();
    if value.is_empty() {
        return Err(SpiritError::invalid_input(
            "backup_target",
            "must be one path or rsync target",
        ));
    }
    if value.len() > MAX_BACKUP_TARGET {
        return Err(SpiritError::invalid_input("backup_target", "is too long"));
    }
    if value.starts_with('-') {
        return Err(SpiritError::invalid_input(
            "backup_target",
            "must not start with '-'",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(SpiritError::invalid_input(
            "backup_target",
            "must not contain control characters",
        ));
    }
    if value.contains("://") || value.find('@').is_some_and(|at| value[..at].contains(':')) {
        return Err(SpiritError::invalid_input(
            "backup_target",
            "must not embed a password or token",
        )
        .with_hint("Use the server's SSH key for the backup destination."));
    }
    Ok(())
}

/// Builds and validates the core config without touching a host.
fn build_config(input: &ClusterFormInput) -> Result<ClusterConfig, SpiritError> {
    if input.backup.every_secs < MIN_BACKUP_EVERY_SECS {
        return Err(SpiritError::invalid_input(
            "backup.every",
            "VOWP's shortest backup schedule is 60 seconds",
        ));
    }
    if input.home.trim().is_empty() {
        return Err(SpiritError::invalid_input(
            "cluster.home",
            "choose one saved Home server",
        ));
    }
    if input.workers.is_empty() {
        return Err(SpiritError::invalid_input(
            "cluster.workers",
            "choose at least one Worker/Backup server",
        ));
    }
    let mut workers = Vec::with_capacity(input.workers.len());
    for worker in &input.workers {
        NodeName::parse(&worker.node_name).map_err(|_| {
            SpiritError::invalid_input("worker.node_name", "must be a lowercase VOWP node slug")
        })?;
        workers.push(ClusterWorker {
            host: HostId(worker.host.clone()),
            node_name: worker.node_name.clone(),
            role: worker.role,
            agent_state: ClusterAgentState::Pending,
        });
    }
    let config = ClusterConfig {
        home: HostId(input.home.clone()),
        workers,
        backup: BackupPolicy {
            every: Duration::from_secs(input.backup.every_secs),
            keep: input.backup.keep,
        },
    };
    config.validate()?;
    Ok(config)
}

/// Builds the core form request after the adapter's own destination check.
fn build_form_request(input: ClusterFormInput) -> Result<ClusterFormRequest, SpiritError> {
    let config = build_config(&input)?;
    validate_backup_target(&input.backup_target)?;
    let name = ProjectName::parse(input.home_project.name.trim()).map_err(|_| {
        SpiritError::invalid_input("home_project.name", "must be the project slug VOWP stores")
    })?;
    let mut worker_projects = Vec::with_capacity(input.worker_projects.len());
    for mapping in &input.worker_projects {
        worker_projects.push(ClusterWorkerProject {
            worker: HostId(mapping.worker.clone()),
            project: ProjectName::parse(&mapping.project).map_err(|_| {
                SpiritError::invalid_input(
                    "worker_project.project",
                    "must be the project slug VOWP stores",
                )
            })?,
            root: mapping.root.trim().to_owned(),
            worktree_root: mapping
                .worktree_root
                .as_ref()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            allow: mapping.allow.clone(),
        });
    }
    let request = ClusterFormRequest {
        config,
        home_project: ClusterProject {
            name,
            root: input.home_project.root.trim().to_owned(),
        },
        worker_projects,
        backup_target: Some(input.backup_target.trim().to_owned()),
    };
    request.validate()?;
    Ok(request)
}

/// Removes one worker from the saved shape, keeping Home and at least one worker.
fn remove_worker(mut config: ClusterConfig, host_id: &str) -> Result<ClusterConfig, SpiritError> {
    if config.home.0 == host_id {
        return Err(SpiritError::invalid_input(
            "cluster.home",
            "the Home server is not a configured worker",
        ));
    }
    let before = config.workers.len();
    config.workers.retain(|worker| worker.host.0 != host_id);
    if config.workers.len() == before {
        return Err(SpiritError::not_found("cluster worker"));
    }
    if config.workers.is_empty() {
        return Err(SpiritError::new(
            SpiritErrorCode::Refused,
            "a cluster needs at least one worker",
        )
        .with_hint(
            "Re-form the cluster with another worker; Home's VOWP enrollment is not dissolved.",
        ));
    }
    config.validate()?;
    Ok(config)
}

/// Refuses a stale renderer that never asked the owner. Core's `form` has no token;
/// this is the desktop adapter's second, explicit yes in addition to the UI checkbox.
fn ensure_confirmed(confirmed: bool) -> Result<(), SpiritError> {
    if confirmed {
        Ok(())
    } else {
        Err(SpiritError::invalid_input(
            "confirmation",
            "confirm the SSH changes before forming a cluster",
        ))
    }
}

/// Forms a cluster after the owner's explicit confirmation.
#[tauri::command]
pub async fn cluster_form(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    input: ClusterFormInput,
) -> CommandResult<ClusterFormedView> {
    ensure_confirmed(input.confirmed)?;
    let request = build_form_request(input)?;
    let formed = service(&state).form(&request).await?;
    let store = ClusterStore::for_app(&app)?;
    let record = ClusterRecord {
        config: formed.config.clone(),
        formed_at: formed.formed_at,
        promotion: None,
    };
    // The cluster already exists on the servers at this point. Keep the error honest so
    // the owner knows to inspect Home instead of blindly re-forming.
    store.save(&record).map_err(|error| {
        error.with_hint(
            "VOWP changes already ran; inspect the servers before re-forming the cluster.",
        )
    })?;
    Ok(formed_view(&formed))
}

/// Reads the saved cluster and its pooled health from Home.
///
/// A failed read keeps the saved config and returns `health_error`, so the Server page
/// can still name the cluster instead of pretending it disappeared.
#[tauri::command]
pub async fn cluster_health(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> CommandResult<ClusterStateView> {
    let store = ClusterStore::for_app(&app)?;
    let Some(record) = store.load()? else {
        return Ok(ClusterStateView::unconfigured());
    };
    let config = config_view(&record.config);
    let (health, health_error) = match service(&state).health(&record.config).await {
        Ok(health) => (Some(health_view(&health)), None),
        Err(error) => (None, Some(error)),
    };
    Ok(ClusterStateView {
        configured: true,
        config: Some(config),
        health,
        health_error,
        promotion: record.promotion.as_ref().map(promotion_view),
    })
}

/// Promotes one worker's backup to Home; core re-checks the exact confirmation token.
#[tauri::command]
pub async fn cluster_promote(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    input: ClusterPromoteInput,
) -> CommandResult<ClusterPromoteOutcomeView> {
    let store = ClusterStore::for_app(&app)?;
    let Some(record) = store.load()? else {
        return Err(SpiritError::new(
            SpiritErrorCode::NotFound,
            "no cluster is configured on this machine",
        )
        .with_hint("Form a cluster before promoting a backup."));
    };
    let request = PromoteRequest {
        config: record.config.clone(),
        target: HostId(input.target),
        backup_dir: input.backup_dir.trim().to_owned(),
        backup_at: None,
        new_home: input
            .new_home
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        confirmation: input.confirmation,
    };
    let outcome: PromoteOutcome = service(&state).promote(&request).await?;
    let promotion = PromotionRecord {
        new_home: outcome.new_home.0.clone(),
        node_name: outcome.node_name.clone(),
        backup_at: outcome.backup_at,
        promoted_at: outcome.promoted_at,
    };
    let updated = ClusterRecord {
        config: record.config,
        formed_at: record.formed_at,
        promotion: Some(promotion),
    };
    store.save(&updated).map_err(|error| {
        error.with_hint("VOWP promoted the backup, but StackSpirit could not record it locally.")
    })?;
    Ok(ClusterPromoteOutcomeView {
        new_home: outcome.new_home.0,
        node_name: outcome.node_name,
        backup_at: outcome.backup_at,
        promoted_at: outcome.promoted_at,
    })
}

/// Reads the saved backup policy; null when no cluster is configured.
#[tauri::command]
pub fn cluster_backup_policy(app: tauri::AppHandle) -> CommandResult<Option<BackupPolicyView>> {
    let store = ClusterStore::for_app(&app)?;
    Ok(store
        .load()?
        .map(|record| policy_view(&record.config.backup)))
}

/// Removes one worker from the saved cluster. This is local-only and never revokes that
/// worker's VOWP agent credentials or Home's backup schedule.
#[tauri::command]
pub fn cluster_remove_worker(
    app: tauri::AppHandle,
    host_id: String,
) -> CommandResult<ClusterConfigView> {
    let store = ClusterStore::for_app(&app)?;
    let Some(record) = store.load()? else {
        return Err(SpiritError::new(
            SpiritErrorCode::NotFound,
            "no cluster is configured on this machine",
        ));
    };
    let config = remove_worker(record.config, &host_id)?;
    let updated = ClusterRecord {
        config: config.clone(),
        formed_at: record.formed_at,
        promotion: record.promotion,
    };
    store.save(&updated)?;
    Ok(config_view(&config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(host: &str, node: &str, role: ClusterWorkerRole) -> ClusterWorker {
        ClusterWorker {
            host: HostId(host.to_owned()),
            node_name: node.to_owned(),
            role,
            agent_state: ClusterAgentState::Enrolled,
        }
    }

    fn config() -> ClusterConfig {
        ClusterConfig {
            home: HostId("host-home".to_owned()),
            workers: vec![
                worker("host-worker-a", "worker-a", ClusterWorkerRole::Backup),
                worker("host-worker-b", "worker-b", ClusterWorkerRole::Worker),
            ],
            backup: BackupPolicy {
                every: Duration::from_secs(86_400),
                keep: 7,
            },
        }
    }

    fn form_input() -> ClusterFormInput {
        ClusterFormInput {
            home: "host-home".to_owned(),
            workers: vec![ClusterWorkerInput {
                host: "host-worker-a".to_owned(),
                node_name: "worker-a".to_owned(),
                role: ClusterWorkerRole::Backup,
            }],
            backup: BackupPolicyInput {
                every_secs: 86_400,
                keep: 7,
            },
            home_project: ClusterProjectInput {
                name: "stackspirit".to_owned(),
                root: "/srv/code/stackspirit".to_owned(),
            },
            worker_projects: Vec::new(),
            backup_target: "backup-host:/srv/vowp-backups".to_owned(),
            confirmed: true,
        }
    }

    #[test]
    fn backup_target_accepts_a_path_or_an_rsync_target() {
        assert!(validate_backup_target("/srv/vowp-backups").is_ok());
        assert!(validate_backup_target("backup-host:/srv/vowp-backups").is_ok());
        assert!(validate_backup_target("backup@backup-host:/srv/vowp-backups").is_ok());
    }

    #[test]
    fn backup_target_refuses_a_password_or_token() {
        let error = validate_backup_target("backup:pw@backup-host:/srv/vowp-backups").unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        assert!(error.message.contains("password or token"));
        assert!(validate_backup_target("https://example.invalid/backup").is_err());
    }

    #[test]
    fn form_request_refuses_a_sub_minute_schedule() {
        let mut input = form_input();
        input.backup.every_secs = 30;
        let error = build_form_request(input).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        assert!(error.message.contains("60 seconds"));
    }

    #[test]
    fn form_requires_the_explicit_confirmation() {
        assert!(ensure_confirmed(false).is_err());
        assert!(ensure_confirmed(true).is_ok());
    }

    #[test]
    fn form_request_carries_the_paths_only_in_the_request() {
        let request = build_form_request(form_input()).expect("valid request");
        assert_eq!(
            request.backup_target.as_deref(),
            Some("backup-host:/srv/vowp-backups")
        );
        assert_eq!(request.home_project.root, "/srv/code/stackspirit");
    }

    #[test]
    fn form_request_carries_worker_path_maps() {
        let mut input = form_input();
        input.worker_projects.push(ClusterWorkerProjectInput {
            worker: "host-worker-a".to_owned(),
            project: "stackspirit".to_owned(),
            root: "/srv/code/stackspirit".to_owned(),
            worktree_root: None,
            allow: vec!["crates".to_owned()],
        });
        let request = build_form_request(input).expect("valid request");
        assert_eq!(request.worker_projects.len(), 1);
        assert_eq!(request.worker_projects[0].worker.0, "host-worker-a");
        assert_eq!(request.worker_projects[0].root, "/srv/code/stackspirit");
        assert_eq!(request.worker_projects[0].allow, vec!["crates".to_owned()]);
    }

    #[test]
    fn form_request_refuses_a_mapping_for_a_non_worker() {
        let mut input = form_input();
        input.worker_projects.push(ClusterWorkerProjectInput {
            worker: "host-not-a-worker".to_owned(),
            project: "stackspirit".to_owned(),
            root: "/srv/code/stackspirit".to_owned(),
            worktree_root: None,
            allow: Vec::new(),
        });
        let error = build_form_request(input).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
    }

    #[test]
    fn remove_worker_refuses_home_and_the_last_worker() {
        let error = remove_worker(config(), "host-home").unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);

        let mut single = config();
        single.workers.truncate(1);
        let error = remove_worker(single, "host-worker-a").unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::Refused);
        assert!(error.message.contains("at least one worker"));
    }

    #[test]
    fn remove_worker_drops_only_the_named_worker() {
        let updated = remove_worker(config(), "host-worker-a").expect("remove one");
        assert_eq!(updated.workers.len(), 1);
        assert_eq!(updated.workers[0].host.0, "host-worker-b");
        assert_eq!(updated.home.0, "host-home");
    }

    #[test]
    fn config_view_has_no_paths_or_secrets() {
        let json = serde_json::to_string(&config_view(&config())).expect("serialize");
        assert!(json.contains("host-worker-a"));
        assert!(json.contains("worker-a"));
        assert!(!json.contains("/srv"));
        assert!(!json.contains("backup-host"));
        assert!(!json.contains("://"));
    }

    #[test]
    fn policy_view_uses_the_vowp_schedule_spec() {
        let view = policy_view(&config().backup);
        assert_eq!(view.every_secs, 86_400);
        assert_eq!(view.keep, 7);
        assert_eq!(view.schedule, "every 1d");
    }
}
