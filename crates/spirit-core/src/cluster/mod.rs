//! Pooled-compute and failover cluster core.
//!
//! A cluster is one **Home** server plus one or more **Worker/Backup** servers. Home
//! stays the single source of truth; workers are enrolled as outbound VOWP agent nodes
//! so ready jobs can run on Home and on every online worker at the same time. The model
//! here is deliberately headless: every remote action goes through the verified
//! [`SshRemote`] runner, every mutation is argv-only, and a pairing code is only ever
//! written to a child's stdin.
//!
//! The exact VOWP commands are documented in `docs/CLUSTER.md`. In particular, VOWP's
//! effective TOML key is `[offload] default` (the handoff calls the same setting
//! `default_placement`); this module writes `default = "any"` because VOWP rejects an
//! unknown key inside `[offload]`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::SecretString;
use crate::error::{SpiritError, SpiritErrorCode, code_for_exit};
use crate::host::{HostId, HostProfile, HostStore};
use crate::ssh::command::CommandOutput;
use crate::ssh::{CommandRunner, SshOptions, SshRemote};
use crate::time::{Clock, SystemClock, Timestamp};
use crate::wire::{
    CapacityReportDoc, FleetStatus, NodeName, NodePresence, NodeStatus, ProjectName,
    parse_capacity_report, parse_fleet_status,
};

/// The saved host-profile id used by the cluster model.
///
/// This is an alias for [`crate::host::HostId`]; the cluster name keeps the API
/// self-describing.
pub type HostProfileId = HostId;

/// The exact confirmation token a caller must pass to [`ClusterService::promote`].
///
/// The token is not a security boundary by itself; it is a second, explicit "yes" in
/// addition to the caller verifying the backup and the target host. Promotion never
/// runs without it.
pub const PROMOTE_CONFIRMATION_TOKEN: &str = "promote-backup-to-home";

/// The schedule name StackSpirit registers for Home backups.
pub const CLUSTER_BACKUP_SCHEDULE: &str = "cluster-backup";

/// The role of one worker server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterWorkerRole {
    /// A worker that only contributes pooled compute.
    #[default]
    Worker,
    /// A worker that also receives scheduled backups.
    Backup,
}

impl ClusterWorkerRole {
    /// Returns the stable snake_case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Backup => "backup",
        }
    }
}

/// The last-known agent state of one worker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterAgentState {
    /// The worker is enrolled, active and online.
    Enrolled,
    /// The Home has a pending enrollment code for the worker.
    #[default]
    Pending,
    /// The worker is enrolled but not currently reachable.
    Offline,
    /// The worker's credentials were revoked.
    Revoked,
    /// The Home has no node record for the worker.
    Missing,
    /// The state cannot be determined honestly.
    Unknown,
}

impl ClusterAgentState {
    /// Returns the stable snake_case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enrolled => "enrolled",
            Self::Pending => "pending",
            Self::Offline => "offline",
            Self::Revoked => "revoked",
            Self::Missing => "missing",
            Self::Unknown => "unknown",
        }
    }

    /// Maps one VOWP node status and presence pair to the cluster state.
    #[must_use]
    pub fn from_vowp(status: &NodeStatus, presence: &NodePresence, registered: bool) -> Self {
        if !registered && matches!(status, NodeStatus::Unknown(_)) {
            return Self::Missing;
        }
        if *status == NodeStatus::Revoked || *presence == NodePresence::Revoked {
            return Self::Revoked;
        }
        if *status == NodeStatus::Pending {
            return Self::Pending;
        }
        if *status == NodeStatus::Active {
            return if *presence == NodePresence::Online {
                Self::Enrolled
            } else if *presence == NodePresence::Offline || *presence == NodePresence::Degraded {
                Self::Offline
            } else {
                Self::Unknown
            };
        }
        Self::Unknown
    }
}

/// One worker or backup server of a cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterWorker {
    /// Saved host profile.
    pub host: HostProfileId,
    /// VOWP node name enrolled on Home.
    pub node_name: String,
    /// Pooled-compute or backup role.
    #[serde(default)]
    pub role: ClusterWorkerRole,
    /// Last-known agent state. Forming a cluster normalizes this to `enrolled`.
    #[serde(default)]
    pub agent_state: ClusterAgentState,
}

/// How often Home writes a backup and how many copies the owner expects to keep.
///
/// VOWP's own rotation keeps 7 daily and 4 weekly copies; `keep` is the owner's
/// expectation used by the UI and by promotion's freshness check documentation. It is
/// not passed to a VOWP command because `vowp backup` has no retention flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupPolicy {
    /// Interval between backup runs.
    pub every: Duration,
    /// Number of backup copies the owner expects to retain.
    pub keep: u32,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            every: Duration::from_secs(24 * 60 * 60),
            keep: 7,
        }
    }
}

impl BackupPolicy {
    /// Validates a policy before it becomes a schedule or a promotion guard.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if self.every.is_zero() {
            return Err(SpiritError::invalid_input(
                "backup.every",
                "must be a positive interval",
            ));
        }
        if self.every.subsec_nanos() != 0 {
            return Err(SpiritError::invalid_input(
                "backup.every",
                "must be a whole number of seconds",
            ));
        }
        if self.keep == 0 || self.keep > 1000 {
            return Err(SpiritError::invalid_input(
                "backup.keep",
                "must be between 1 and 1000",
            ));
        }
        Ok(())
    }

    /// Returns the VOWP schedule spec, for example `every 30m` or `every 1d`.
    pub fn schedule_spec(&self) -> Result<String, SpiritError> {
        self.validate()?;
        let secs = self.every.as_secs();
        let (count, unit) = if secs % 86_400 == 0 {
            (secs / 86_400, "d")
        } else if secs % 3_600 == 0 {
            (secs / 3_600, "h")
        } else if secs % 60 == 0 {
            (secs / 60, "m")
        } else {
            (secs, "s")
        };
        Ok(format!("every {count}{unit}"))
    }

    /// Returns the maximum age promotion accepts for a backup.
    #[must_use]
    pub fn max_age(&self) -> Duration {
        self.every
    }
}

/// The whole cluster shape saved by StackSpirit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// The single Home server.
    pub home: HostProfileId,
    /// Worker and backup servers.
    pub workers: Vec<ClusterWorker>,
    /// Scheduled backup policy.
    pub backup: BackupPolicy,
}

