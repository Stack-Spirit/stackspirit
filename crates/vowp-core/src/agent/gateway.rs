//! The stdio gateway: enroll, authenticate, heartbeat, ping, revocation, surface pairing,
//! local chat, offload dispatch and orchestrator commands.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.2–§4.3, §6.3, §8.2–§8.3, §12.1 and §12.3.
//! [`serve`] reads one JSON [`AgentFrame`] per line from stdin and writes one JSON frame per
//! line to stdout. Standard error is the caller's; this module never logs a code, surface
//! token, signature, public-key value or path map, and the run lease token exists only in the
//! `AssignRun` frame.
//!
//! The gateway runs as a home command with the same store access as `session ingest`. The
//! low-privilege gateway behind the supervisor socket is Phase 4. After the handshake it
//! enqueues ready jobs; each heartbeat sweeps expired run leases, enqueues again and sends at
//! most the node's free slots as `AssignRun`. Surface pairing, chat frames and offload frames
//! are answered on the same authenticated connection. A refused `SurfacePair` keeps the
//! connection open so the agent can retry with a fresh code, while an internal store failure
//! closes it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use vowp_protocol::{
    AGENT_PROTOCOL, AgentCapabilities, AgentFrame, ErrorKind, Executor, ExecutorRef, IngestAck, Lease, LeaseMode,
    Level, NodeName, NodeStatus, RemoteLine, RunId, RunPurpose, SessionId, SurfaceId, SurfaceInfo, SurfaceKind,
    SurfaceState, Timestamp, TurnSummary,
};
use vowp_store::{NodeCredential, Store};

use super::{auth, dispatch, enroll, pair, spool::AgentFrameSpool};
use crate::deps::Deps;
use crate::error::CoreError;
use crate::session::remote::{BeginTurn, IngestGrant, IngestSession};
use crate::surface::lease::{self, AgentLeaseAcquire, AgentLeaseGrant, AgentLeaseHeartbeat, AgentLeaseRelease};
use crate::surface::orchestrator::{CommandSpool, Orchestrator};

/// Largest accepted frame, in bytes (1 MiB).
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Time allowed for the whole handshake, from the first frame to `HomeWelcome`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Heartbeat interval advertised to the agent.
pub const HEARTBEAT_SECS: u64 = 10;

/// Knobs the CLI and tests inject into [`serve_with_options`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOptions {
    /// Identity sent in `HomeChallenge`, `HomeWelcome` and `Enrolled`.
    pub home_id: String,
    /// Heartbeat interval advertised to the agent.
    pub heartbeat_secs: u64,
    /// Largest accepted frame in bytes.
    pub max_frame_bytes: usize,
    /// Whole-handshake deadline.
    pub handshake_timeout: Duration,
}

impl GatewayOptions {
    /// Returns the Phase 0 defaults for one home.
    pub fn for_deps(deps: &Deps) -> Self {
        Self {
            home_id: deps.node.to_string(),
            heartbeat_secs: HEARTBEAT_SECS,
            max_frame_bytes: MAX_FRAME_BYTES,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        }
    }
}

/// How one gateway connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayOutcome {
    /// An `Enroll` frame was redeemed; the agent reconnects for the handshake.
    Enrolled {
        /// Node that was enrolled.
        node: NodeName,
    },
    /// The connection closed after authentication, EOF or a typed error.
    Closed {
        /// Authenticated node, when the handshake had completed.
        node: Option<NodeName>,
    },
}

/// Serves one agent connection with the Phase 0 defaults.
///
/// `reader` must outlive the call and is read on a helper thread so the handshake deadline
/// can be enforced; `writer` carries frames only.
pub fn serve(
    store: &Store,
    deps: &Deps,
    reader: impl BufRead + Send + 'static,
    writer: impl Write,
) -> Result<GatewayOutcome, CoreError> {
    serve_with_options(store, deps, reader, writer, GatewayOptions::for_deps(deps))
}

/// Serves one agent connection with explicit limits.
pub fn serve_with_options(
    store: &Store,
    deps: &Deps,
    reader: impl BufRead + Send + 'static,
    mut writer: impl Write,
    options: GatewayOptions,
) -> Result<GatewayOutcome, CoreError> {
    let incoming = spawn_reader(reader, options.max_frame_bytes);
    let deadline = Instant::now() + options.handshake_timeout;
    let Some(first) = read_frame(&incoming, Some(deadline), &mut writer, options.max_frame_bytes)? else {
        return Ok(GatewayOutcome::Closed { node: None });
    };

    match first {
        AgentFrame::Enroll {
            code,
            public_key,
            agent_version,
            capabilities,
            paths,
            ..
        } => {
            let request = enroll::EnrollRequest {
                code,
                public_key,
                agent_version: agent_version.clone(),
                capabilities,
                paths,
            };
            let now = deps.clock.now();
            match enroll::redeem(store, &request, now) {
                Ok(enrolled) => {
                    send_frame(
                        &mut writer,
                        &AgentFrame::Enrolled {
                            home_id: options.home_id.clone(),
                            node: enrolled.node.clone(),
                            protocol: AGENT_PROTOCOL.to_owned(),
                            server_time: now,
                            heartbeat_secs: options.heartbeat_secs,
                        },
                    )?;
                    Ok(GatewayOutcome::Enrolled { node: enrolled.node })
                }
                Err(error) => {
                    send_error(&mut writer, error.kind(), error.to_string(), false)?;
                    Ok(GatewayOutcome::Closed { node: None })
                }
            }
        }
        AgentFrame::AgentHello {
            node,
            key_id,
            agent_version,
            capabilities,
            ..
        } => {
            let hello = Hello {
                node,
                key_id,
                agent_version,
                capabilities,
            };
            let Some(session) = authenticate(store, deps, hello, &incoming, &mut writer, deadline, &options)? else {
                return Ok(GatewayOutcome::Closed { node: None });
            };
            run_connection(store, deps, session, &incoming, &mut writer, &options)
        }
        _ => {
            send_error(
                &mut writer,
                ErrorKind::InvalidInput,
                "first frame must be Enroll or AgentHello",
                false,
            )?;
            Ok(GatewayOutcome::Closed { node: None })
        }
    }
}

/// An `AgentHello` reduced to the fields the home uses.
struct Hello {
    node: NodeName,
    key_id: String,
    agent_version: String,
    capabilities: AgentCapabilities,
}

/// A connection that passed the challenge-response handshake.
struct Session {
    node: NodeName,
    connection_id: String,
    agent_version: String,
    capabilities: serde_json::Value,
}

