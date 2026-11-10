//! Lease acquisition, heartbeat, checks and takeover notifications.
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.3.
//!
//! A session has at most one lease row. [`LeaseGuard::acquire`] registers the surface,
//! takes the row, appends the attach note and keeps the heartbeat fresh every five
//! seconds. A lease whose heartbeat is older than sixty seconds is stale and may be
//! replaced without asking the owner; a live lease needs an explicit takeover.
//!
//! A takeover publishes a [`LeaseRevocation`] in-process before the (possibly slow)
//! drain starts, so an ingest writer can push `IngestAck::LeaseRevoked` immediately
//! instead of waiting for its next lease check.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use time::{OffsetDateTime, UtcOffset, macros::format_description};
use vowp_protocol::{
    Actor, EventBody, EventSource, HarnessKind, IngestAck, Lease, Note, SessionId, Surface, SurfaceKind, Timestamp,
};
use vowp_store::session_log::{NewEvent, SessionLog, index_row};
use vowp_store::{Store, StoreError};

use crate::CoreError;
use crate::deps::Deps;

pub use vowp_protocol::surface::new_lease_token;

/// How often a held lease sends a heartbeat.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// A lease without a heartbeat for this long is free.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

/// The request to acquire a session lease.
#[derive(Debug, Clone, PartialEq)]
pub struct AcquireLease {
    /// Session to attach to.
    pub session: SessionId,
    /// The surface asking for the lease.
    pub surface: Surface,
    /// Whether an interactive owner already agreed to take over.
    pub takeover: bool,
}

/// A held lease with its heartbeat thread.
pub struct LeaseGuard {
    deps: Deps,
    session: SessionId,
    token: String,
    thread: Option<JoinHandle<()>>,
}

impl LeaseGuard {
    /// Acquires the session lease, replacing a stale one or a live one the owner agreed
    /// to take over.
    ///
    /// A live lease without `req.takeover` returns [`StoreError::Conflict`] with the
    /// holder, kind, node and acquisition time so the caller can ask the owner.
    pub fn acquire(deps: &Deps, store: &Store, req: &AcquireLease) -> Result<LeaseGuard, CoreError> {
        let now = deps.clock.now();
        store.upsert_surface(&req.surface)?;

        let existing = store.lease(&req.session)?;
        if let Some(previous) = &existing
            && now.since(previous.heartbeat_at) < STALE_AFTER
        {
            if !req.takeover {
                return Err(CoreError::Store(StoreError::Conflict(held_text(
                    &req.session,
                    previous,
                ))));
            }
            super::takeover::takeover(deps, store, &req.session, &req.surface, now)?;
        } else {
            let lease = Lease {
                session_id: req.session.clone(),
                surface_id: Some(req.surface.id.clone()),
                surface_kind: req.surface.kind,
                node: req.surface.node.clone(),
                holder: req.surface.holder.clone(),
                token: new_lease_token(),
                acquired_at: now,
                heartbeat_at: now,
            };
            match store.take_lease(&lease)? {
                Some(previous) => {
                    // Another surface won the race between our look and our write.
                    let stale = now.since(previous.heartbeat_at) >= STALE_AFTER;
                    if stale {
                        append_note(
                            deps,
                            store,
                            &req.session,
                            format!("stale surface {} replaced", previous.holder),
                        )?;
                    } else if req.takeover {
                        // Put the live row back and run the full takeover path so the
                        // old surface gets the switch, capsule and revocation notice.
                        store.take_lease(&previous)?;
                        super::takeover::takeover(deps, store, &req.session, &req.surface, now)?;
                    } else {
                        store.take_lease(&previous)?;
                        return Err(CoreError::Store(StoreError::Conflict(held_text(
                            &req.session,
                            &previous,
                        ))));
                    }
                }
                None => append_note(
                    deps,
                    store,
                    &req.session,
                    format!(
                        "surface {} ({}) attached",
                        surface_kind_name(req.surface.kind),
                        req.surface.holder
                    ),
                )?,
            }
        }

        let lease = store
            .lease(&req.session)?
            .ok_or_else(|| CoreError::NotFound(format!("lease for session {}", req.session)))?;
        let token = lease.token.clone();
        let thread = start_heartbeat(deps, req.session.clone(), token.clone());
        Ok(LeaseGuard {
            deps: deps.clone(),
            session: req.session.clone(),
            token,
            thread: Some(thread),
        })
    }