impl ClusterConfig {
    /// Validates host, node-name, role and backup rules without touching the network.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if self.home.0.trim().is_empty() {
            return Err(SpiritError::invalid_input(
                "cluster.home",
                "must be a saved host profile id",
            ));
        }
        self.backup.validate()?;
        if self.workers.is_empty() {
            return Err(SpiritError::invalid_input(
                "cluster.workers",
                "a cluster needs at least one worker server",
            ));
        }
        let mut hosts = BTreeSet::new();
        let mut nodes = BTreeSet::new();
        for worker in &self.workers {
            if worker.host == self.home {
                return Err(SpiritError::invalid_input(
                    "cluster.workers",
                    "a worker must not be the Home server",
                ));
            }
            if !hosts.insert(worker.host.clone()) {
                return Err(SpiritError::invalid_input(
                    "cluster.workers",
                    "a saved server may appear only once in a cluster",
                ));
            }
            let node = NodeName::parse(&worker.node_name)?;
            if !nodes.insert(node.to_string()) {
                return Err(SpiritError::invalid_input(
                    "cluster.workers",
                    "worker node names must be unique",
                ));
            }
        }
        Ok(())
    }

    /// Returns the first worker with the backup role, if any.
    #[must_use]
    pub fn first_backup_worker(&self) -> Option<&ClusterWorker> {
        self.workers
            .iter()
            .find(|worker| worker.role == ClusterWorkerRole::Backup)
    }
}

/// One absolute project checkout used by a fallible remote command.
///
/// The root never crosses into [`ClusterHealth`] and its `Debug` output redacts the
/// path, so a request cannot leak a checkout path through a log line.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct ClusterProject {
    /// Project slug.
    pub name: ProjectName,
    /// Absolute checkout path on the server.
    pub root: String,
}

impl fmt::Debug for ClusterProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClusterProject")
            .field("name", &self.name)
            .field("root", &"[remote-path]")
            .finish()
    }
}

/// One project mapping to add through `vowp agent paths add` on a worker.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct ClusterWorkerProject {
    /// Worker host the mapping belongs to.
    pub worker: HostProfileId,
    /// Project slug.
    pub project: ProjectName,
    /// Absolute checkout path on the worker.
    pub root: String,
    /// Optional absolute worktree root on the worker.
    #[serde(default)]
    pub worktree_root: Option<String>,
    /// Allowed relative paths below the root.
    #[serde(default)]
    pub allow: Vec<String>,
}

impl fmt::Debug for ClusterWorkerProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClusterWorkerProject")
            .field("worker", &self.worker)
            .field("project", &self.project)
            .field("root", &"[remote-path]")
            .field(
                "worktree_root",
                &self.worktree_root.as_ref().map(|_| "[remote-path]"),
            )
            .field("allow", &self.allow)
            .finish()
    }
}

/// Everything [`ClusterService::form`] needs beyond the saved cluster shape.
///
/// `home_project` names the VOWP project whose `vowp.toml` receives `[offload]`.
/// `backup_target` is the destination passed to `vowp backup --to`; it is required
/// because StackSpirit never invents a server path or rsync target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterFormRequest {
    /// The cluster shape.
    pub config: ClusterConfig,
    /// Home project whose config is edited.
    pub home_project: ClusterProject,
    /// Project path maps added on workers.
    pub worker_projects: Vec<ClusterWorkerProject>,
    /// Backup destination such as `backup@example:/srv/vowp-backups`.
    pub backup_target: Option<String>,
}

impl ClusterFormRequest {
    /// Validates every user-supplied value before the first remote command.
    pub fn validate(&self) -> Result<(), SpiritError> {
        self.config.validate()?;
        validate_remote_path(&self.home_project.root, "home project root")?;

        let worker_hosts: BTreeSet<&HostProfileId> = self
            .config
            .workers
            .iter()
            .map(|worker| &worker.host)
            .collect();
        let mut seen = BTreeSet::new();
        for mapping in &self.worker_projects {
            if !worker_hosts.contains(&mapping.worker) {
                return Err(SpiritError::invalid_input(
                    "worker project",
                    "the mapping names a host that is not a cluster worker",
                ));
            }
            validate_remote_path(&mapping.root, "worker project root")?;
            if let Some(worktree) = &mapping.worktree_root {
                validate_remote_path(worktree, "worker worktree root")?;
            }
            for allowed in &mapping.allow {
                validate_allowed_path(allowed)?;
            }
            if !seen.insert((mapping.worker.clone(), mapping.project.clone())) {
                return Err(SpiritError::invalid_input(
                    "worker project",
                    "a project may be mapped only once per worker",
                ));
            }
        }

        match self.backup_target.as_deref() {
            Some(target) => validate_backup_target(target),
            None => Err(SpiritError::invalid_input(
                "backup target",
                "is required to register the cluster-backup schedule",
            )),
        }
    }
}

/// The result of forming a cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterFormed {
    /// The normalized cluster shape; every worker is now `enrolled`.
    pub config: ClusterConfig,
    /// The registered schedule name.
    pub backup_schedule: String,
    /// The backup destination passed to VOWP.
    pub backup_target: String,
    /// True when the Home project config was rewritten.
    pub offload_config_changed: bool,
    /// When the form finished.
    pub formed_at: Timestamp,
}

/// Which host a health row describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterHostRole {
    /// The Home and its supervisor.
    Home,
    /// A pooled-compute worker.
    Worker,
    /// A worker that also receives backups.
    Backup,
}

/// Which source produced the pooled totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterTotalsSource {
    /// VOWP 0.5 reported the additive `cluster` card.
    VowpCluster,
    /// VOWP did not report it, so the totals were computed from Home plus the visible workers.
    Computed,
}

/// One safe per-host health row. It never carries a path, token or pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterHostHealth {
    /// Saved host profile.
    pub host: HostProfileId,
    /// Sanitized display label.
    pub label: String,
    /// Role in the cluster.
    pub role: ClusterHostRole,
    /// VOWP node name, when known.
    pub node_name: Option<String>,
    /// Enrollment and connection state.
    pub agent_state: ClusterAgentState,
    /// VOWP node status, or `unknown` when the Home has no record.
    pub status: NodeStatus,
    /// VOWP computed presence.
    pub presence: NodePresence,
    /// Whether `vowp node ls` returned a row for this node.
    pub registered: bool,
    /// Busy worker slots (or Home running runs).
    pub slots_busy: u32,
    /// Maximum worker slots (or the Home target).
    pub slots_max: u32,
    /// Mapped project slugs.
    pub mapped_projects: Vec<String>,
    /// Last successful contact, when known.
    pub last_seen_at: Option<Timestamp>,
    /// Sanitized last error.
    pub last_error: Option<String>,
}

/// The combined cluster read model returned by [`ClusterService::health`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterHealth {
    /// Home node name from the current snapshot.
    pub home_node: String,
    /// Number of active, online worker nodes (Home is not counted).
    pub online_workers: u32,
    /// Home's own worker target.
    pub home_target_workers: u32,
    /// Maximum slots contributed by online workers.
    pub worker_slots: u32,
    /// Home target plus worker slots, from VOWP's cluster card when present.
    pub pooled_target_workers: u32,
    /// Home running runs plus worker busy slots, from VOWP's cluster card when present.
    pub busy_workers: u32,
    /// Whether VOWP reported the additive `cluster` card.
    pub cluster_totals_present: bool,
    /// Where the pooled totals came from.
    pub totals_source: ClusterTotalsSource,
    /// One row for Home and one for every configured worker.
    pub hosts: Vec<ClusterHostHealth>,
    /// Honest degradation notes; these never include a path or token.
    pub warnings: Vec<String>,
    /// When the read model was produced.
    pub checked_at: Timestamp,
}