/// Issues the challenge, verifies the signature and returns the authenticated session.
fn authenticate(
    store: &Store,
    deps: &Deps,
    hello: Hello,
    incoming: &Receiver<Incoming>,
    writer: &mut dyn Write,
    deadline: Instant,
    options: &GatewayOptions,
) -> Result<Option<Session>, CoreError> {
    let now = deps.clock.now();
    let Some(card) = store.node_card(&hello.node, now)? else {
        send_error(
            writer,
            ErrorKind::NotFound,
            format!("unknown node {}", hello.node),
            false,
        )?;
        return Ok(None);
    };
    if card.status == NodeStatus::Revoked {
        send_frame(
            writer,
            &AgentFrame::Revoked {
                reason: "node is revoked".to_owned(),
            },
        )?;
        return Ok(None);
    }
    if card.status != NodeStatus::Active {
        send_error(
            writer,
            ErrorKind::Auth,
            "node is not active; run `vowp node enroll` on the home".to_owned(),
            false,
        )?;
        return Ok(None);
    }

    let credential = store.node_credential(&hello.node, &hello.key_id)?;
    let Some(credential) = credential.filter(|credential| credential.revoked_at.is_none()) else {
        send_frame(
            writer,
            &AgentFrame::Revoked {
                reason: "node key is not active".to_owned(),
            },
        )?;
        return Ok(None);
    };
    if credential.public_key != hello.key_id || !auth::is_public_key_hex(&credential.public_key) {
        send_error(writer, ErrorKind::Auth, "node key does not match its credential", false)?;
        return Ok(None);
    }

    let nonce = auth::issue_nonce();
    let home_id = options.home_id.clone();
    let connection_id = vowp_protocol::new_lease_token();
    send_frame(
        writer,
        &AgentFrame::HomeChallenge {
            nonce: nonce.clone(),
            home_id: home_id.clone(),
            connection_id: connection_id.clone(),
            server_time: now,
            heartbeat_secs: options.heartbeat_secs,
            max_frame_bytes: options.max_frame_bytes as u64,
        },
    )?;

    let Some(frame) = read_frame(incoming, Some(deadline), writer, options.max_frame_bytes)? else {
        return Ok(None);
    };
    let AgentFrame::AgentAuth { signature } = frame else {
        send_error(writer, ErrorKind::InvalidInput, "expected AgentAuth", false)?;
        return Ok(None);
    };
    if !auth::verify(&credential.public_key, &signature, &nonce, &home_id, &connection_id) {
        send_error(writer, ErrorKind::Auth, "signature rejected", false)?;
        return Ok(None);
    }

    store.upsert_node_credential(&NodeCredential {
        last_used_at: Some(now),
        ..credential
    })?;
    send_frame(
        writer,
        &AgentFrame::HomeWelcome {
            protocol: AGENT_PROTOCOL.to_owned(),
            home_id,
            node: hello.node.clone(),
            connection_id: connection_id.clone(),
        },
    )?;
    register_connection(store, &hello.node, &connection_id);
    Ok(Some(Session {
        node: hello.node,
        connection_id,
        agent_version: hello.agent_version,
        capabilities: enroll::capabilities_value(&hello.capabilities)?,
    }))
}

/// Handles frames after the handshake until the peer closes or is revoked.
fn run_connection(
    store: &Store,
    deps: &Deps,
    session: Session,
    incoming: &Receiver<Incoming>,
    writer: &mut dyn Write,
    options: &GatewayOptions,
) -> Result<GatewayOutcome, CoreError> {
    let node = session.node.clone();
    let connection_id = session.connection_id.clone();
    let result = run_connection_inner(store, deps, session, incoming, writer, options);
    let was_current = connection_is_current(store, &node, &connection_id);
    unregister_connection(store, &node, &connection_id);
    // A superseded connection must not disturb the runs the new connection owns. A current
    // connection that closes leaves its accepted runs resumable for the grace period.
    if was_current {
        let _ = dispatch::disconnect_node(deps, store, &node, "the agent connection closed", deps.clock.now());
        if let Err(error) = Orchestrator::new_queued(deps.clone()).mark_node_disconnected(&node) {
            deps.log.log(
                Level::Warn,
                "orchestrator",
                &format!("cannot mark orchestrators on node {node} degraded: {error}"),
            );
        }
    }
    result
}

