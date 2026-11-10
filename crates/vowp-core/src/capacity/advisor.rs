//! Capacity advice: one measured proposal per turn, never a target change.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §6.4. The advisor runs on the node's
//! thirty-minute timer and on capacity events. It reads the same [`CapacityReport`] as
//! `vowp capacity report --json`, writes one `capacity_advice` row and one node-inbox
//! `decision` item with `apply`/`keep` options, and leaves `capacity_targets` untouched.
//! Applying a proposal is the owner's `vowp capacity set --target N`; the AI never changes
//! the target itself.
//!
//! The corresponding framework skill is `framework/skills/vowp-capacity-advisor/SKILL.md`.

use std::time::Duration;

use vowp_protocol::{
    AdviceState, CapacityAdvice, CapacityReport, InboxId, InboxItem, InboxKind, InboxState, SampleSource, TargetMode,
    Timestamp,
};
use vowp_store::Store;

use crate::CoreError;
use crate::deps::Deps;

use super::admit::CapacityManager;
use super::targets::{MAX_TARGET, MIN_TARGET};

/// The shortest gap between two timer-driven advice turns.
pub const ADVICE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Key holding the timestamp of the last advice row this node wrote.
const LAST_ADVICE_KEY: &str = "capacity:advice_at";

/// Why the advisor is considering a new target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdviceTrigger {
    /// The thirty-minute timer elapsed.
    Timer,
    /// A project was registered.
    NewProject,
    /// A brake tripped.
    Brake,
    /// A run was OOM-killed.
    Oom,
    /// The node stayed idle for long enough.
    Idle,
}

/// Produces one advice row when the trigger calls for it.
///
/// Event triggers are the caller's evidence that the event happened; this function only
/// trusts them. The timer trigger is the one rule the advisor enforces itself: two
/// timer-driven turns are at least [`ADVICE_INTERVAL`] apart. An event turn also moves that
/// gap, so a burst of events cannot be followed by a redundant timer turn a minute later.
///
/// The call writes an advice row, a linked node-inbox `decision` item and a `kv` timestamp.
/// It never writes a `capacity_targets` row and never calls the target setter.
pub fn maybe_advise(
    deps: &Deps,
    store: &Store,
    now: Timestamp,
    trigger: AdviceTrigger,
) -> Result<Option<CapacityAdvice>, CoreError> {
    reconcile_dismissed_advice(store, now)?;
    if !is_due(store, now, trigger)? {
        return Ok(None);
    }

    let report = CapacityManager::new(deps.clone()).report(store, now)?;
    if trigger == AdviceTrigger::Idle && report.running.runs > 0 {
        // The caller reported idle, but a run is open now. A target proposal would be
        // based on the wrong picture; wait for the next turn.
        return Ok(None);
    }

    let advised = report.target.advised_workers.unwrap_or(report.target.target_workers);
    let measured = report
        .host
        .as_ref()
        .is_some_and(|host| host.source != SampleSource::Fallback && host.mem_total_bytes > 0);
    let proposed = if measured {
        advised.clamp(MIN_TARGET, MAX_TARGET)
    } else {
        // Without a measured host sample the conservative auto target is one worker.
        // Proposing that would silently shrink an owner's fixed target on macOS, so the
        // fallback advice is to keep what the owner already chose.
        report.target.target_workers.clamp(MIN_TARGET, MAX_TARGET)
    };

    let reasons = build_reasons(&report, trigger, measured, advised);
    let expected_effect = expected_effect(&report, proposed);
    let inbox_id = InboxId::new();
    let advice = CapacityAdvice {
        id: ulid::Ulid::generate().to_string(),
        created_at: now,
        current_target: report.target.target_workers,
        proposed_target: proposed,
        mode: report.target.mode,
        reasons,
        expected_effect,
        inbox_id: Some(inbox_id.clone()),
        state: AdviceState::Open,
        applied_at: None,
        applied_by: None,
    };
    let inbox = InboxItem {
        id: inbox_id,
        project: None,
        job_id: None,
        kind: InboxKind::Decision,
        title: format!("Capacity: {proposed} workers (advised {proposed})"),
        body: inbox_body(&advice, &report, trigger),
        options: vec!["apply".to_owned(), "keep".to_owned()],
        state: InboxState::Open,
        answer: None,
        answered_by: None,
        created_at: now,
        answered_at: None,
    };

    store.transaction(|store| {
        store.insert_capacity_advice(&advice)?;
        store.insert_node_inbox(&inbox)?;
        store.kv_set(LAST_ADVICE_KEY, &now.as_rfc3339())?;
        Ok(())
    })?;
    Ok(Some(advice))
}