/// The additive VOWP 0.5 `cluster` card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterCapacityTotals {
    /// Online, active agent worker nodes (Home is not counted).
    #[serde(default)]
    pub online_worker_nodes: u32,
    /// Home target plus every online worker's max slots.
    #[serde(default)]
    pub target_workers: u32,
    /// Home running runs plus every online worker's busy slots.
    #[serde(default)]
    pub busy_workers: u32,
    /// Logical CPU cores across the cluster.
    #[serde(default)]
    pub cpu_cores: u32,
    /// Total physical memory across the cluster.
    #[serde(default)]
    pub mem_total_bytes: u64,
    /// Available memory across the cluster.
    #[serde(default)]
    pub mem_available_bytes: u64,
    /// Free work-filesystem bytes across the cluster.
    #[serde(default)]
    pub disk_free_bytes: u64,
}

/// A request to promote a backup on one worker to a new Home.
#[derive(Clone, PartialEq, Eq)]
pub struct PromoteRequest {
    /// The cluster shape.
    pub config: ClusterConfig,
    /// Worker that holds the backup and will become the new Home.
    pub target: HostProfileId,
    /// Absolute backup directory on the target.
    pub backup_dir: String,
    /// Known backup timestamp. When absent, the service reads the directory mtime.
    pub backup_at: Option<Timestamp>,
    /// Fresh home directory on the target. Defaults to the target profile's `vowp_home`.
    pub new_home: Option<String>,
    /// Must equal [`PROMOTE_CONFIRMATION_TOKEN`].
    pub confirmation: String,
}

impl fmt::Debug for PromoteRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromoteRequest")
            .field("config", &self.config)
            .field("target", &self.target)
            .field("backup_dir", &"[remote-path]")
            .field("backup_at", &self.backup_at)
            .field("new_home", &self.new_home.as_ref().map(|_| "[remote-path]"))
            .field("confirmation", &self.confirmation)
            .finish()
    }
}

/// The new Home identity returned by [`ClusterService::promote`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromoteOutcome {
    /// Host profile that now serves the Home.
    pub new_home: HostProfileId,
    /// VOWP node name restored from the backup.
    pub node_name: String,
    /// Fresh home directory on the new Home.
    pub home_dir: String,
    /// Timestamp of the promoted backup.
    pub backup_at: Timestamp,
    /// When the promotion finished.
    pub promoted_at: Timestamp,
}

/// The headless cluster service.
///
/// It borrows the same [`HostStore`] and verified SSH runner the rest of the core uses.
/// Tests inject [`crate::ssh::FakeCommandRunner`]; production injects the system runner.
pub struct ClusterService {
    hosts: Arc<dyn HostStore>,
    runner: Arc<dyn CommandRunner>,
    known_hosts: PathBuf,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for ClusterService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClusterService")
            .field("known_hosts", &"[path]")
            .finish_non_exhaustive()
    }
}

impl ClusterService {
    /// Builds a service from the host store, the verified SSH runner and the app-private
    /// `known_hosts` path.
    #[must_use]
    pub fn new(
        hosts: Arc<dyn HostStore>,
        runner: Arc<dyn CommandRunner>,
        known_hosts: impl Into<PathBuf>,
    ) -> Self {
        Self {
            hosts,
            runner,
            known_hosts: known_hosts.into(),
            clock: Arc::new(SystemClock),
        }
    }

    /// Replaces the clock (tests and previews).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Forms a cluster: enrolls every worker, enables pooled compute on Home and
    /// registers the scheduled backup.
    ///
    /// No mutation starts until every worker passes a read-only VOWP preflight, the
    /// Home project config has been read, and every input has been validated. A
    /// pairing code is created on Home and written only to the worker's
    /// `vowp agent enroll --code-file -` stdin; it never appears in argv, `Debug`, an
    /// error or a log.
    pub async fn form(&self, request: &ClusterFormRequest) -> Result<ClusterFormed, SpiritError> {
        request.validate()?;

        let saved = self.hosts.list()?;
        if saved.len() < 2 {
            return Err(SpiritError::new(
                SpiritErrorCode::Refused,
                "a cluster needs at least two saved servers",
            )
            .with_hint("Save the Home server and at least one Worker server first."));
        }

        let home = self.require_host(&request.config.home)?;
        if home.ssh.port != 22 {
            return Err(SpiritError::invalid_input(
                "home.ssh.port",
                "vowp agent enroll --target carries no SSH port; use port 22 or an SSH alias",
            )
            .with_hint("VOWP joins the Home with `ssh <target>`; a non-default port needs an SSH config alias on every worker."));
        }
        let mut worker_profiles = Vec::with_capacity(request.config.workers.len());
        for worker in &request.config.workers {
            let profile = self.require_host(&worker.host)?;
            if profile.ssh == home.ssh {
                return Err(SpiritError::invalid_input(
                    "cluster.workers",
                    "a worker must not use the Home SSH target",
                ));
            }
            worker_profiles.push((worker, profile));
        }

        let home_remote = self.remote_for(&home);
        self.preflight(&home_remote, "the Home").await?;
        for (worker, profile) in &worker_profiles {
            let remote = self.remote_for(profile);
            self.preflight(&remote, &format!("worker {}", worker.node_name))
                .await?;
        }

        let (config_path, current_config) =
            read_project_config(&home_remote, &request.home_project.root).await?;
        let (next_config, offload_config_changed) = enable_pooled_compute(&current_config)?;

        let backup_target = request
            .backup_target
            .as_deref()
            .ok_or_else(|| SpiritError::invalid_input("backup target", "is required"))?;

        let mut enrolled = Vec::with_capacity(worker_profiles.len());
        for (worker, profile) in &worker_profiles {
            let worker_remote = self.remote_for(profile);

            let enroll_argv = node_enroll_argv(&home, &worker.node_name);
            let output =
                run_checked(&home_remote, &enroll_argv, ENROLL_TIMEOUT, "node enroll").await?;
            let code = parse_node_enrollment(&output.stdout, &worker.node_name)?;

            let agent_argv = agent_enroll_argv(&home);
            let mut stdin = code.expose().as_bytes().to_vec();
            stdin.push(b'\n');
            let enroll_output = run_stdin_checked(
                &worker_remote,
                &agent_argv,
                stdin,
                ENROLL_TIMEOUT,
                "agent enroll",
            )
            .await?;
            parse_agent_enrollment(&enroll_output.stdout, &worker.node_name)?;

            let install_argv = plain_vowp_argv(&["--json", "agent", "service", "install", "--now"]);
            run_checked(
                &worker_remote,
                &install_argv,
                SERVICE_INSTALL_TIMEOUT,
                "agent service install",
            )
            .await?;

            for mapping in request
                .worker_projects
                .iter()
                .filter(|mapping| mapping.worker == worker.host)
            {
                let paths_argv = agent_paths_add_argv(mapping);
                run_checked(
                    &worker_remote,
                    &paths_argv,
                    PATHS_TIMEOUT,
                    "agent paths add",
                )
                .await?;
            }

            enrolled.push(ClusterWorker {
                host: worker.host.clone(),
                node_name: worker.node_name.clone(),
                role: worker.role,
                agent_state: ClusterAgentState::Enrolled,
            });
        }

        if offload_config_changed {
            write_remote_text(&home_remote, &config_path, next_config.as_bytes()).await?;
        }

        let schedule_argv = schedule_backup_argv(
            &home,
            &request.home_project.name,
            &request.config.backup,
            backup_target,
        )?;
        run_checked(
            &home_remote,
            &schedule_argv,
            SCHEDULE_TIMEOUT,
            "schedule add",
        )
        .await?;

        let mut formed_config = request.config.clone();
        formed_config.workers = enrolled;

        Ok(ClusterFormed {
            config: formed_config,
            backup_schedule: CLUSTER_BACKUP_SCHEDULE.to_owned(),
            backup_target: backup_target.to_owned(),
            offload_config_changed,
            formed_at: self.clock.now(),
        })
    }