    /// Returns the lease token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Releases the lease and stops the heartbeat thread.
    pub fn release(self) -> Result<(), CoreError> {
        let store = self.deps.open_store()?;
        store.release_lease(&self.session, &self.token)?;
        Ok(())
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Ok(store) = self.deps.open_store() {
            let _ = store.release_lease(&self.session, &self.token);
        }
        // The heartbeat thread observes the deleted row on its next tick and exits.
        let _ = self.thread.take();
    }
}

/// Checks that the caller holds the live lease of `session`.
///
/// Reads `VOWP_LEASE_TOKEN`, which `chat` and `session ingest` set from the grant. A
/// session without a lease (a job session) passes.
pub fn check_lease(deps: &Deps, store: &Store, session: &SessionId) -> Result<(), CoreError> {
    let token = std::env::var("VOWP_LEASE_TOKEN").ok();
    let lease = store.lease(session)?;
    match (lease, token) {
        (None, _) => Ok(()),
        (Some(lease), Some(token)) if token == lease.token => {
            let _ = deps;
            Ok(())
        }
        (Some(lease), _) => Err(CoreError::Refused(read_only_notice(&lease))),
    }
}

/// Checks an explicit lease token; `None` means "no lease required".
pub fn check_lease_with(store: &Store, session: &SessionId, token: Option<&str>) -> Result<(), CoreError> {
    let Some(lease) = store.lease(session)? else {
        return Ok(());
    };
    match token {
        None => Ok(()),
        Some(token) if token == lease.token => Ok(()),
        Some(_) => Err(CoreError::Refused(read_only_notice(&lease))),
    }
}

/// One takeover notice waiting for the active ingest writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRevocation {
    /// Session whose lease moved.
    pub session: SessionId,
    /// New holder, human-visible.
    pub by: String,
    /// Why the lease moved.
    pub reason: String,
    /// When the takeover committed.
    pub at: Timestamp,
    /// Last sequence the home had appended when the lease moved.
    pub last_seq: u64,
}

/// In-process revocation board shared by takeover and the ingest writer.
struct RevocationBoard {
    entries: Mutex<HashMap<String, LeaseRevocation>>,
    changed: Condvar,
}

/// Returns the process-wide revocation board.
fn revocation_board() -> &'static RevocationBoard {
    static BOARD: OnceLock<RevocationBoard> = OnceLock::new();
    BOARD.get_or_init(|| RevocationBoard {
        entries: Mutex::new(HashMap::new()),
        changed: Condvar::new(),
    })
}

/// Publishes a takeover notice and wakes every waiter.
pub(crate) fn publish_revocation(notice: LeaseRevocation) {
    let board = revocation_board();
    let mut entries = board.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.insert(notice.session.as_str().to_owned(), notice);
    board.changed.notify_all();
}

/// Returns the latest takeover notice for `session`, when one was published.
pub fn revocation(session: &SessionId) -> Option<LeaseRevocation> {
    let board = revocation_board();
    let entries = board.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.get(session.as_str()).cloned()
}

/// Waits up to `timeout` for a takeover notice for `session`.
///
/// The ingest writer calls this instead of polling SQLite for every event. A timeout
/// returns `None`; the caller keeps its normal lease check as the safety net.
pub fn wait_for_revocation(session: &SessionId, timeout: Duration) -> Option<LeaseRevocation> {
    let board = revocation_board();
    let deadline = Instant::now() + timeout;
    let mut entries = board.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        if let Some(notice) = entries.get(session.as_str()) {
            return Some(notice.clone());
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let waited = board
            .changed
            .wait_timeout(entries, deadline.saturating_duration_since(now))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries = waited.0;
    }
}

/// Removes one takeover notice; tests use it to keep the process-wide board clean.
pub fn clear_revocation(session: &SessionId) -> Option<LeaseRevocation> {
    let board = revocation_board();
    let mut entries = board.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.remove(session.as_str())
}