/// Marks the newest open advice dismissed when its inbox item was kept or removed.
///
/// `inbox answer <id> apply` intentionally leaves the advice open: the owner still has to
/// run `vowp capacity set`, which is the only writer of the target. Any other resolution
/// (dismissal, `keep`, or a missing item) closes the proposal.
fn reconcile_dismissed_advice(store: &Store, now: Timestamp) -> Result<(), CoreError> {
    let Some(advice) = store.latest_open_advice()? else {
        return Ok(());
    };
    let Some(inbox_id) = advice.inbox_id.as_ref() else {
        return Ok(());
    };
    let Some(item) = store.inbox_item(inbox_id)? else {
        // §3.6: a missing item only means the owner dismissed it.
        close_advice(store, &advice.id, now)?;
        return Ok(());
    };
    let kept = match item.state {
        InboxState::Open => false,
        InboxState::Dismissed => true,
        InboxState::Answered => !item
            .answer
            .as_deref()
            .is_some_and(|answer| answer.trim().eq_ignore_ascii_case("apply")),
    };
    if kept {
        close_advice(store, &advice.id, now)?;
    }
    Ok(())
}

/// Dismisses one proposal and starts a fresh thirty-minute gap.
///
/// Without the gap reset, an owner who answers `keep` just before a timer tick would get
/// the same proposal again one minute later. Events still bypass the gap by design.
fn close_advice(store: &Store, id: &str, now: Timestamp) -> Result<(), CoreError> {
    store.finish_capacity_advice(id, AdviceState::Dismissed, "owner", now)?;
    store.kv_set(LAST_ADVICE_KEY, &now.as_rfc3339())?;
    Ok(())
}

/// Returns whether this trigger may write an advice row now.
fn is_due(store: &Store, now: Timestamp, trigger: AdviceTrigger) -> Result<bool, CoreError> {
    if trigger != AdviceTrigger::Timer {
        return Ok(true);
    }
    let mut last = store
        .kv_get(LAST_ADVICE_KEY)?
        .and_then(|text| Timestamp::parse(&text).ok());
    if let Some(open) = store.latest_open_advice()? {
        last = Some(last.map_or(open.created_at, |last| last.max(open.created_at)));
    }
    Ok(last.is_none_or(|last| now.since(last) >= ADVICE_INTERVAL))
}