    /// Reads the four VOWP documents that describe cluster health.
    ///
    /// `vowp 0.5` adds the `cluster` card to `fleet status` and `capacity report`.
    /// When it is absent, the pooled totals are computed from Home plus the visible
    /// worker rows and [`ClusterHealth::warnings`] says so explicitly.
    pub async fn health(&self, config: &ClusterConfig) -> Result<ClusterHealth, SpiritError> {
        config.validate()?;
        let home = self.require_host(&config.home)?;
        let home_remote = self.remote_for(&home);

        let fleet_output = run_checked(
            &home_remote,
            &vowp_argv(&home, &["--json", "fleet", "status"]),
            READ_TIMEOUT,
            "fleet status",
        )
        .await?;
        let fleet = parse_fleet_status(&fleet_output.stdout)?.value;

        let nodes_output = run_checked(
            &home_remote,
            &vowp_argv(&home, &["--json", "node", "ls"]),
            READ_TIMEOUT,
            "node ls",
        )
        .await?;
        let nodes: Vec<NodeListRecord> = serde_json::from_slice(&nodes_output.stdout)
            .map_err(|err| SpiritError::parse("node ls", err))?;

        let capacity_output = run_checked(
            &home_remote,
            &vowp_argv(&home, &["--json", "capacity", "report"]),
            READ_TIMEOUT,
            "capacity report",
        )
        .await?;
        let capacity: CapacityReportDoc = parse_capacity_report(&capacity_output.stdout)?.value;

        let offload_output = run_checked(
            &home_remote,
            &vowp_argv(&home, &["--json", "offload", "status"]),
            READ_TIMEOUT,
            "offload status",
        )
        .await?;
        let offload: OffloadStatusDoc = serde_json::from_slice(&offload_output.stdout)
            .map_err(|err| SpiritError::parse("offload status", err))?;

        let cluster_totals = cluster_totals_from_fleet(&fleet_output.stdout)?
            .or(cluster_totals_from_capacity(&capacity_output.stdout)?);

        Ok(build_health(
            config,
            &fleet,
            &nodes,
            &capacity,
            &offload,
            cluster_totals,
            self.clock.now(),
        ))
    }

    /// Promotes one backup on a worker to a new Home.
    ///
    /// The caller must pass [`PROMOTE_CONFIRMATION_TOKEN`]. A backup older than
    /// `config.backup.every` is refused before any command runs. The old Home is
    /// drained first; if that cannot be confirmed, promotion refuses rather than risk
    /// two live Homes.
    pub async fn promote(&self, request: &PromoteRequest) -> Result<PromoteOutcome, SpiritError> {
        request.config.validate()?;
        if request.confirmation.trim() != PROMOTE_CONFIRMATION_TOKEN {
            return Err(SpiritError::new(
                SpiritErrorCode::Refused,
                "promote requires the caller's explicit confirmation token",
            )
            .with_hint(format!(
                "Pass `{PROMOTE_CONFIRMATION_TOKEN}` only after verifying the backup and target."
            )));
        }
        let target_worker = request
            .config
            .workers
            .iter()
            .find(|worker| worker.host == request.target)
            .ok_or_else(|| {
                SpiritError::invalid_input(
                    "promote.target",
                    "must be one of the configured cluster workers",
                )
            })?;
        validate_remote_path(&request.backup_dir, "promote.backup_dir")?;

        let now = self.clock.now();
        if let Some(backup_at) = request.backup_at {
            ensure_backup_fresh(now, backup_at, &request.config.backup)?;
        }

        let home = self.require_host(&request.config.home)?;
        let target = self.require_host(&request.target)?;
        let new_home = request
            .new_home
            .clone()
            .or_else(|| target.vowp_home.clone())
            .ok_or_else(|| {
                SpiritError::invalid_input(
                    "promote.new_home",
                    "is required when the target profile has no vowp_home",
                )
            })?;
        validate_remote_path(&new_home, "promote.new_home")?;

        let target_remote = self.remote_for(&target);
        self.preflight(
            &target_remote,
            &format!("target {}", target_worker.node_name),
        )
        .await?;

        // Prove the restore source exists before the destructive Home drain. The
        // directory mtime is also the fallback age when the caller did not supply one.
        let stat_argv = vec![
            "stat".to_owned(),
            "-c".to_owned(),
            "%Y".to_owned(),
            request.backup_dir.clone(),
        ];
        let stat_output =
            run_checked(&target_remote, &stat_argv, READ_TIMEOUT, "backup stat").await?;
        let stat_at = parse_unix_seconds(&stat_output.stdout, "backup stat")?;
        let backup_at = request.backup_at.unwrap_or(stat_at);
        ensure_backup_fresh(now, backup_at, &request.config.backup)?;

        let home_remote = self.remote_for(&home);
        run_checked(
            &home_remote,
            &vowp_argv(&home, &["down", "--drain"]),
            DRAIN_TIMEOUT,
            "vowp down --drain",
        )
        .await?;

        let restore_argv = vec![
            crate::ssh::command::ENGINE_BINARY.to_owned(),
            "backup".to_owned(),
            "--restore".to_owned(),
            request.backup_dir.clone(),
            "--to-home".to_owned(),
            new_home.clone(),
        ];
        run_checked(
            &target_remote,
            &restore_argv,
            RESTORE_TIMEOUT,
            "backup restore",
        )
        .await?;

        run_checked(
            &target_remote,
            &vowp_home_argv(&new_home, &["up"]),
            UP_TIMEOUT,
            "vowp up",
        )
        .await?;

        let status_output = run_checked(
            &target_remote,
            &vowp_home_argv(&new_home, &["--json", "fleet", "status"]),
            READ_TIMEOUT,
            "fleet status after promotion",
        )
        .await?;
        let status = parse_fleet_status(&status_output.stdout)?.value;

        Ok(PromoteOutcome {
            new_home: request.target.clone(),
            node_name: status.snapshot.home.to_string(),
            home_dir: new_home,
            backup_at,
            promoted_at: self.clock.now(),
        })
    }