/// The body of one authenticated connection.
fn run_connection_inner(
    store: &Store,
    deps: &Deps,
    session: Session,
    incoming: &Receiver<Incoming>,
    writer: &mut dyn Write,
    options: &GatewayOptions,
) -> Result<GatewayOutcome, CoreError> {
    // Offload is enqueued once after the handshake; the first heartbeat sends frames, so
    // the node's reported slots are fresh and the connection has a writer to use.
    dispatch::enqueue_ready_jobs(deps, store, deps.clock.now())?;
    // One live local-chat stream per run on this connection. The ingest state machine
    // owns the session log handle, so a fenced run is dropped here and its later frames
    // are answered with `RunLost` instead of being appended.
    let mut chats: BTreeMap<RunId, ChatStream<'_>> = BTreeMap::new();
    // Orchestrator commands cross the same authenticated connection. The home command
    // writes the durable spool; this connection drains it and publishes the result back.
    let orchestrator = Orchestrator::new_queued(deps.clone());
    let spool = CommandSpool::new(deps.paths.root());
    let frame_spool = AgentFrameSpool::new(deps.paths.root());
    let mut sent_commands: BTreeSet<String> = BTreeSet::new();
    loop {
        if connection_is_current(store, &session.node, &session.connection_id) {
            drain_orchestrator_commands(writer, &session.node, &spool, &mut sent_commands)?;
            drain_agent_frames(writer, &session.node, &frame_spool)?;
        }
        let frame = match poll_frame(incoming, writer, options.max_frame_bytes)? {
            Polled::Frame(frame) => frame,
            Polled::Timeout => {
                if !connection_is_current(store, &session.node, &session.connection_id) {
                    let current = current_connection(store, &session.node).unwrap_or_default();
                    send_frame(writer, &AgentFrame::Superseded { connection_id: current })?;
                    return Ok(GatewayOutcome::Closed {
                        node: Some(session.node),
                    });
                }
                continue;
            }
            Polled::Closed => {
                return Ok(GatewayOutcome::Closed {
                    node: Some(session.node),
                });
            }
        };

        // Presence is derived from `last_seen_at`, so a revoked node is refused on the next
        // frame even when the live connection has no registry entry (Phase 0).
        let now = deps.clock.now();
        let Some(card) = store.node_card(&session.node, now)? else {
            send_error(
                writer,
                ErrorKind::NotFound,
                format!("unknown node {}", session.node),
                false,
            )?;
            return Ok(GatewayOutcome::Closed { node: None });
        };
        if card.status == NodeStatus::Revoked {
            fence_all(&mut chats, "node was revoked", writer)?;
            dispatch::fence_node(deps, store, &session.node, "the node was revoked", now)?;
            send_frame(
                writer,
                &AgentFrame::Revoked {
                    reason: "node was revoked".to_owned(),
                },
            )?;
            return Ok(GatewayOutcome::Closed {
                node: Some(session.node),
            });
        }

        match frame {
            AgentFrame::Heartbeat {
                capacity,
                pending_spool_bytes,
                ..
            } => {
                let mut capabilities = session.capabilities.clone();
                if let Some(object) = capabilities.as_object_mut() {
                    object.insert(
                        "capacity".to_owned(),
                        serde_json::to_value(&capacity).map_err(vowp_protocol::ProtocolError::from)?,
                    );
                    object.insert("pending_spool_bytes".to_owned(), serde_json::json!(pending_spool_bytes));
                }
                store.touch_node(
                    &session.node,
                    now,
                    Some(&session.agent_version),
                    Some(AGENT_PROTOCOL),
                    &capabilities,
                )?;
                // The node is now online at `now`. Queued `Resume`/`Start` commands for a
                // degraded record go to the spool; the drain at the top of the next loop
                // sends them on this connection without blocking the heartbeat.
                if let Err(error) = orchestrator.node_reconnected(&session.node) {
                    deps.log.log(
                        Level::Warn,
                        "orchestrator",
                        &format!("cannot resume orchestrators on node {}: {error}", session.node),
                    );
                }
                // Dispatch on the heartbeat so the slot count is the node's latest
                // report. Expired run leases are fenced before they can be resumed.
                let lost = dispatch::sweep_expired(deps, store, &session.node, now)?;
                for assignment in lost {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id: assignment.run_id,
                            reason: "the run lease expired".to_owned(),
                        },
                    )?;
                }
                dispatch::enqueue_ready_jobs(deps, store, now)?;
                let claimed = dispatch::claim_for_node(deps, store, &session.node, now)?;
                for (index, item) in claimed.iter().enumerate() {
                    if let Err(error) = send_frame(writer, &item.frame) {
                        let _ = dispatch::return_claimed_to_queue(
                            store,
                            &claimed[index..],
                            "the AssignRun frame could not be sent; the assignment stays queued",
                        );
                        return Err(error);
                    }
                }
            }
            AgentFrame::RunAccepted {
                assignment_id,
                run_id,
                epoch,
            } => match dispatch::accept(deps, store, &session.node, &assignment_id, &run_id, epoch, now) {
                Ok(_) => {}
                Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                    send_error(writer, error.kind(), error.to_string(), false)?;
                }
                Err(error) => return Err(error),
            },
            AgentFrame::RunRejected {
                assignment_id,
                run_id,
                epoch,
                reason,
                detail,
            } => match dispatch::reject(
                deps,
                store,
                &session.node,
                &assignment_id,
                &run_id,
                epoch,
                reason,
                &detail,
                now,
            ) {
                Ok(_) => {}
                Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                    send_error(writer, error.kind(), error.to_string(), false)?;
                }
                Err(error) => return Err(error),
            },
            AgentFrame::RunState {
                run_id,
                epoch,
                state,
                spool_bytes,
                head_commit,
                detail,
            } => match dispatch::update_state(
                deps,
                store,
                &session.node,
                &run_id,
                epoch,
                state,
                spool_bytes,
                head_commit.as_deref(),
                &detail,
                now,
            ) {
                Ok(_) => {
                    if let Some(stream) = chats.get_mut(&run_id) {
                        stream.ingest.set_reported_head(head_commit.as_deref());
                    }
                }
                Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                    send_error(writer, error.kind(), error.to_string(), false)?;
                }
                Err(error) => return Err(error),
            },
            AgentFrame::Resume { runs, surfaces } => {
                match dispatch::resume_connection(deps, store, &session.node, &runs, &surfaces, now) {
                    Ok(outcome) => {
                        send_frame(writer, &outcome.resume_ok)?;
                        for (run_id, reason) in outcome.lost {
                            send_frame(writer, &AgentFrame::RunLost { run_id, reason })?;
                        }
                        for claimed in outcome.assignments {
                            send_frame(writer, &claimed.frame)?;
                        }
                    }
                    Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::SurfacePair {
                code,
                surface_id,
                session: session_id,
                app,
            } => match pair::issue_surface_token(store, &session.node, &surface_id, &session_id, &app, &code, now) {
                Ok(issued) => {
                    deps.log.log(
                        Level::Info,
                        &format!("agent {}", session.node),
                        &format!("surface {} paired for session {}", issued.surface_id, session_id),
                    );
                    send_frame(
                        writer,
                        &AgentFrame::SurfaceIssued {
                            surface_id: issued.surface_id,
                            token: issued.token,
                            scopes: issued.scopes,
                            expires_at: issued.expires_at,
                        },
                    )?;
                }
                Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                    let revoked = store
                        .surface_by_id(&surface_id)?
                        .is_some_and(|surface| surface.state == SurfaceState::Revoked);
                    if revoked {
                        send_frame(
                            writer,
                            &AgentFrame::SurfaceRevoked {
                                surface_id: surface_id.clone(),
                                reason: "surface was revoked".to_owned(),
                            },
                        )?;
                    } else {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                }
                Err(error) => return Err(error),
            },
            AgentFrame::LeaseAcquire {
                surface,
                session: session_id,
                surface_token,
                mode,
            } => {
                let result = acquire_lease(store, &session.node, &surface, &session_id, &surface_token, mode, now);
                match result {
                    Ok(LeaseResponse::Granted {
                        grant,
                        previous,
                        holder,
                    }) => {
                        if let Some(previous) = &previous
                            && lease::is_live(now, previous)
                        {
                            send_frame(
                                writer,
                                &AgentFrame::LeaseRevoked {
                                    reason: format!("session {session_id} was taken over by {holder}"),
                                    holder: holder.clone(),
                                },
                            )?;
                        }
                        // A live run under the old lease is fenced now, before the new holder
                        // can write. Its later frames get `RunLost` and are never appended.
                        fence_session(
                            &mut chats,
                            &session_id,
                            &format!("session {session_id} was taken over by {holder}"),
                            writer,
                        )?;
                        deps.log.log(
                            Level::Info,
                            &format!("agent {}", session.node),
                            &format!("surface {surface} acquired the lease on session {session_id}"),
                        );
                        send_frame(
                            writer,
                            &AgentFrame::LeaseGrant {
                                lease_token: grant.lease_token,
                                expires_at: grant.expires_at,
                                tail_seq: grant.tail_seq,
                            },
                        )?;
                    }
                    Ok(LeaseResponse::Conflict(message)) => {
                        send_error(writer, ErrorKind::Conflict, message, false)?;
                    }
                    Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::LeaseHeartbeat {
                surface,
                session: session_id,
                token,
            } => {
                let result = bound_surface(store, &session.node, &surface, &session_id)
                    .and_then(|_| lease::heartbeat_agent_lease(store, &session_id, &surface, &token, now));
                match result {
                    Ok(AgentLeaseHeartbeat::Accepted) => {
                        // A `Credit` grants the next 64-event window. The agent's next
                        // accepted heartbeat is the round-trip that confirms it consumed
                        // the grant, so only then does the home open a new window.
                        for chat in chats.values_mut().filter(|chat| chat.session == session_id) {
                            if chat.credit_pending {
                                chat.in_flight = 0;
                                chat.credit_pending = false;
                            }
                        }
                    }
                    Ok(AgentLeaseHeartbeat::Lost(reason)) => {
                        fence_session(&mut chats, &session_id, &reason, writer)?;
                        send_frame(writer, &AgentFrame::LeaseLost { reason })?;
                    }
                    Ok(AgentLeaseHeartbeat::Revoked { reason, holder }) => {
                        fence_session(&mut chats, &session_id, &reason, writer)?;
                        send_frame(writer, &AgentFrame::LeaseRevoked { reason, holder })?;
                    }
                    Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::LeaseRelease {
                surface,
                session: session_id,
                token,
            } => {
                let result = bound_surface(store, &session.node, &surface, &session_id)
                    .and_then(|_| lease::release_agent_lease(store, &session_id, &surface, &token));
                match result {
                    Ok(AgentLeaseRelease::Released) => {
                        fence_session(&mut chats, &session_id, "lease was released", writer)?;
                    }
                    Ok(AgentLeaseRelease::Lost(reason)) => {
                        fence_session(&mut chats, &session_id, &reason, writer)?;
                        send_frame(writer, &AgentFrame::LeaseLost { reason })?;
                    }
                    Ok(AgentLeaseRelease::Revoked { reason, holder }) => {
                        fence_session(&mut chats, &session_id, &reason, writer)?;
                        send_frame(writer, &AgentFrame::LeaseRevoked { reason, holder })?;
                    }
                    Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::TurnBegin {
                surface,
                session: session_id,
                lease_token,
                prompt,
                executor,
                cwd_ref,
                job,
                purpose,
            } => {
                if chats.values().any(|chat| chat.session == session_id) {
                    send_error(
                        writer,
                        ErrorKind::Conflict,
                        format!("session {session_id} already has a local chat run"),
                        false,
                    )?;
                    continue;
                }
                match begin_agent_turn(
                    store,
                    deps,
                    &session.node,
                    &surface,
                    &session_id,
                    &lease_token,
                    prompt,
                    &executor,
                    &cwd_ref,
                    job,
                    purpose,
                    now,
                ) {
                    Ok(started) => {
                        let grant = AgentFrame::TurnGrant {
                            run_id: started.run_id.clone(),
                            session: started.session.clone(),
                            lease_token: started.lease.clone(),
                            expires_at: started.expires_at,
                            tail_seq: started.tail_seq,
                        };
                        chats.insert(started.run_id.clone(), started.stream);
                        send_frame(writer, &grant)?;
                    }
                    Err(
                        error @ (CoreError::Refused(_)
                        | CoreError::Invalid(_)
                        | CoreError::NotFound(_)
                        | CoreError::Store(vowp_store::StoreError::Conflict(_))),
                    ) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::RemoteLine { run_id, epoch, line } => {
                if !chats.contains_key(&run_id) {
                    // A reconnect opens a new gateway process, so the live map starts
                    // empty. A `Hello` for a run this home still owns (open run row,
                    // paired surface and the live lease) resumes its ingest stream;
                    // events without that proof are fenced as before.
                    if let RemoteLine::Hello {
                        session_id,
                        run_id: hello_run,
                        lease,
                        ..
                    } = &line
                    {
                        match resume_run_stream(store, deps, &session.node, &run_id, hello_run, session_id, lease, now)
                        {
                            Ok(stream) => {
                                chats.insert(run_id.clone(), stream);
                            }
                            Err(error) => {
                                send_frame(
                                    writer,
                                    &AgentFrame::RunLost {
                                        run_id: run_id.clone(),
                                        reason: error.to_string(),
                                    },
                                )?;
                                continue;
                            }
                        }
                    }
                }
                let Some(stream) = chats.get_mut(&run_id) else {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id,
                            reason: "run is not active on this connection".to_owned(),
                        },
                    )?;
                    continue;
                };
                if stream.epoch == 0 {
                    stream.epoch = epoch;
                } else if stream.epoch != epoch {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id: run_id.clone(),
                            reason: format!("stale run epoch {epoch}; expected {}", stream.epoch),
                        },
                    )?;
                    chats.remove(&run_id);
                    continue;
                }
                let is_event = matches!(line, RemoteLine::Event { .. });
                if is_event {
                    if stream.credit_pending && stream.in_flight >= CREDIT_WINDOW {
                        send_frame(
                            writer,
                            &AgentFrame::Error {
                                kind: ErrorKind::Conflict,
                                message: format!(
                                    "the {CREDIT_WINDOW}-event in-flight window is exhausted; wait for Credit"
                                ),
                                retryable: true,
                            },
                        )?;
                        continue;
                    }
                    // Send the replenishment before the acknowledgement of the window's
                    // last event, so a client that reads one frame per send sees the
                    // Credit first and can confirm it before the next event.
                    if stream.in_flight + 1 == CREDIT_WINDOW {
                        send_frame(
                            writer,
                            &AgentFrame::Credit {
                                run_id: run_id.clone(),
                                count: CREDIT_WINDOW,
                            },
                        )?;
                        stream.credit_pending = true;
                    }
                }
                let stop = ingest_remote_line(writer, &run_id, stream, &line)?;
                if stop {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id: run_id.clone(),
                            reason: "ingest stopped; the surface is read-only".to_owned(),
                        },
                    )?;
                    chats.remove(&run_id);
                    continue;
                }
                if is_event {
                    stream.in_flight = stream.in_flight.saturating_add(1);
                }
            }
            AgentFrame::TurnEnd { run_id, summary } => {
                let Some(mut stream) = chats.remove(&run_id) else {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id,
                            reason: "run is not active on this connection".to_owned(),
                        },
                    )?;
                    continue;
                };
                let line = summary_line(summary);
                let stop = ingest_remote_line(writer, &run_id, &mut stream, &line)?;
                if stop {
                    send_frame(
                        writer,
                        &AgentFrame::RunLost {
                            run_id,
                            reason: "ingest stopped; the surface is read-only".to_owned(),
                        },
                    )?;
                }
            }
            AgentFrame::OrchestratorResult(result) => {
                match orchestrator.apply_result(&session.node, &result) {
                    Ok(Some(_)) => {
                        // Publish the result for the waiting home command and retire the
                        // queued command. A late result was already applied above.
                        spool.complete(&session.node, &result)?;
                        sent_commands.remove(&result.request_id);
                    }
                    Ok(None) => {
                        // No record has this request id as its latest command. Do not
                        // publish an unsolicited result file.
                    }
                    Err(error @ (CoreError::Refused(_) | CoreError::Invalid(_) | CoreError::NotFound(_))) => {
                        send_error(writer, error.kind(), error.to_string(), false)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AgentFrame::Ping => send_frame(writer, &AgentFrame::Pong)?,
            AgentFrame::Pong => {}
            _ => {
                send_error(writer, ErrorKind::InvalidInput, "unexpected frame", false)?;
                return Ok(GatewayOutcome::Closed {
                    node: Some(session.node),
                });
            }
        }
    }
}

/// Sends queued orchestrator commands that this connection has not carried yet.
///
/// The spool is the cross-process queue between `vowp orchestrator …` and the gateway.
/// A command stays on disk until its result is published, so a dropped connection
/// re-sends it once on the next authenticated connection.
fn drain_orchestrator_commands(
    writer: &mut dyn Write,
    node: &NodeName,
    spool: &CommandSpool,
    sent: &mut BTreeSet<String>,
) -> Result<(), CoreError> {
    for command in spool.pending(node)? {
        let request_id = command.request_id().to_owned();
        if !sent.insert(request_id.clone()) {
            continue;
        }
        if let Err(error) = send_frame(writer, &AgentFrame::OrchestratorCommand(command)) {
            sent.remove(&request_id);
            return Err(error);
        }
    }
    Ok(())
}

/// Sends queued system-initiated frames for this node and retires each after the write.
///
/// The spool is the cross-process queue between a home command (`vowp surface rotate`) and
/// the node's live gateway. A frame stays on disk until the write succeeds, so a dropped
/// connection delivers it on the next authenticated connection. Only the directory of this
/// connection's node is read, so a frame for another node is never sent.
fn drain_agent_frames(writer: &mut dyn Write, node: &NodeName, spool: &AgentFrameSpool) -> Result<(), CoreError> {
    for queued in spool.pending(node)? {
        send_frame(writer, &queued.frame)?;
        spool.remove(node, &queued.request_id)?;
    }
    Ok(())
}

/// The in-flight event window replenished by a `Credit` frame.
const CREDIT_WINDOW: u64 = 64;

/// One live local-chat stream on this connection.
struct ChatStream<'a> {
    /// Session the run belongs to.
    session: SessionId,
    /// Run epoch; zero until the first line arrives.
    epoch: u64,
    /// Home-side ingest state machine.
    ingest: IngestSession<'a>,
    /// Events accepted in the current 64-event window.
    in_flight: u64,
    /// Whether the next window was granted and awaits the agent's round-trip.
    credit_pending: bool,
}

/// A turn begun by a paired surface, ready to ingest.
struct StartedTurn<'a> {
    /// Run id assigned by the home.
    run_id: RunId,
    /// Session the run continues.
    session: SessionId,
    /// Lease token the run is fenced by.
    lease: String,
    /// Lease expiry.
    expires_at: Timestamp,
    /// Last sequence at turn begin.
    tail_seq: u64,
    /// Ingest state for the run.
    stream: ChatStream<'a>,
}

