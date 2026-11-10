//! Takeover of a session from another surface.
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.3.
//!
//! The lease row moves first, so the old surface is cut off immediately and its writes
//! are refused while the old turn drains. The takeover notice is published in-process
//! before the drain, which is what lets `session ingest` push `IngestAck::LeaseRevoked`
//! within the two-second budget. When the session already has an executor, the existing
//! switch protocol drains the old run, interrupts it after five seconds, writes a capsule
//! and seeds the new surface. A session that has never attached an executor gets the note
//! only, because a switch has no valid target.

use vowp_protocol::{
    ExecutorRef, Lease, SessionId, Surface, SwitchReason, SwitchRequest, TakeoverReport, Timestamp, new_lease_token,
};
use vowp_store::Store;
use vowp_store::session_log::SessionLog;

use crate::CoreError;
use crate::deps::Deps;
use crate::session::service::SessionService;

use super::lease::{LeaseRevocation, append_note, clock_time, publish_revocation, surface_kind_name};

/// How long a takeover lets the old surface's turn drain before interrupting it.
pub const TAKEOVER_DRAIN_SECS: u64 = 5;

/// Takes the session over for `target`.
pub fn takeover(
    deps: &Deps,
    store: &Store,
    session: &SessionId,
    target: &Surface,
    now: Timestamp,
) -> Result<TakeoverReport, CoreError> {
    let row = store
        .session(session)?
        .ok_or_else(|| CoreError::NotFound(format!("session {session}")))?;
    store.upsert_surface(target)?;

    let last_seq = SessionLog::open_read(&deps.paths, session)?.last_seq()?;
    let previous = store.lease(session)?;
    let lease = Lease {
        session_id: session.clone(),
        surface_id: Some(target.id.clone()),
        surface_kind: target.kind,
        node: target.node.clone(),
        holder: target.holder.clone(),
        token: new_lease_token(),
        acquired_at: now,
        heartbeat_at: now,
    };
    let replaced = store.take_lease(&lease)?.or(previous);

    let Some(previous) = replaced else {
        // No old surface: this is a first attach, not a takeover.
        append_note(
            deps,
            store,
            session,
            format!(
                "surface {} ({}) attached",
                surface_kind_name(target.kind),
                target.holder
            ),
        )?;
        return Ok(TakeoverReport {
            previous: None,
            drained: false,
            capsule_id: None,
        });
    };

    // The notice must reach the old ingest writer before the drain, not after it.
    publish_revocation(LeaseRevocation {
        session: session.clone(),
        by: target.holder.clone(),
        reason: "session taken over".to_owned(),
        at: now,
        last_seq,
    });

    let active = row.state == vowp_protocol::SessionState::Running
        || store
            .open_runs()?
            .iter()
            .any(|run| run.session_id.as_ref() == Some(session));
    let mut drained = false;
    let mut capsule_id = None;
    if let Some(executor) = row.executor.clone() {
        // A local target needs no node row; a remote target is named explicitly so the
        // switch probes it over SSH.
        let to_node = if row.node == deps.node {
            None
        } else {
            Some(row.node.clone())
        };
        let switched = SessionService::new(deps.clone()).switch(&SwitchRequest {
            session_id: session.clone(),
            to: ExecutorRef {
                executor,
                node: to_node,
            },
            reason: SwitchReason::Takeover,
            drain_secs: TAKEOVER_DRAIN_SECS,
        })?;
        drained = active;
        capsule_id = switched.capsule_id;
    }

    // The note is written after the switch: during an active turn the runner owns the
    // single-writer log lock, and the switch releases it before this point.
    append_note(
        deps,
        store,
        session,
        format!(
            "taken over by {} at {}",
            target.holder,
            clock_time(now, deps.config.node.tz_offset_minutes)
        ),
    )?;

    Ok(TakeoverReport {
        previous: Some(previous.holder),
        drained,
        capsule_id,
    })
}

/// Returns a short human notice for a takeover, used by chat and tests.
pub fn notice(by: &str, at: Timestamp, tz_offset_minutes: i32) -> String {
    format!(
        "Taken over by {} at {}. This surface is now read-only (follow mode).",
        by,
        clock_time(at, tz_offset_minutes)
    )
}