    fn require_host(&self, id: &HostProfileId) -> Result<HostProfile, SpiritError> {
        self.hosts
            .get(id)?
            .ok_or_else(|| SpiritError::not_found(&format!("host profile {id}")))
    }

    fn remote_for(&self, host: &HostProfile) -> SshRemote {
        let options = SshOptions::new(self.known_hosts.clone())
            .with_identity_file(host.identity_file.clone());
        SshRemote::new(self.runner.clone(), host.ssh.clone(), options)
    }

    async fn preflight(&self, remote: &SshRemote, label: &str) -> Result<(), SpiritError> {
        let argv = vec![
            crate::ssh::command::ENGINE_BINARY.to_owned(),
            "--version".to_owned(),
        ];
        let output = remote.run_with_timeout(&argv, PREFLIGHT_TIMEOUT).await?;
        check_cluster_output(output, &format!("{label} VOWP preflight"))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Wire helpers
// ---------------------------------------------------------------------------

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(20);
const ENROLL_TIMEOUT: Duration = Duration::from_secs(120);
const SERVICE_INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
const PATHS_TIMEOUT: Duration = Duration::from_secs(30);
const SCHEDULE_TIMEOUT: Duration = Duration::from_secs(30);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const UP_TIMEOUT: Duration = Duration::from_secs(120);
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const CONFIG_CAP_BYTES: usize = 1024 * 1024;
const MAX_REMOTE_PATH: usize = 4096;

#[derive(Deserialize)]
struct NodeEnrollOutput {
    node: String,
    code: String,
    #[serde(default)]
    #[allow(dead_code)]
    expires_at: Option<Timestamp>,
    #[serde(default)]
    #[allow(dead_code)]
    command: Option<String>,
}

impl fmt::Debug for NodeEnrollOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeEnrollOutput")
            .field("node", &self.node)
            .field("code", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("command", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct AgentEnrollOutput {
    node: String,
    #[serde(default)]
    #[allow(dead_code)]
    home: Option<String>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    heartbeat_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NodeListRecord {
    name: NodeName,
    #[serde(default)]
    last_seen_at: Option<Timestamp>,
    #[serde(default)]
    #[allow(dead_code)]
    is_local: bool,
}

#[derive(Debug, Default, Deserialize)]
struct OffloadStatusDoc {
    #[serde(default)]
    nodes: Vec<OffloadNodeRow>,
}

#[derive(Debug, Deserialize)]
struct OffloadNodeRow {
    name: NodeName,
    status: NodeStatus,
    presence: NodePresence,
    #[serde(default)]
    mapped: Vec<ProjectName>,
    #[serde(default)]
    slots_busy: Option<u32>,
    #[serde(default)]
    slots_max: Option<u32>,
    #[serde(default)]
    last_error: Option<String>,
}

fn plain_vowp_argv(rest: &[&str]) -> Vec<String> {
    let mut argv = vec![crate::ssh::command::ENGINE_BINARY.to_owned()];
    argv.extend(rest.iter().map(|value| (*value).to_owned()));
    argv
}

fn vowp_argv(home: &HostProfile, rest: &[&str]) -> Vec<String> {
    let mut argv = vec![crate::ssh::command::ENGINE_BINARY.to_owned()];
    if let Some(path) = &home.vowp_home {
        argv.push("--home".to_owned());
        argv.push(path.clone());
    }
    argv.extend(rest.iter().map(|value| (*value).to_owned()));
    argv
}

fn vowp_home_argv(home_dir: &str, rest: &[&str]) -> Vec<String> {
    let mut argv = vec![
        crate::ssh::command::ENGINE_BINARY.to_owned(),
        "--home".to_owned(),
        home_dir.to_owned(),
    ];
    argv.extend(rest.iter().map(|value| (*value).to_owned()));
    argv
}

fn node_enroll_argv(home: &HostProfile, node_name: &str) -> Vec<String> {
    vowp_argv(home, &["--json", "node", "enroll", node_name])
}

fn agent_enroll_argv(home: &HostProfile) -> Vec<String> {
    vec![
        crate::ssh::command::ENGINE_BINARY.to_owned(),
        "--json".to_owned(),
        "agent".to_owned(),
        "enroll".to_owned(),
        "--target".to_owned(),
        home.ssh.destination(),
        "--code-file".to_owned(),
        "-".to_owned(),
    ]
}

fn agent_paths_add_argv(mapping: &ClusterWorkerProject) -> Vec<String> {
    let mut argv = vec![
        crate::ssh::command::ENGINE_BINARY.to_owned(),
        "--json".to_owned(),
        "agent".to_owned(),
        "paths".to_owned(),
        "add".to_owned(),
        "--project".to_owned(),
        mapping.root.clone(),
        "--name".to_owned(),
        mapping.project.to_string(),
    ];
    if let Some(worktree) = &mapping.worktree_root {
        argv.push("--worktree-root".to_owned());
        argv.push(worktree.clone());
    }
    for allowed in &mapping.allow {
        argv.push("--allow".to_owned());
        argv.push(allowed.clone());
    }
    argv
}

fn schedule_backup_argv(
    home: &HostProfile,
    project: &ProjectName,
    policy: &BackupPolicy,
    backup_target: &str,
) -> Result<Vec<String>, SpiritError> {
    let spec = policy.schedule_spec()?;
    let mut argv = vec![crate::ssh::command::ENGINE_BINARY.to_owned()];
    if let Some(path) = &home.vowp_home {
        argv.push("--home".to_owned());
        argv.push(path.clone());
    }
    argv.extend([
        "--project".to_owned(),
        project.to_string(),
        "--json".to_owned(),
        "schedule".to_owned(),
        "add".to_owned(),
        spec,
        "command".to_owned(),
        "--name".to_owned(),
        CLUSTER_BACKUP_SCHEDULE.to_owned(),
        "--".to_owned(),
        crate::ssh::command::engine_binary_for(&home.ssh.destination()).to_owned(),
    ]);
    if let Some(path) = &home.vowp_home {
        argv.push("--home".to_owned());
        argv.push(path.clone());
    }
    argv.extend([
        "backup".to_owned(),
        "--to".to_owned(),
        backup_target.to_owned(),
    ]);
    Ok(argv)
}

fn parse_node_enrollment(stdout: &[u8], expected_node: &str) -> Result<SecretString, SpiritError> {
    let NodeEnrollOutput { node, code, .. } =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("node enroll", err))?;
    if node != expected_node {
        return Err(SpiritError::vowp_incompatible(
            "node enroll",
            "VOWP enrolled a different node name",
        ));
    }
    if code.is_empty() || code.trim() != code || code.chars().any(char::is_control) {
        return Err(SpiritError::new(
            SpiritErrorCode::VowpIncompatible,
            "node enroll returned an unusable one-time code",
        ));
    }
    Ok(SecretString::new(code))
}

fn parse_agent_enrollment(stdout: &[u8], expected_node: &str) -> Result<(), SpiritError> {
    let AgentEnrollOutput { node, protocol, .. } =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("agent enroll", err))?;
    if node != expected_node {
        return Err(SpiritError::vowp_incompatible(
            "agent enroll",
            "the worker enrolled under a different node name",
        ));
    }
    if let Some(protocol) = protocol {
        if !protocol.starts_with("vowp-agent/") {
            return Err(SpiritError::vowp_incompatible(
                "agent enroll",
                format!("unexpected agent protocol {protocol:?}"),
            ));
        }
    }
    Ok(())
}

fn cluster_totals_from_fleet(stdout: &[u8]) -> Result<Option<ClusterCapacityTotals>, SpiritError> {
    let value: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("fleet status", err))?;
    let cluster = value
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("capacity"))
        .and_then(|capacity| capacity.get("cluster"));
    parse_cluster_value(cluster, "fleet status")
}

fn cluster_totals_from_capacity(
    stdout: &[u8],
) -> Result<Option<ClusterCapacityTotals>, SpiritError> {
    let value: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|err| SpiritError::parse("capacity report", err))?;
    parse_cluster_value(value.get("cluster"), "capacity report")
}

