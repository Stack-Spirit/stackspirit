//! Read commands: fleet, capacity and gates.
//!
//! Live commands use the same `VowpClient`, host store, SSH runner, fixture client and
//! view models as the desktop host. Fixture mode is explicit and prints a banner.

use spirit_core::connection::LatencyStats;
use spirit_core::engine::gate::{
    AsifGate, GateFeatureFlags, GateProbeOptions, GateSnapshot, LocalAgentState,
    LocalOrchestratorGate, probe_asif, probe_local_orchestrator,
};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::{HostId, HostStore};
use spirit_core::time::Timestamp;
use spirit_core::view::FleetView;
use spirit_core::view::format::{
    format_age_secs, format_bytes_gib, format_percent, truncate_chars,
};
use spirit_core::vowp::VowpClient;
use spirit_core::wire::{CapacityReportDoc, FleetStatus, ProjectName};

use crate::args::{Cli, FleetArgs};
use crate::context::CliEnv;
use crate::error::CliError;
use crate::output::{Output, enum_label, optional, safe_text};

pub(crate) async fn fleet(
    args: &FleetArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let loaded = if cli.fixtures {
        fixture_view(cli, args.project.as_deref(), false).await?
    } else {
        live_view(cli, env, args.project.as_deref(), false).await?
    };
    for warning in loaded.warnings {
        output.warning(warning)?;
    }
    if output.is_json() {
        return output.json(&loaded.view);
    }
    show_fleet(&loaded.view, output)
}

pub(crate) async fn capacity(
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let loaded = if cli.fixtures {
        fixture_view(cli, None, true).await?
    } else {
        live_view(cli, env, None, true).await?
    };
    for warning in loaded.warnings {
        output.warning(warning)?;
    }
    if output.is_json() {
        return output.json(&loaded.view.capacity);
    }
    show_capacity(&loaded.view, output)
}

pub(crate) async fn gates(
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let snapshot = if cli.fixtures {
        fixture_gates(cli)?
    } else {
        live_gates(cli, env).await?
    };
    if output.is_json() {
        return output.json(&snapshot);
    }
    show_gates(&snapshot, output)
}

struct LoadedView {
    view: FleetView,
    warnings: Vec<String>,
}

#[cfg(feature = "fixtures")]
async fn fixture_view(
    cli: &Cli,
    project: Option<&str>,
    require_report: bool,
) -> Result<LoadedView, CliError> {
    let client = crate::fixture::client(cli, "fresh-single")?;
    load_view(
        &client,
        HostId("fixture".to_owned()),
        project,
        require_report,
        true,
    )
    .await
}

#[cfg(not(feature = "fixtures"))]
async fn fixture_view(
    _cli: &Cli,
    _project: Option<&str>,
    _require_report: bool,
) -> Result<LoadedView, CliError> {
    Err(fixture_build_error())
}

#[cfg(feature = "fixtures")]
fn fixture_gates(cli: &Cli) -> Result<GateSnapshot, CliError> {
    let scenario = crate::fixture::scenario(cli, "gate-phase3-waiting")?;
    spirit_core::fixtures::gate_snapshot(scenario).map_err(CliError::from)
}

#[cfg(not(feature = "fixtures"))]
fn fixture_gates(_cli: &Cli) -> Result<GateSnapshot, CliError> {
    Err(fixture_build_error())
}

#[cfg(not(feature = "fixtures"))]
fn fixture_build_error() -> CliError {
    CliError::usage("this build was compiled without fixture support")
        .with_hint("Rebuild with the `fixtures` feature to use --fixtures.")
}

async fn live_view(
    cli: &Cli,
    env: &CliEnv,
    project: Option<&str>,
    require_report: bool,
) -> Result<LoadedView, CliError> {
    let host =
        crate::commands::host::resolve(env.host_store().as_ref(), None, cli.host.as_deref())?;
    crate::commands::host::ensure_pinned(&host)?;
    let client = env.client_for(&host);
    load_view(
        client.as_ref(),
        host.id.clone(),
        project,
        require_report,
        false,
    )
    .await
}

