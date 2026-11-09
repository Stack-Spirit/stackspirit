//! Startup recovery: finish or abort switch rows left in `started` and close runs that
//! outlived their supervisor. Spec: `docs/design/sessions.md` §5 (failure table) and
//! `docs/design/crates/vowp-core.md` §3.
//!
//! Crash recovery never rewrites the log. For each `started` row it looks at the *last*
//! `session.executor` event: a recorded `switch` event or a final attach of the target
//! completes the row and unfreezes the session on the target; anything else (including a
//! target that was attached and then detached) aborts the row and recomputes the session
//! state. Exactly one of the two outcomes is chosen per row.

use vowp_protocol::{Actor, AttachAction, EventBody, EventSource, HarnessKind, StopReason, SwitchEvent, SwitchState};
use vowp_store::Store;
use vowp_store::session_log::{NewEvent, SessionLog, index_row};

use crate::deps::Deps;
use crate::error::CoreError;
pub use crate::session::service::RecoveryReport;

/// Runs startup recovery against `store`.
pub fn recover(deps: &Deps, store: &Store) -> Result<RecoveryReport, CoreError> {
    let mut report = RecoveryReport::default();
    for switch in store.open_switches()? {
        if recover_switch(deps, store, &switch)? {
            report.switches_completed = report.switches_completed.saturating_add(1);
        } else {
            report.switches_aborted = report.switches_aborted.saturating_add(1);
        }
    }
    for run in store.open_runs()? {
        store.finish_run(
            &run.id,
            &vowp_store::RunFinish {
                outcome: vowp_protocol::RunOutcome::Crash,
                exit_code: None,
                limit_kind: None,
                cost: vowp_protocol::Cost::unknown(),
                native_session_id: None,
                detail: "orphaned by supervisor restart".to_owned(),
            },
        )?;
        report.runs_closed = report.runs_closed.saturating_add(1);
    }
    Ok(report)
}

/// Resolves one `started` switch; returns true when it was completed.
fn recover_switch(deps: &Deps, store: &Store, switch: &vowp_protocol::Switch) -> Result<bool, CoreError> {
    let events = SessionLog::open_read(&deps.paths, &switch.session_id)?.read_all()?;
    let has_switch_event = events.iter().any(|event| {
        matches!(
            event.body(),
            Ok(EventBody::Switch(payload)) if payload.switch_id == switch.id
        )
    });
    let last_executor = events.iter().rev().find_map(|event| match event.body() {
        Ok(EventBody::SessionExecutor(payload)) => Some(payload),
        _ => None,
    });
    let target_attached = last_executor
        .as_ref()
        .is_some_and(|payload| payload.action == AttachAction::Attach && payload.executor == switch.to_executor);

    if has_switch_event || target_attached {
        if !has_switch_event {
            let log = SessionLog::open(&deps.paths, &switch.session_id, deps.clock.clone())?;
            let appended = log.append(NewEvent {
                actor: Actor::System,
                source: EventSource {
                    harness: HarnessKind::Vowp,
                    model: None,
                    node: Some(deps.node.clone()),
                    native_session_id: None,
                },
                body: EventBody::Switch(SwitchEvent {
                    switch_id: switch.id.clone(),
                    from: switch.from_executor.clone().map(|executor| vowp_protocol::ExecutorRef {
                        executor,
                        node: switch.from_node.clone(),
                    }),
                    to: vowp_protocol::ExecutorRef {
                        executor: switch.to_executor.clone(),
                        node: Some(switch.to_node.clone()),
                    },
                    capsule_id: switch.capsule_id.clone(),
                    reason: switch.reason,
                }),
                raw: None,
                ts: None,
            })?;
            store.index_event(&index_row(&appended))?;
            log.sync()?;
        }
        store.finish_switch(&switch.id, SwitchState::Completed, None)?;
        let state = recomputed_state(&events);
        store.set_session_state(&switch.session_id, state, Some(&switch.to_executor))?;
        Ok(true)
    } else {
        store.finish_switch(&switch.id, SwitchState::Aborted, Some("interrupted by restart"))?;
        let state = recomputed_state(&events);
        let executor = match &last_executor {
            Some(payload) => match payload.action {
                AttachAction::Attach => Some(payload.executor.clone()),
                AttachAction::Detach => None,
            },
            None => store.session(&switch.session_id)?.and_then(|session| session.executor),
        };
        store.set_session_state(&switch.session_id, state, executor.as_ref())?;
        Ok(false)
    }
}

/// Returns `blocked` when the last turn ended with a limit, else `idle`.
fn recomputed_state(events: &[vowp_protocol::EventEnvelope]) -> vowp_protocol::SessionState {
    let last_stop = events.iter().rev().find_map(|event| match event.body() {
        Ok(EventBody::TurnEnded(payload)) => Some(payload.stop_reason),
        _ => None,
    });
    if last_stop == Some(StopReason::Limit) {
        vowp_protocol::SessionState::Blocked
    } else {
        vowp_protocol::SessionState::Idle
    }
}