fn parse_cluster_value(
    value: Option<&serde_json::Value>,
    document: &str,
) -> Result<Option<ClusterCapacityTotals>, SpiritError> {
    match value {
        Some(value) if !value.is_null() => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|err| SpiritError::parse(document, err)),
        _ => Ok(None),
    }
}

fn build_health(
    config: &ClusterConfig,
    fleet: &FleetStatus,
    nodes: &[NodeListRecord],
    capacity: &CapacityReportDoc,
    offload: &OffloadStatusDoc,
    cluster: Option<ClusterCapacityTotals>,
    now: Timestamp,
) -> ClusterHealth {
    let node_rows: BTreeMap<&str, &NodeListRecord> = nodes
        .iter()
        .map(|record| (record.name.as_str(), record))
        .collect();
    let offload_rows: BTreeMap<&str, &OffloadNodeRow> = offload
        .nodes
        .iter()
        .map(|row| (row.name.as_str(), row))
        .collect();

    let home_running = fleet.snapshot.supervisor.running;
    let home_target = capacity.report.target.target_workers;
    let home_busy = capacity.report.running.runs;
    let home_node = fleet.snapshot.home.to_string();
    let home_record = node_rows.get(fleet.snapshot.home.as_str()).copied();
    let mut warnings = Vec::new();
    if fleet.stale {
        warnings.push(
            "VOWP fleet status is stale; refresh before acting on these health numbers".to_owned(),
        );
    }
    if fleet.source_is_cache() {
        warnings.push(
            "VOWP fleet status came from the cache; the live Home was not re-read".to_owned(),
        );
    }
    let mut hosts = vec![ClusterHostHealth {
        host: config.home.clone(),
        label: "Home".to_owned(),
        role: ClusterHostRole::Home,
        node_name: Some(home_node.clone()),
        agent_state: if home_running {
            ClusterAgentState::Enrolled
        } else {
            ClusterAgentState::Offline
        },
        status: if home_running {
            NodeStatus::Active
        } else {
            NodeStatus::Unknown("down".to_owned())
        },
        presence: if home_running {
            NodePresence::Online
        } else {
            NodePresence::Offline
        },
        registered: home_record.is_some(),
        slots_busy: home_busy,
        slots_max: home_target,
        mapped_projects: fleet
            .snapshot
            .projects
            .iter()
            .map(|project| project.name.to_string())
            .collect(),
        last_seen_at: home_record.and_then(|record| record.last_seen_at),
        last_error: None,
    }];

    let mut computed_online = 0_u32;
    let mut computed_slots = 0_u32;
    let mut computed_busy = 0_u32;
    for worker in &config.workers {
        let row = offload_rows.get(worker.node_name.as_str()).copied();
        let record = node_rows.get(worker.node_name.as_str()).copied();
        let registered = record.is_some();
        let status = sanitize_node_status(
            row.map_or_else(|| NodeStatus::parse("unknown"), |row| row.status.clone()),
        );
        let presence = sanitize_node_presence(row.map_or_else(
            || {
                if registered {
                    NodePresence::Offline
                } else {
                    NodePresence::Unknown("unknown".to_owned())
                }
            },
            |row| row.presence.clone(),
        ));
        let agent_state = ClusterAgentState::from_vowp(&status, &presence, registered);
        let online = status == NodeStatus::Active && presence == NodePresence::Online;
        let slots_busy = row.and_then(|row| row.slots_busy).unwrap_or(0);
        let slots_max = row.and_then(|row| row.slots_max).unwrap_or(0);
        if online {
            computed_online = computed_online.saturating_add(1);
            computed_slots = computed_slots.saturating_add(slots_max);
            computed_busy = computed_busy.saturating_add(slots_busy);
        }

        if worker.node_name == home_node {
            warnings.push(format!(
                "worker {} uses the Home node name; VOWP cannot keep two node records with that name",
                worker.node_name
            ));
        }
        if !registered {
            warnings.push(format!(
                "worker {} is configured but `vowp node ls` has no record for it",
                worker.node_name
            ));
        }

        let role = match worker.role {
            ClusterWorkerRole::Worker => ClusterHostRole::Worker,
            ClusterWorkerRole::Backup => ClusterHostRole::Backup,
        };
        hosts.push(ClusterHostHealth {
            host: worker.host.clone(),
            label: safe_cluster_text(&format!("worker {}", worker.node_name)),
            role,
            node_name: Some(worker.node_name.clone()),
            agent_state,
            status,
            presence,
            registered,
            slots_busy,
            slots_max,
            mapped_projects: row
                .map(|row| {
                    row.mapped
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            last_seen_at: record.and_then(|record| record.last_seen_at),
            last_error: row
                .and_then(|row| row.last_error.as_deref())
                .map(safe_cluster_text),
        });
    }

    let (online_workers, worker_slots, pooled_target_workers, busy_workers, source) = if let Some(
        cluster,
    ) =
        &cluster
    {
        (
            cluster.online_worker_nodes,
            cluster.target_workers.saturating_sub(home_target),
            cluster.target_workers,
            cluster.busy_workers,
            ClusterTotalsSource::VowpCluster,
        )
    } else {
        warnings.push(
                "VOWP did not report pooled cluster totals; the pooled numbers are computed from Home and the visible worker nodes"
                    .to_owned(),
            );
        (
            computed_online,
            computed_slots,
            home_target.saturating_add(computed_slots),
            home_busy.saturating_add(computed_busy),
            ClusterTotalsSource::Computed,
        )
    };

    ClusterHealth {
        home_node,
        online_workers,
        home_target_workers: home_target,
        worker_slots,
        pooled_target_workers,
        busy_workers,
        cluster_totals_present: cluster.is_some(),
        totals_source: source,
        hosts,
        warnings,
        checked_at: now,
    }
}

fn ensure_backup_fresh(
    now: Timestamp,
    backup_at: Timestamp,
    policy: &BackupPolicy,
) -> Result<(), SpiritError> {
    let age = now.since(backup_at);
    if age > policy.max_age() {
        return Err(SpiritError::new(
            SpiritErrorCode::Refused,
            format!(
                "backup is stale: it is {}s old and the policy allows {}s",
                age.as_secs(),
                policy.max_age().as_secs()
            ),
        )
        .with_hint("Run or copy a newer `vowp backup` before promoting it."));
    }
    Ok(())
}

fn parse_unix_seconds(stdout: &[u8], document: &str) -> Result<Timestamp, SpiritError> {
    let text = String::from_utf8_lossy(stdout);
    let secs = text
        .trim()
        .parse::<u64>()
        .map_err(|_| SpiritError::parse(document, "expected one Unix-seconds value"))?;
    let secs = i64::try_from(secs)
        .map_err(|_| SpiritError::parse(document, "timestamp is out of range"))?;
    Ok(Timestamp::from_unix_secs(secs))
}

fn project_config_path(root: &str, file: &str) -> Result<String, SpiritError> {
    validate_remote_path(root, "home project root")?;
    let path = format!("{}/{file}", root.trim_end_matches('/'));
    validate_remote_path(&path, "home project config")?;
    Ok(path)
}

/// Reads the Home project config, preferring `stackspirit.toml` and falling back to the
/// legacy `vowp.toml`. Returns the path that was read so a write goes back to the same file.
async fn read_project_config(
    remote: &SshRemote,
    root: &str,
) -> Result<(String, String), SpiritError> {
    let preferred = project_config_path(root, crate::ssh::command::ENGINE_CONFIG_FILE)?;
    if let Ok(text) = read_remote_text(remote, &preferred).await {
        return Ok((preferred, text));
    }
    let legacy = project_config_path(root, crate::ssh::command::LEGACY_ENGINE_CONFIG_FILE)?;
    let text = read_remote_text(remote, &legacy).await?;
    Ok((legacy, text))
}

fn validate_remote_path(path: &str, field: &str) -> Result<(), SpiritError> {
    if !path.starts_with('/')
        || path.len() < 2
        || path.len() > MAX_REMOTE_PATH
        || path.ends_with('/')
        || path.contains("//")
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return Err(SpiritError::invalid_input(
            field,
            "must be an absolute POSIX path without '.', '..' or control characters",
        ));
    }
    Ok(())
}

fn validate_allowed_path(path: &str) -> Result<(), SpiritError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|component| component == "..")
    {
        return Err(SpiritError::invalid_input(
            "allow",
            "must be a relative path below the project root",
        ));
    }
    Ok(())
}