/// Resumes the ingest stream of an open run after a reconnect.
///
/// A paired-surface lease continues a chat turn; an agent-kind run lease continues an
/// offload assignment. The `Hello` must prove the run is still the live one in both cases.
#[allow(clippy::too_many_arguments)]
fn resume_run_stream<'a>(
    store: &'a Store,
    deps: &'a Deps,
    node: &NodeName,
    run_id: &RunId,
    hello_run: &RunId,
    session: &SessionId,
    lease: &str,
    now: Timestamp,
) -> Result<ChatStream<'a>, CoreError> {
    let Some(current) = store.lease(session)? else {
        return Err(CoreError::Refused(format!("session {session} has no live lease")));
    };
    if !lease::is_live(now, &current) {
        return Err(CoreError::Refused(format!(
            "session {session} lease has expired; the surface is read-only"
        )));
    }
    if current.surface_kind == SurfaceKind::Agent && current.surface_id.is_none() {
        return resume_offload_stream(store, deps, node, run_id, hello_run, session, lease);
    }
    resume_chat_stream(store, deps, node, run_id, hello_run, session, lease, now)
}

/// Opens the ingest stream of an offloaded run from its agent-kind run lease.
///
/// An offload run has no paired surface. The durable assignment is the proof that this
/// node owns the run; the lease token is still verified against its hash before any event
/// is appended.
fn resume_offload_stream<'a>(
    store: &'a Store,
    deps: &'a Deps,
    node: &NodeName,
    run_id: &RunId,
    hello_run: &RunId,
    session: &SessionId,
    lease: &str,
) -> Result<ChatStream<'a>, CoreError> {
    if run_id != hello_run {
        return Err(CoreError::Invalid(format!(
            "hello names run {hello_run}, expected {run_id}"
        )));
    }
    let run = store.require_run(run_id)?;
    if run.session_id.as_ref() != Some(session) {
        return Err(CoreError::Refused(format!(
            "run {run_id} does not belong to session {session}"
        )));
    }
    if run.node != *node {
        return Err(CoreError::Refused(format!(
            "run {run_id} belongs to node {}, not {node}",
            run.node
        )));
    }
    if run.ended_at.is_some() {
        return Err(CoreError::Refused(format!("run {run_id} has already ended")));
    }
    let session_row = store.require_session(session)?;
    if session_row.node != *node {
        return Err(CoreError::Refused(format!(
            "session {session} belongs to node {}, not {node}",
            session_row.node
        )));
    }
    let assigned = store.assignments_for_run(run_id)?.into_iter().any(|assignment| {
        assignment.node == *node && assignment.session_id == *session && assignment.state.is_active()
    });
    if !assigned {
        return Err(CoreError::Refused(format!(
            "run {run_id} has no live offload assignment on node {node}"
        )));
    }
    crate::surface::check_lease_with(store, session, Some(lease))?;
    let ingest = IngestSession::begin(
        deps,
        store,
        &IngestGrant {
            session: session.clone(),
            lease: lease.to_owned(),
            run: run_id.clone(),
        },
    )?;
    Ok(ChatStream {
        session: session.clone(),
        epoch: 0,
        ingest,
        in_flight: 0,
        credit_pending: false,
    })
}