/// Builds the human-readable reasons stored on the advice row.
fn build_reasons(report: &CapacityReport, trigger: AdviceTrigger, measured: bool, advised: u32) -> Vec<String> {
    let mut reasons = vec![trigger_reason(trigger).to_owned()];
    reasons.push(format!(
        "current target: {} worker(s) ({}), measured advice {advised}",
        report.target.target_workers,
        mode_name(report.target.mode)
    ));
    if report.target.is_override {
        reasons.push(format!(
            "owner override: {} worker(s) above the advised {advised}",
            report.target.target_workers
        ));
    }
    match &report.host {
        Some(host) if !measured => reasons.push(format!(
            "host: fallback sample at {}; no measured limit, so the current target is kept",
            host.at.as_rfc3339()
        )),
        Some(host) => reasons.push(format!(
            "host: {} available of {}, reserve {}, {} CPU core(s), {} free disk",
            bytes(host.mem_available_bytes),
            bytes(host.mem_total_bytes),
            bytes(report.reserve_ram_bytes),
            host.cpu_count,
            bytes(host.disk_free_home_bytes)
        )),
        None => reasons.push("host: no sample yet; the conservative target stands".to_owned()),
    }
    reasons.push(format!(
        "running: {} run(s), p90 {} RAM and {:.2} CPU core(s)",
        report.running.runs,
        bytes(report.running.ram_p90_bytes),
        report.running.cpu_p90_cores
    ));
    if report.estimates.is_empty() {
        reasons.push("estimates: none yet; the compiled default is used".to_owned());
    } else {
        let samples: u64 = report
            .estimates
            .iter()
            .map(|estimate| u64::from(estimate.samples))
            .sum();
        let ram = report
            .estimates
            .iter()
            .map(|estimate| estimate.ram_p90_bytes)
            .max()
            .unwrap_or(0);
        let cpu = report
            .estimates
            .iter()
            .map(|estimate| estimate.cpu_p90_cores)
            .fold(0.0_f64, f64::max);
        reasons.push(format!(
            "estimates: {} group(s), {samples} sample(s), largest p90 {} RAM and {cpu:.2} CPU core(s)",
            report.estimates.len(),
            bytes(ram)
        ));
    }
    match report.waiting.first() {
        Some(first) => reasons.push(format!(
            "waiting: {} job(s); first {} in {}: {}",
            report.waiting.len(),
            first.job,
            first.project,
            first.reason.detail
        )),
        None => reasons.push("waiting: no job is waiting for capacity".to_owned()),
    }
    if report.brakes.active {
        let detail = report
            .brakes
            .reasons
            .iter()
            .map(|reason| reason.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        reasons.push(format!("brake: active ({detail}); a target change does not lift it"));
    } else if trigger == AdviceTrigger::Brake {
        reasons.push("brake: off; the reported brake cleared before this turn".to_owned());
    } else {
        reasons.push("brake: off".to_owned());
    }
    if !report.limits.is_empty() {
        let providers = report
            .limits
            .iter()
            .map(|limit| limit.provider.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        reasons.push(format!("provider limits: {providers}"));
    }
    reasons
}

/// Returns one sentence describing what applying the proposal changes.
fn expected_effect(report: &CapacityReport, proposed: u32) -> String {
    let current = report.target.target_workers;
    let mut effect = if proposed > current {
        format!(
            "Admit up to {proposed} concurrent run(s), {} more than now; the node keeps its RAM reserve and CPU headroom.",
            proposed - current
        )
    } else if proposed < current {
        format!(
            "Admit up to {proposed} concurrent run(s), {} fewer than now, leaving more RAM and CPU headroom; running work is not stopped.",
            current - proposed
        )
    } else {
        format!("Keep the target at {proposed} worker(s); the measurements and reserves fit it.")
    };
    effect.push_str(" Every run still passes the brake, provider and per-run checks.");
    if report.target.is_override {
        effect.push_str(" Applying it replaces the current owner override.");
    }
    if report.brakes.active {
        effect.push_str(" The brake is active; a target change does not lift it and running work is never stopped.");
    }
    effect
}

/// Builds the node-inbox body shown to the owner.
fn inbox_body(advice: &CapacityAdvice, report: &CapacityReport, trigger: AdviceTrigger) -> String {
    let mut body = format!(
        "Trigger: {}.\nCurrent target: {} worker(s) ({}).\nProposed target: {} worker(s).\n\nReasons:\n",
        trigger_name(trigger),
        advice.current_target,
        mode_name(advice.mode),
        advice.proposed_target
    );
    for reason in &advice.reasons {
        body.push_str(&format!("- {reason}\n"));
    }
    body.push_str(&format!("\nExpected effect: {}\n", advice.expected_effect));
    if report
        .host
        .as_ref()
        .is_some_and(|host| host.source == SampleSource::Fallback || host.mem_total_bytes == 0)
    {
        body.push_str(
            "\nThis host has no measured sample; the proposal keeps the current target rather than guessing.\n",
        );
    }
    body.push_str(&format!(
        "\nApply only after the owner says yes:\n  vowp capacity set --target {}\nKeep the current target: answer \"keep\" or dismiss this item; nothing changes.\n",
        advice.proposed_target
    ));
    body
}

/// Returns a stable short trigger name for the inbox body.
fn trigger_name(trigger: AdviceTrigger) -> &'static str {
    match trigger {
        AdviceTrigger::Timer => "timer",
        AdviceTrigger::NewProject => "new project",
        AdviceTrigger::Brake => "brake",
        AdviceTrigger::Oom => "OOM",
        AdviceTrigger::Idle => "idle",
    }
}

/// Returns the one-line reason for the trigger.
fn trigger_reason(trigger: AdviceTrigger) -> &'static str {
    match trigger {
        AdviceTrigger::Timer => "scheduled review: the thirty-minute interval elapsed",
        AdviceTrigger::NewProject => "event: a project was registered",
        AdviceTrigger::Brake => "event: a capacity brake was reported",
        AdviceTrigger::Oom => "event: a run was OOM-killed",
        AdviceTrigger::Idle => "event: the node stayed idle",
    }
}

/// Returns the lowercase target-mode name.
fn mode_name(mode: TargetMode) -> &'static str {
    match mode {
        TargetMode::Auto => "auto",
        TargetMode::Fixed => "fixed",
    }
}

/// Formats bytes as GiB with one decimal, or MiB for small values.
fn bytes(value: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if value as f64 >= GIB {
        format!("{:.1} GiB", value as f64 / GIB)
    } else {
        format!("{:.0} MiB", value as f64 / MIB)
    }
}