fn validate_backup_target(target: &str) -> Result<(), SpiritError> {
    if target.is_empty()
        || target.len() > MAX_REMOTE_PATH
        || target.starts_with('-')
        || target.chars().any(char::is_control)
    {
        return Err(SpiritError::invalid_input(
            "backup target",
            "must be one non-empty path or rsync target without control characters",
        ));
    }
    Ok(())
}

/// Enables pooled compute in one `vowp.toml` text.
///
/// Only the three `[offload]` keys StackSpirit owns are changed; every other line is
/// preserved byte-for-byte (including unrelated `[offload.*]` tables). VOWP's real key
/// is `default`, not the handoff's conceptual `default_placement`.
pub fn enable_pooled_compute(config: &str) -> Result<(String, bool), SpiritError> {
    const KEYS: [(&str, &str); 3] = [
        ("enabled", "true"),
        ("default", "\"any\""),
        ("result_transport", "\"bundle\""),
    ];
    let trailing_newline = config.ends_with('\n');
    let mut lines: Vec<String> = config.lines().map(str::to_owned).collect();

    let Some(header) = lines.iter().position(|line| is_offload_header(line)) else {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("[offload]".to_owned());
        for (key, value) in KEYS {
            lines.push(format!("{key} = {value}"));
        }
        let next = join_lines(lines, trailing_newline || config.is_empty());
        return Ok((next, true));
    };

    let end = lines
        .iter()
        .enumerate()
        .skip(header + 1)
        .find(|(_, line)| line.trim_start().starts_with('['))
        .map_or(lines.len(), |(index, _)| index);

    let mut found = [false; 3];
    for line in &mut lines[header + 1..end] {
        let trimmed = line.trim_start();
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        for (index, (name, value)) in KEYS.iter().enumerate() {
            if key == *name {
                let indent = &line[..line.len() - trimmed.len()];
                *line = format!("{indent}{name} = {value}");
                found[index] = true;
                break;
            }
        }
    }

    let missing: Vec<String> = KEYS
        .iter()
        .enumerate()
        .filter(|(index, _)| !found[*index])
        .map(|(_, (key, value))| format!("{key} = {value}"))
        .collect();
    if !missing.is_empty() {
        let mut insert_at = end;
        while insert_at > header + 1 && lines[insert_at - 1].trim().is_empty() {
            insert_at -= 1;
        }
        lines.splice(insert_at..insert_at, missing);
    }

    let next = join_lines(lines, trailing_newline);
    Ok((next.clone(), next != config))
}

fn join_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut output = lines.join("\n");
    if trailing_newline && !output.is_empty() {
        output.push('\n');
    }
    output
}

fn is_offload_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "[offload]"
        || trimmed
            .strip_prefix("[offload]")
            .is_some_and(|rest| rest.trim_start().starts_with('#'))
}

async fn read_remote_text(remote: &SshRemote, path: &str) -> Result<String, SpiritError> {
    let argv = vec!["cat".to_owned(), path.to_owned()];
    let output = remote
        .run_with_cap(&argv, READ_TIMEOUT, CONFIG_CAP_BYTES)
        .await?;
    let output = check_cluster_output(output, "reading the Home project config")?;
    String::from_utf8(output.stdout).map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Parse,
            "the Home project config is not valid UTF-8",
        )
    })
}

async fn write_remote_text(
    remote: &SshRemote,
    path: &str,
    bytes: &[u8],
) -> Result<(), SpiritError> {
    let argv = vec!["tee".to_owned(), path.to_owned()];
    let output = remote
        .run_with_stdin(&argv, bytes.to_vec(), SCHEDULE_TIMEOUT, CONFIG_CAP_BYTES)
        .await?;
    check_cluster_output(output, "writing the Home project config")?;
    Ok(())
}

async fn run_checked(
    remote: &SshRemote,
    argv: &[String],
    timeout: Duration,
    label: &str,
) -> Result<CommandOutput, SpiritError> {
    let output = remote.run_with_timeout(argv, timeout).await?;
    check_cluster_output(output, label)
}