/// Resumes the ingest stream of an open run after a reconnect.
///
/// The new connection has no in-memory stream, so the `Hello` must prove the run is
/// still the live one: it belongs to this node and the named session, it has not ended,
/// the named session is on this node, and the presented token is the live lease of a
/// paired, attached surface. The caller then feeds the `Hello` through `IngestSession`.
#[allow(clippy::too_many_arguments)]
fn resume_chat_stream<'a>(
    store: &'a Store,
    deps: &'a Deps,
    node: &NodeName,
    run_id: &RunId,
    hello_run: &RunId,
    session: &SessionId,
    lease: &str,
    now: Timestamp,
) -> Result<ChatStream<'a>, CoreError> {
    if run_id != hello_run {
        return Err(CoreError::Invalid(format!(
            "hello names run {hello_run}, expected {run_id}"
        )));
    }
    let run = store.require_run(run_id)?;
    if run.session_id.as_ref() != Some(session) {
        return Err(CoreError::Refused(format!(
            "run {run_id} does not belong to session {session}"
        )));
    }
    if run.node != *node {
        return Err(CoreError::Refused(format!(
            "run {run_id} belongs to node {}, not {node}",
            run.node
        )));
    }
    if run.ended_at.is_some() {
        return Err(CoreError::Refused(format!("run {run_id} has already ended")));
    }
    let session_row = store.require_session(session)?;
    if session_row.node != *node {
        return Err(CoreError::Refused(format!(
            "session {session} belongs to node {}, not {node}",
            session_row.node
        )));
    }
    let Some(current) = store.lease(session)? else {
        return Err(CoreError::Refused(format!("session {session} has no live lease")));
    };
    if !lease::is_live(now, &current) {
        return Err(CoreError::Refused(format!(
            "session {session} lease has expired; the surface is read-only"
        )));
    }
    let surface = current
        .surface_id
        .clone()
        .ok_or_else(|| CoreError::Refused(format!("session {session} lease has no surface")))?;
    bound_surface(store, node, &surface, session)?;
    crate::surface::check_lease_with(store, session, Some(lease))?;
    let ingest = IngestSession::begin(
        deps,
        store,
        &IngestGrant {
            session: session.clone(),
            lease: lease.to_owned(),
            run: run_id.clone(),
        },
    )?;
    Ok(ChatStream {
        session: session.clone(),
        epoch: 0,
        ingest,
        in_flight: 0,
        credit_pending: false,
    })
}