async fn load_view(
    client: &dyn VowpClient,
    host_id: HostId,
    project: Option<&str>,
    require_report: bool,
    use_document_time: bool,
) -> Result<LoadedView, CliError> {
    let probe = client.probe().await?;
    let mut status: FleetStatus = client.fleet_status_initial().await?;
    let mut warnings = Vec::new();
    let report: Option<CapacityReportDoc> = match client.capacity_report().await {
        Ok(report) => Some(report),
        Err(error) if !require_report => {
            warnings.push(format!(
                "capacity detail unavailable: {}",
                safe_text(&error.message, 200)
            ));
            None
        }
        Err(error) => return Err(CliError::from(error)),
    };
    warnings.extend(
        client
            .take_warnings()
            .into_iter()
            .map(|warning| format!("VOWP {}: {}", warning.path, warning.detail)),
    );

    if let Some(project) = project {
        filter_project(&mut status, project)?;
    }

    // A fixture document is a captured moment: evaluate freshness at its own
    // `fetched_at` so `fresh-single` is fresh rather than days old. Live runs use
    // the real clock.
    let now = if use_document_time {
        status.fetched_at
    } else {
        Timestamp::now()
    };
    let mut latency = LatencyStats::default();
    latency.record(probe.latency_ms);
    let view = FleetView::from_status(host_id, &status, report.as_ref(), latency, 1, now);
    Ok(LoadedView { view, warnings })
}

fn filter_project(status: &mut FleetStatus, project: &str) -> Result<(), CliError> {
    let available: Vec<String> = status
        .snapshot
        .projects
        .iter()
        .map(|project| project.name.as_str().to_owned())
        .collect();
    status
        .snapshot
        .projects
        .retain(|candidate| candidate.name.as_str() == project);
    if status.snapshot.projects.is_empty() {
        let hint = if available.is_empty() {
            "The snapshot has no projects.".to_owned()
        } else {
            format!("Available projects: {}.", available.join(", "))
        };
        return Err(CliError::new(
            SpiritError::new(
                SpiritErrorCode::NotFound,
                format!("project {project} is not in the fleet snapshot"),
            )
            .with_hint(hint),
        ));
    }
    status
        .snapshot
        .sessions
        .retain(|session| has_project(session.project.as_ref(), project));
    status
        .snapshot
        .orchestrators
        .retain(|orchestrator| has_project(orchestrator.project.as_ref(), project));
    status
        .snapshot
        .inbox
        .retain(|item| has_project(item.project.as_ref(), project));
    Ok(())
}

fn has_project(project: Option<&ProjectName>, name: &str) -> bool {
    project.is_some_and(|project| project.as_str() == name)
}

async fn live_gates(cli: &Cli, env: &CliEnv) -> Result<GateSnapshot, CliError> {
    let store: std::sync::Arc<dyn HostStore> = env.host_store();
    let host = crate::commands::host::resolve(store.as_ref(), None, cli.host.as_deref())?;
    crate::commands::host::ensure_pinned(&host)?;

    let client = env.client_for(&host);
    let active_project = client.fleet_status().await.ok().and_then(|status| {
        status
            .snapshot
            .projects
            .iter()
            .find(|project| project.state.as_str() == "active")
            .map(|project| project.name.as_str().to_owned())
    });
    let options = GateProbeOptions {
        target: host.ssh.clone(),
        ssh: env.ssh_options(host.identity_file.clone()),
        home: host.vowp_home.clone(),
        active_project,
    };
    let (local_orchestrator, local_agent) = probe_local_orchestrator(
        env.runner().as_ref(),
        Some(env.local_runner().as_ref()),
        &options,
    )
    .await;
    let asif = probe_asif(env.runner().as_ref(), &options).await;

    Ok(GateSnapshot {
        probed_at: Timestamp::now(),
        local_orchestrator,
        asif,
        local_agent,
        // D4 deliberately leaves both action flags false. The CLI never enables a
        // gated action.
        features: GateFeatureFlags::default(),
    })
}