async fn run_stdin_checked(
    remote: &SshRemote,
    argv: &[String],
    stdin: Vec<u8>,
    timeout: Duration,
    label: &str,
) -> Result<CommandOutput, SpiritError> {
    let output = remote
        .run_with_stdin(argv, stdin, timeout, CONFIG_CAP_BYTES)
        .await?;
    check_cluster_output(output, label)
}

/// Maps one finished cluster command without ever carrying stdout, stderr or stdin.
///
/// A pairing code travels on stdin, so this guard deliberately does not include remote
/// text in an error. Only the exit status, a fixed label and a typed code are kept.
fn check_cluster_output(output: CommandOutput, label: &str) -> Result<CommandOutput, SpiritError> {
    if output.success() {
        return Ok(output);
    }
    if output.status == 255 {
        let stderr = output.stderr_text();
        if stderr.contains("Host key verification failed") {
            return Err(SpiritError::new(
                SpiritErrorCode::HostKeyMismatch,
                "host key verification failed",
            )
            .with_hint("Compare the fingerprint out of band and explicitly re-pin the host."));
        }
        if stderr.contains("Permission denied") || stderr.contains("Authentication failed") {
            return Err(
                SpiritError::new(SpiritErrorCode::SshAuth, "SSH authentication failed")
                    .with_hint("Check the SSH agent or the identity file path."),
            );
        }
        return Err(
            SpiritError::new(SpiritErrorCode::Remote, format!("{label} failed over SSH"))
                .retryable()
                .with_hint("Check the SSH connection and retry."),
        );
    }
    let code = if output.status == 127 {
        SpiritErrorCode::VowpMissing
    } else {
        code_for_exit(output.status)
    };
    let error = SpiritError::new(code, format!("{label} exited {}", output.status));
    if code == SpiritErrorCode::VowpMissing {
        Err(error.with_hint("Install VOWP on the host, then re-probe."))
    } else if matches!(
        code,
        SpiritErrorCode::InvalidInput
            | SpiritErrorCode::Refused
            | SpiritErrorCode::Conflict
            | SpiritErrorCode::NotFound
    ) {
        Err(error)
    } else {
        Err(error.retryable())
    }
}

/// Normalizes any unrecognized enum value to `unknown` in the health document.
///
/// VOWP enums are normally known snake_case values. A compromised or incompatible Home
/// could put an absolute path or a token in an unknown value, so the health read model
/// never echoes a raw unknown value; it shows `unknown` instead.
fn sanitize_node_status(status: NodeStatus) -> NodeStatus {
    match status {
        NodeStatus::Unknown(_) => NodeStatus::Unknown("unknown".to_owned()),
        other => other,
    }
}

fn sanitize_node_presence(presence: NodePresence) -> NodePresence {
    match presence {
        NodePresence::Unknown(_) => NodePresence::Unknown("unknown".to_owned()),
        other => other,
    }
}

/// Redacts secret-shaped text and absolute paths before a string enters a health row.
fn safe_cluster_text(input: &str) -> String {
    let redacted = crate::error::redact(input);
    scrub_absolute_paths(&redacted)
        .split_whitespace()
        .map(|word| {
            if word.contains("known_hosts")
                || word.contains("id_rsa")
                || word.contains("id_ed25519")
                || word.contains(".vowp-agent")
            {
                "[path]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

fn scrub_absolute_paths(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_path = false;
    let mut path_quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if in_path {
            let terminates = path_quote.map_or_else(|| is_path_terminator(c), |quote| c == quote);
            if terminates {
                in_path = false;
                path_quote = None;
                out.push(c);
            }
            continue;
        }
        let previous = out.chars().next_back();
        let boundary = previous.is_none_or(|p| p.is_whitespace() || is_path_opener(p));
        let starts_path = boundary && (c == '/' || (c == '~' && chars.peek() == Some(&'/')));
        if starts_path {
            out.push_str("[path]");
            in_path = true;
            path_quote = previous.filter(|p| is_quote(*p));
            continue;
        }
        out.push(c);
    }
    out
}

fn is_quote(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '`' | '\u{2018}' | '\u{2019}' | '\u{201c}' | '\u{201d}'
    )
}

fn is_path_opener(c: char) -> bool {
    is_quote(c) || matches!(c, '(' | '[' | '{' | '<' | '=' | ',' | ';' | ':' | '|')
}

fn is_path_terminator(c: char) -> bool {
    c.is_whitespace() || is_quote(c) || matches!(c, ')' | ']' | '}' | '>' | ',' | ';' | '|')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooled_compute_edit_only_touches_the_three_owned_keys() {
        let config = r#"[project]
name = "demo"

[offload]
enabled = false
max_remote_runs = 5
result_transport = "push" # keep the note about the old transport
pause_on_home_brake = true

[offload.projects.demo]
placement = "any"
"#;
        let (updated, changed) = enable_pooled_compute(config).unwrap();
        assert!(changed);
        assert!(updated.contains("enabled = true\n"));
        assert!(updated.contains("default = \"any\"\n"));
        assert!(updated.contains("result_transport = \"bundle\"\n"));
        assert!(updated.contains("max_remote_runs = 5"));
        assert!(updated.contains("pause_on_home_brake = true"));
        assert!(updated.contains("[offload.projects.demo]\nplacement = \"any\""));
        assert!(!updated.contains("default_placement"));
    }

    #[test]
    fn pooled_compute_edit_is_idempotent() {
        let config =
            "[offload]\nenabled = true\ndefault = \"any\"\nresult_transport = \"bundle\"\n";
        let (updated, changed) = enable_pooled_compute(config).unwrap();
        assert!(!changed);
        assert_eq!(updated, config);
    }

    #[test]
    fn pooled_compute_edit_creates_the_section_when_missing() {
        let config = "[project]\nname = \"demo\"\n";
        let (updated, changed) = enable_pooled_compute(config).unwrap();
        assert!(changed);
        assert!(updated.contains(
            "\n[offload]\nenabled = true\ndefault = \"any\"\nresult_transport = \"bundle\"\n"
        ));
    }

    #[test]
    fn pooled_compute_edit_handles_a_header_comment() {
        let config =
            "[offload] # inherited from the setup\nenabled = false\nresult_transport = \"push\"\n";
        let (updated, changed) = enable_pooled_compute(config).unwrap();
        assert!(changed);
        assert!(updated.contains("enabled = true"));
        assert!(updated.contains("default = \"any\""));
        assert!(updated.contains("result_transport = \"bundle\""));
        assert_eq!(
            updated.matches("[offload]").count(),
            1,
            "a commented header must not create a second table"
        );
    }

    #[test]
    fn schedule_spec_formats_whole_units() {
        let policy = BackupPolicy {
            every: Duration::from_secs(24 * 60 * 60),
            keep: 7,
        };
        assert_eq!(policy.schedule_spec().unwrap(), "every 1d");
        let policy = BackupPolicy {
            every: Duration::from_secs(30 * 60),
            keep: 7,
        };
        assert_eq!(policy.schedule_spec().unwrap(), "every 30m");
    }
}