/// Validates a `TurnBegin`, writes the framing events and opens its ingest stream.
#[allow(clippy::too_many_arguments)]
fn begin_agent_turn<'a>(
    store: &'a Store,
    deps: &'a Deps,
    node: &NodeName,
    surface: &SurfaceId,
    session: &SessionId,
    lease: &str,
    prompt: String,
    executor: &str,
    cwd_ref: &vowp_protocol::PathRef,
    job: Option<vowp_protocol::JobId>,
    purpose: RunPurpose,
    now: Timestamp,
) -> Result<StartedTurn<'a>, CoreError> {
    bound_surface(store, node, surface, session)?;
    // A paired surface may begin a turn only while it holds the live lease. The
    // generic ingest path treats a missing lease row as a job session; the agent path
    // must not, because there is no paired surface then.
    let Some(current) = store.lease(session)? else {
        return Err(CoreError::Refused(format!("session {session} has no live lease")));
    };
    if !lease::is_live(now, &current) {
        return Err(CoreError::Refused(format!(
            "session {session} lease has expired; the surface is read-only"
        )));
    }
    crate::surface::check_lease_with(store, session, Some(lease))?;
    let row = store.require_session(session)?;
    if let Some(project) = cwd_ref_project(cwd_ref)
        && row.project.as_ref() != Some(project)
    {
        return Err(CoreError::Refused(format!(
            "cwd ref is for project {project}, but session {session} belongs to another project"
        )));
    }
    if let Some(job) = &job
        && row.job_id.as_ref() != Some(job)
    {
        return Err(CoreError::Refused(format!(
            "turn names job {job}, but session {session} belongs to another job"
        )));
    }
    let executor = Executor::parse(executor)?;
    let grant = crate::session::remote::begin_turn(
        deps,
        store,
        &BeginTurn {
            session: session.clone(),
            lease: lease.to_owned(),
            executor: ExecutorRef {
                executor,
                node: Some(row.node.clone()),
            },
            purpose,
            node: row.node.clone(),
            // The agent resolves its own working directory locally; the home must not
            // record a Mac absolute path in a run row or an event.
            cwd: None,
            prompt,
        },
    )?;
    let stream = ChatStream {
        session: session.clone(),
        epoch: 0,
        ingest: IngestSession::begin(
            deps,
            store,
            &IngestGrant {
                session: session.clone(),
                lease: grant.lease_token.clone(),
                run: grant.run_id.clone(),
            },
        )?,
        in_flight: 0,
        credit_pending: false,
    };
    Ok(StartedTurn {
        run_id: grant.run_id,
        session: session.clone(),
        lease: grant.lease_token,
        expires_at: now.plus(lease::STALE_AFTER),
        tail_seq: grant.tail_seq,
        stream,
    })
}

/// Returns the project a path ref belongs to.
fn cwd_ref_project(cwd_ref: &vowp_protocol::PathRef) -> Option<&vowp_protocol::ProjectName> {
    match cwd_ref {
        vowp_protocol::PathRef::Project { project }
        | vowp_protocol::PathRef::Worktree { project, .. }
        | vowp_protocol::PathRef::Subdir { project, .. } => Some(project),
    }
}

/// Feeds one v0.2 line through the ingest state machine and forwards every acknowledgement.
///
/// Returns `true` when the stream must stop: a takeover, a rejection or a conflict. The run
/// is then fenced by the caller and no later line is appended.
fn ingest_remote_line(
    writer: &mut dyn Write,
    run_id: &RunId,
    stream: &mut ChatStream<'_>,
    line: &RemoteLine,
) -> Result<bool, CoreError> {
    let mut text = serde_json::to_string(line).map_err(vowp_protocol::ProtocolError::from)?;
    text.push('\n');
    let mut ack_bytes = Vec::new();
    let result = stream.ingest.line(&text, &mut ack_bytes);
    let acks = parse_ingest_acks(&ack_bytes)?;
    let mut stop = result.is_err();
    let mut ack_sent = false;
    for ack in acks {
        let terminal = matches!(ack, IngestAck::LeaseRevoked { .. } | IngestAck::Error { .. });
        send_frame(writer, &agent_ack_frame(run_id, ack))?;
        ack_sent = true;
        stop |= terminal;
    }
    if let Err(error) = result
        && !ack_sent
    {
        send_error(writer, error.kind(), error.to_string(), false)?;
    }
    Ok(stop)
}

/// Parses the acknowledgement lines the ingest state machine wrote.
fn parse_ingest_acks(bytes: &[u8]) -> Result<Vec<IngestAck>, CoreError> {
    let text = String::from_utf8_lossy(bytes);
    let mut acks = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        acks.push(
            serde_json::from_str(line)
                .map_err(|error| CoreError::Command(format!("invalid ingest acknowledgement: {error}")))?,
        );
    }
    Ok(acks)
}