/// Removes leases whose heartbeat is older than [`STALE_AFTER`].
///
/// Returns how many rows were deleted. The sweep is the node tick's safety net for a
/// surface that died without calling [`LeaseGuard::release`]; a live surface heartbeats
/// every five seconds, so it is never touched.
pub fn sweep_leases(_deps: &Deps, store: &Store, now: Timestamp) -> Result<u32, CoreError> {
    let before = now.minus(STALE_AFTER);
    let mut dropped = 0_u32;
    for lease in store.stale_leases(before)? {
        if store.release_lease(&lease.session_id, &lease.token)? {
            dropped = dropped.saturating_add(1);
        }
    }
    Ok(dropped)
}

/// Returns the acknowledgement an active ingest writer should push to the old surface.
///
/// The client already understands `IngestAck::LeaseRevoked`; the ingest loop only needs
/// to wait on [`wait_for_revocation`] and write this value on the same stdout stream.
pub fn revocation_ack(notice: &LeaseRevocation) -> IngestAck {
    IngestAck::LeaseRevoked {
        reason: notice.reason.clone(),
        by: notice.by.clone(),
        at: notice.at,
        last_seq: notice.last_seq,
    }
}

/// Appends one free-form note event and indexes it.
pub(crate) fn append_note(deps: &Deps, store: &Store, session: &SessionId, text: String) -> Result<(), CoreError> {
    let log = SessionLog::open(&deps.paths, session, deps.clock.clone())?;
    let appended = log.append(NewEvent {
        actor: Actor::System,
        source: EventSource {
            harness: HarnessKind::Vowp,
            model: None,
            node: Some(deps.node.clone()),
            native_session_id: None,
        },
        body: EventBody::Note(Note { text }),
        raw: None,
        ts: None,
    })?;
    store.index_event(&index_row(&appended))?;
    log.sync()?;
    Ok(())
}

/// Returns the database/help name of a surface kind.
pub(crate) fn surface_kind_name(kind: SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::Chat => "chat",
        SurfaceKind::Opencode => "opencode",
        SurfaceKind::ClaudeCode => "claude-code",
        SurfaceKind::Mcp => "mcp",
        SurfaceKind::Mirror => "mirror",
        SurfaceKind::Api => "api",
    }
}

/// Formats the `HH:MM` of an instant in the given UTC offset.
pub(crate) fn clock_time(ts: Timestamp, tz_offset_minutes: i32) -> String {
    let Some(utc) = OffsetDateTime::from_unix_timestamp(ts.unix_secs()).ok() else {
        return ts.as_rfc3339();
    };
    let offset = UtcOffset::from_whole_seconds(tz_offset_minutes.saturating_mul(60)).unwrap_or(UtcOffset::UTC);
    utc.to_offset(offset)
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| ts.as_rfc3339())
}

/// Returns the read-only refusal text for a lease held by another surface.
fn read_only_notice(lease: &Lease) -> String {
    format!(
        "this surface is read-only; taken over by {} ({}) at {}",
        lease.holder,
        surface_kind_name(lease.surface_kind),
        clock_time(lease.acquired_at, 0)
    )
}

/// Returns the conflict text that lets an interactive caller ask the owner.
fn held_text(session: &SessionId, lease: &Lease) -> String {
    format!(
        "session {} is held by {} ({} on {}) since {}",
        session,
        lease.holder,
        surface_kind_name(lease.surface_kind),
        lease.node,
        clock_time(lease.acquired_at, 0)
    )
}

/// Starts the heartbeat thread for a held lease.
fn start_heartbeat(deps: &Deps, session: SessionId, token: String) -> JoinHandle<()> {
    let deps = deps.clone();
    thread::spawn(move || {
        let Ok(store) = deps.open_store() else {
            return;
        };
        loop {
            thread::sleep(HEARTBEAT_INTERVAL);
            let now = deps.clock.now();
            match store.heartbeat_lease(&session, &token, now) {
                Ok(true) => {}
                _ => return,
            }
        }
    })
}