fn show_fleet(view: &FleetView, output: &mut Output<'_>) -> Result<(), CliError> {
    output.text(format!(
        "host: {} ({})",
        view.host_id.as_str(),
        safe_text(&view.home, 64)
    ))?;
    output.text(format!(
        "source: {}  stale: {}  fetched: {}",
        view.source,
        if view.stale { "yes" } else { "no" },
        view.fetched_at
    ))?;
    output.text(format!(
        "supervisor: {}  paused: {}  version: {}  tick: {}s",
        if view.supervisor.running {
            "running"
        } else {
            "stopped"
        },
        view.supervisor.paused,
        safe_text(&view.supervisor.version, 40),
        view.supervisor.tick_secs
    ))?;
    for project in &view.projects {
        output.text(format!(
            "project: {}  state: {}  jobs: {}",
            safe_text(&project.name, 64),
            project.state,
            project
                .jobs_by_state
                .iter()
                .map(|(state, count)| format!("{state}={count}"))
                .collect::<Vec<_>>()
                .join(" ")
        ))?;
    }
    output.text(format!(
        "capacity: {}/{} workers  advised: {}  brake: {}",
        view.capacity.running.runs,
        view.capacity.target.target_workers,
        optional(view.capacity.advised_workers),
        if view.capacity.brake.active {
            view.capacity
                .brake
                .reasons
                .iter()
                .map(|reason| reason.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "none".to_owned()
        }
    ))?;
    output.text(format!(
        "lanes: running {}  ready {}  review {}  blocked {}  done {}",
        lane_count(view, "running"),
        lane_count(view, "ready"),
        lane_count(view, "review_fix"),
        lane_count(view, "parked_blocked"),
        lane_count(view, "done")
    ))?;
    for card in view.running.iter().take(12) {
        output.text(format!(
            "running: {}  {}  {}  {}s",
            safe_text(&card.project, 32),
            safe_text(&card.job_id, 48),
            safe_text(&card.state, 20),
            card.age_secs
        ))?;
    }
    if view.running.len() > 12 {
        output.text(format!("running: … {} more", view.running.len() - 12))?;
    }
    output.text(format!(
        "inbox items: {}  sessions: {}  orchestrators: {}",
        view.inbox.len(),
        view.sessions.len(),
        view.orchestrators.len()
    ))
}

fn lane_count(view: &FleetView, lane: &str) -> u32 {
    view.lanes.counts.get(lane).copied().unwrap_or(0)
}

fn show_capacity(view: &FleetView, output: &mut Output<'_>) -> Result<(), CliError> {
    let capacity = &view.capacity;
    output.text(format!(
        "capacity for {}  generated: {}",
        view.host_id.as_str(),
        capacity.generated_at
    ))?;
    output.text(format!(
        "target: {} {}  advised: {}  override: {}",
        capacity.target.mode,
        capacity.target.target_workers,
        optional(capacity.advised_workers),
        capacity.target.is_override
    ))?;
    output.text(format!(
        "brake: {}",
        if capacity.brake.active {
            capacity
                .brake
                .reasons
                .iter()
                .map(|reason| format!("{} ({})", reason.code, safe_text(&reason.detail, 160)))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "none".to_owned()
        }
    ))?;
    output.text(format!(
        "disk: {} free / {} floor  source: {}  state: {}",
        format_bytes_gib(capacity.disk.free_home_bytes),
        format_bytes_gib(capacity.disk.floor_bytes),
        capacity.disk.source,
        enum_label(capacity.disk.state)
    ))?;
    output.text(format!(
        "ram: {} available / {} reserve  p90 running: {}  state: {}",
        format_bytes_gib(capacity.ram.available_bytes),
        format_bytes_gib(capacity.ram.reserve_bytes),
        format_bytes_gib(capacity.ram.running_p90_bytes),
        enum_label(capacity.ram.state)
    ))?;
    output.text(format!(
        "cpu: load1 {:.2} on {} cores  {:.0}%  state: {}",
        capacity.cpu.load1,
        capacity.cpu.cpu_count,
        capacity.cpu.load_ratio * 100.0,
        enum_label(capacity.cpu.state)
    ))?;
    if let Some(pressure) = capacity.cpu.pressure_avg10 {
        output.text(format!("cpu pressure avg10: {}", format_percent(pressure)))?;
    }
    output.text(format!(
        "running: {} runs  p90 RAM: {}  p90 CPU: {:.2} cores",
        capacity.running.runs,
        format_bytes_gib(capacity.running.ram_p90_bytes),
        capacity.running.cpu_p90_cores
    ))?;
    output.text(format!(
        "waiting: {}  limits: {}  estimates: {}  report loaded: {}  reload pending: {}",
        capacity.waiting.len(),
        capacity.limits.len(),
        capacity.estimates.len(),
        capacity.report_loaded,
        capacity.reload_pending
    ))?;
    for waiting in &capacity.waiting {
        output.text(format!(
            "waiting: {} {}  {}  since {}  {}",
            safe_text(&waiting.project, 32),
            safe_text(&waiting.job, 48),
            waiting.code,
            format_age_secs(age_secs(capacity.generated_at, waiting.since)),
            safe_text(&waiting.detail, 160)
        ))?;
    }
    for limit in &capacity.limits {
        output.text(format!(
            "limit: {}  {}  since {}  {}",
            safe_text(&limit.provider, 48),
            limit.kind,
            format_age_secs(age_secs(capacity.generated_at, limit.since)),
            safe_text(&limit.detail, 160)
        ))?;
    }
    Ok(())
}

fn age_secs(now: Timestamp, at: Timestamp) -> u64 {
    now.since(at).as_secs()
}

fn show_gates(snapshot: &GateSnapshot, output: &mut Output<'_>) -> Result<(), CliError> {
    output.text(format!("probed at: {}", snapshot.probed_at))?;
    output.text(format!(
        "local orchestrator: {}",
        local_orchestrator_summary(&snapshot.local_orchestrator)
    ))?;
    output.text(format!("asif: {}", asif_summary(&snapshot.asif)))?;
    output.text(format!(
        "local agent: {}{}",
        local_agent_label(snapshot.local_agent.state),
        snapshot
            .local_agent
            .last_error
            .as_deref()
            .map_or_else(String::new, |error| format!(" ({})", safe_text(error, 160)))
    ))?;
    output.text(format!(
        "actions: asif={}  local_orchestrator={}",
        snapshot.features.asif_actions, snapshot.features.local_orchestrator_actions
    ))?;
    Ok(())
}

fn local_orchestrator_summary(gate: &LocalOrchestratorGate) -> String {
    match gate {
        LocalOrchestratorGate::Unknown => "unknown".to_owned(),
        LocalOrchestratorGate::Probing => "probing".to_owned(),
        LocalOrchestratorGate::Ready {
            agent_protocol,
            node,
            capabilities,
            ..
        } => format!(
            "ready (protocol {}, node {}, {} capabilities)",
            safe_text(agent_protocol, 64),
            node.as_deref()
                .map_or("unknown".to_owned(), |node| { safe_text(node, 64) }),
            capabilities.len()
        ),
        LocalOrchestratorGate::WaitingForVowpPhase3 {
            missing,
            detected_version,
            ..
        } => format!(
            "waiting for VOWP Phase 3 (version {}, missing: {})",
            detected_version.as_deref().unwrap_or("unknown"),
            truncate_chars(&missing.join(", "), 300)
        ),
        LocalOrchestratorGate::Incompatible {
            detected, required, ..
        } => format!(
            "incompatible (detected {}, required {})",
            safe_text(detected, 120),
            safe_text(required, 120)
        ),
        LocalOrchestratorGate::Unreachable { detail } => {
            format!("unreachable ({})", safe_text(detail, 160))
        }
    }
}

fn asif_summary(gate: &AsifGate) -> String {
    match gate {
        AsifGate::Unknown => "unknown".to_owned(),
        AsifGate::Probing => "probing".to_owned(),
        AsifGate::Ready {
            export,
            import,
            detected_version,
            ..
        } => format!(
            "ready (export {}, import {}, version {})",
            export,
            import,
            detected_version.as_deref().unwrap_or("unknown")
        ),
        AsifGate::WaitingForVowpAsif {
            missing,
            detected_version,
            ..
        } => format!(
            "waiting for VOWP ASIF (version {}, missing: {})",
            detected_version.as_deref().unwrap_or("unknown"),
            truncate_chars(&missing.join(", "), 300)
        ),
        AsifGate::Incompatible {
            detected, required, ..
        } => format!(
            "incompatible (detected {}, required {})",
            safe_text(detected, 120),
            safe_text(required, 120)
        ),
        AsifGate::Unreachable { detail } => {
            format!("unreachable ({})", safe_text(detail, 160))
        }
    }
}

fn local_agent_label(state: LocalAgentState) -> &'static str {
    match state {
        LocalAgentState::Unknown => "unknown",
        LocalAgentState::NotInstalled => "not_installed",
        LocalAgentState::NotEnrolled => "not_enrolled",
        LocalAgentState::Connecting => "connecting",
        LocalAgentState::Connected => "connected",
        LocalAgentState::Reconnecting => "reconnecting",
        LocalAgentState::Revoked => "revoked",
        LocalAgentState::Superseded => "superseded",
        LocalAgentState::Stopped => "stopped",
        LocalAgentState::Incompatible => "incompatible",
    }
}