/// Maps one v0.2 acknowledgement onto the agent envelope.
fn agent_ack_frame(run_id: &RunId, ack: IngestAck) -> AgentFrame {
    match ack {
        IngestAck::Ack { event_id, seq } => AgentFrame::IngestAck {
            run_id: run_id.clone(),
            event_id: Some(event_id),
            seq: Some(seq),
            duplicate: false,
            lease_revoked: None,
            error: None,
        },
        IngestAck::Duplicate { event_id, seq } => AgentFrame::IngestAck {
            run_id: run_id.clone(),
            event_id: Some(event_id),
            seq: Some(seq),
            duplicate: true,
            lease_revoked: None,
            error: None,
        },
        IngestAck::Summary { .. } => AgentFrame::IngestAck {
            run_id: run_id.clone(),
            event_id: None,
            seq: None,
            duplicate: false,
            lease_revoked: None,
            error: None,
        },
        IngestAck::LeaseRevoked { reason, last_seq, .. } => AgentFrame::IngestAck {
            run_id: run_id.clone(),
            event_id: None,
            seq: Some(last_seq),
            duplicate: false,
            lease_revoked: Some(reason),
            error: None,
        },
        IngestAck::Error { message, .. } => AgentFrame::IngestAck {
            run_id: run_id.clone(),
            event_id: None,
            seq: None,
            duplicate: false,
            lease_revoked: None,
            error: Some(message),
        },
    }
}

/// Converts the terminal agent summary into the v0.2 summary line.
fn summary_line(summary: TurnSummary) -> RemoteLine {
    RemoteLine::Summary {
        exit_code: summary.exit_code,
        outcome: summary.outcome,
        native_session_id: summary.native_session_id,
        cost: summary.cost,
        limit: summary.limit,
        result_text: summary.result_text,
        stop_reason: summary.stop_reason,
        usage: summary.usage,
    }
}

/// Fences every live run of one session and tells the peer why.
fn fence_session(
    chats: &mut BTreeMap<RunId, ChatStream<'_>>,
    session: &SessionId,
    reason: &str,
    writer: &mut dyn Write,
) -> Result<(), CoreError> {
    let runs: Vec<RunId> = chats
        .iter()
        .filter(|(_, chat)| chat.session == *session)
        .map(|(run, _)| run.clone())
        .collect();
    for run in runs {
        chats.remove(&run);
        send_frame(
            writer,
            &AgentFrame::RunLost {
                run_id: run,
                reason: reason.to_owned(),
            },
        )?;
    }
    Ok(())
}

/// Fences every live run on the connection and tells the peer why.
fn fence_all(
    chats: &mut BTreeMap<RunId, ChatStream<'_>>,
    reason: &str,
    writer: &mut dyn Write,
) -> Result<(), CoreError> {
    let runs: Vec<RunId> = chats.keys().cloned().collect();
    for run in runs {
        chats.remove(&run);
        send_frame(
            writer,
            &AgentFrame::RunLost {
                run_id: run,
                reason: reason.to_owned(),
            },
        )?;
    }
    Ok(())
}

/// The answer to one lease acquire frame.
enum LeaseResponse {
    /// The surface now holds the lease.
    Granted {
        /// The one-time grant.
        grant: AgentLeaseGrant,
        /// The row this acquisition replaced.
        previous: Option<Box<Lease>>,
        /// Human-visible holder name of the new surface.
        holder: String,
    },
    /// A live lease is held by another surface and no takeover was requested.
    Conflict(String),
}

/// Verifies the surface token and acquires or takes over the session lease.
///
/// The token is checked against the v4 `surface_tokens` row and must belong to a surface
/// on the connection's node whose metadata names this session and whose scope grants
/// `session.write:<id>`. No token or code value is logged or echoed.
fn acquire_lease(
    store: &Store,
    node: &NodeName,
    surface_id: &SurfaceId,
    session: &SessionId,
    surface_token: &str,
    mode: LeaseMode,
    now: vowp_protocol::Timestamp,
) -> Result<LeaseResponse, CoreError> {
    let verified = pair::verify_surface_token(store, surface_token, now)?;
    if verified.surface.id != *surface_id {
        return Err(CoreError::Refused("surface token is for another surface".to_owned()));
    }
    let surface = bound_surface(store, node, surface_id, session)?;
    let write_scope = format!("session.write:{}", session.as_str());
    if !verified.token.scopes.iter().any(|scope| scope.as_str() == write_scope) {
        return Err(CoreError::Refused(format!(
            "surface token does not grant {write_scope}"
        )));
    }
    match lease::acquire_agent_lease(store, session, &surface, mode, now)? {
        AgentLeaseAcquire::Granted { grant, previous } => Ok(LeaseResponse::Granted {
            grant,
            previous: previous.map(Box::new),
            holder: surface.holder,
        }),
        AgentLeaseAcquire::Conflict(conflict) => Ok(LeaseResponse::Conflict(format!(
            "session {session} is held by {} ({}) since {}",
            conflict.holder,
            lease::surface_kind_name(conflict.surface_kind),
            conflict.since.as_rfc3339()
        ))),
    }
}

/// Resolves a surface that is attached to `node` and paired for `session`.
///
/// This is the per-frame binding for heartbeats and releases, which carry no surface
/// token: the lease token authenticates the holder, while the surface row and its metadata
/// bind the frame to the connection node and the paired session.
fn bound_surface(
    store: &Store,
    node: &NodeName,
    surface_id: &SurfaceId,
    session: &SessionId,
) -> Result<SurfaceInfo, CoreError> {
    let Some(surface) = store.surface_by_id(surface_id)? else {
        return Err(CoreError::NotFound(format!("surface {surface_id}")));
    };
    if surface.node != *node {
        return Err(CoreError::Refused("surface belongs to another node".to_owned()));
    }
    if surface.state != SurfaceState::Attached {
        return Err(CoreError::Refused("surface is not attached".to_owned()));
    }
    if surface.metadata.get("session").and_then(serde_json::Value::as_str) != Some(session.as_str()) {
        return Err(CoreError::Refused("surface is paired for another session".to_owned()));
    }
    let session_row = store.require_session(session)?;
    if session_row.node != *node {
        return Err(CoreError::Refused("session belongs to another node".to_owned()));
    }
    Ok(surface)
}

/// One item read from the input thread.
enum Incoming {
    Line(String),
    TooLarge,
    NotUtf8,
    Eof,
    Io(std::io::Error),
}

/// Reads lines on a helper thread so the handshake can time out on a blocking reader.
fn spawn_reader(reader: impl BufRead + Send + 'static, max_frame_bytes: usize) -> Receiver<Incoming> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = reader;
        loop {
            let item = match read_line_limited(&mut reader, max_frame_bytes) {
                Ok(Some(line)) => Incoming::Line(line),
                Ok(None) => Incoming::Eof,
                Err(LineError::TooLarge) => Incoming::TooLarge,
                Err(LineError::NotUtf8) => Incoming::NotUtf8,
                Err(LineError::Io(error)) => Incoming::Io(error),
            };
            let done = matches!(
                item,
                Incoming::Eof | Incoming::TooLarge | Incoming::NotUtf8 | Incoming::Io(_)
            );
            if sender.send(item).is_err() || done {
                break;
            }
        }
    });
    receiver
}

/// Why one bounded line read failed.
enum LineError {
    TooLarge,
    NotUtf8,
    Io(std::io::Error),
}

/// Reads one newline-terminated UTF-8 line of at most `max` bytes.
///
/// The bound is enforced while reading, so a peer that never sends a newline cannot grow
/// memory without limit. A trailing `\r` is stripped; an empty stream returns `Ok(None)`.
fn read_line_limited(reader: &mut impl BufRead, max: usize) -> Result<Option<String>, LineError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(LineError::Io)?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return String::from_utf8(line).map(Some).map_err(|_| LineError::NotUtf8);
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len() + newline > max {
                reader.consume(newline + 1);
                return Err(LineError::TooLarge);
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return String::from_utf8(line).map(Some).map_err(|_| LineError::NotUtf8);
        }
        if line.len() + available.len() > max {
            let take = available.len();
            reader.consume(take);
            return Err(LineError::TooLarge);
        }
        let take = available.len();
        line.extend_from_slice(available);
        reader.consume(take);
    }
}

/// One item read and decoded during the connection loop.
#[allow(clippy::large_enum_variant)]
enum Polled {
    /// A decoded frame.
    Frame(AgentFrame),
    /// The poll interval elapsed without input.
    Timeout,
    /// The peer closed or sent unreadable input.
    Closed,
}

/// Polls one frame without writing a timeout error to the peer.
fn poll_frame(
    incoming: &Receiver<Incoming>,
    writer: &mut dyn Write,
    max_frame_bytes: usize,
) -> Result<Polled, CoreError> {
    match incoming.recv_timeout(Duration::from_millis(200)) {
        Ok(item) => match decode_incoming(item, writer, max_frame_bytes)? {
            Some(frame) => Ok(Polled::Frame(frame)),
            None => Ok(Polled::Closed),
        },
        Err(RecvTimeoutError::Timeout) => Ok(Polled::Timeout),
        Err(RecvTimeoutError::Disconnected) => Ok(Polled::Closed),
    }
}

/// Reads and decodes one frame, answering malformed or oversized input with a typed `Error`.
fn read_frame(
    incoming: &Receiver<Incoming>,
    deadline: Option<Instant>,
    writer: &mut dyn Write,
    max_frame_bytes: usize,
) -> Result<Option<AgentFrame>, CoreError> {
    let item = match deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match incoming.recv_timeout(remaining) {
                Ok(item) => item,
                Err(RecvTimeoutError::Timeout) => {
                    send_error(writer, ErrorKind::Timeout, "gateway handshake timed out", true)?;
                    return Ok(None);
                }
                Err(RecvTimeoutError::Disconnected) => Incoming::Eof,
            }
        }
        None => match incoming.recv() {
            Ok(item) => item,
            Err(_) => Incoming::Eof,
        },
    };
    decode_incoming(item, writer, max_frame_bytes)
}

/// Decodes one item read from the input thread.
fn decode_incoming(
    item: Incoming,
    writer: &mut dyn Write,
    max_frame_bytes: usize,
) -> Result<Option<AgentFrame>, CoreError> {
    match item {
        Incoming::Line(line) => match serde_json::from_str(&line) {
            Ok(frame) => Ok(Some(frame)),
            Err(_) => {
                send_error(writer, ErrorKind::Parse, "invalid agent frame", false)?;
                Ok(None)
            }
        },
        Incoming::TooLarge => {
            send_error(
                writer,
                ErrorKind::InvalidInput,
                format!("frame exceeds {max_frame_bytes} bytes"),
                false,
            )?;
            Ok(None)
        }
        Incoming::NotUtf8 => {
            send_error(writer, ErrorKind::Parse, "agent frame is not UTF-8", false)?;
            Ok(None)
        }
        Incoming::Eof => Ok(None),
        Incoming::Io(error) => Err(CoreError::Io {
            path: "agent gateway input".to_owned(),
            source: error,
        }),
    }
}

/// The process-wide live connection id per `(home root, node)`.
fn connections() -> &'static Mutex<BTreeMap<String, String>> {
    static CONNECTIONS: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    CONNECTIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Builds the registry key without leaking a path into a log or frame.
fn connection_key(store: &Store, node: &NodeName) -> String {
    let root = store
        .paths()
        .map(|paths| paths.root().display().to_string())
        .unwrap_or_default();
    format!("{root}:{node}")
}

/// Records `connection_id` as the live connection for the node.
fn register_connection(store: &Store, node: &NodeName, connection_id: &str) {
    let mut live = connections().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    live.insert(connection_key(store, node), connection_id.to_owned());
}

/// Returns whether `connection_id` is still the live connection for the node.
fn connection_is_current(store: &Store, node: &NodeName, connection_id: &str) -> bool {
    let live = connections().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    live.get(&connection_key(store, node)).map(String::as_str) == Some(connection_id)
}

/// Returns the live connection id for the node, when one is registered.
fn current_connection(store: &Store, node: &NodeName) -> Option<String> {
    let live = connections().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    live.get(&connection_key(store, node)).cloned()
}

/// Removes the registration when this connection is still the live one.
fn unregister_connection(store: &Store, node: &NodeName, connection_id: &str) {
    let mut live = connections().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = connection_key(store, node);
    if live.get(&key).map(String::as_str) == Some(connection_id) {
        live.remove(&key);
    }
}

/// Writes one JSON frame followed by a newline and flushes it.
fn send_frame(writer: &mut dyn Write, frame: &AgentFrame) -> Result<(), CoreError> {
    let mut bytes = serde_json::to_vec(frame).map_err(vowp_protocol::ProtocolError::from)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).map_err(|error| CoreError::Io {
        path: "agent gateway output".to_owned(),
        source: error,
    })?;
    writer.flush().map_err(|error| CoreError::Io {
        path: "agent gateway output".to_owned(),
        source: error,
    })
}

/// Writes one typed `Error` frame.
fn send_error(
    writer: &mut dyn Write,
    kind: ErrorKind,
    message: impl Into<String>,
    retryable: bool,
) -> Result<(), CoreError> {
    send_frame(
        writer,
        &AgentFrame::Error {
            kind,
            message: message.into(),
            retryable,
        },
    )
}
