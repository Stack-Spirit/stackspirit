//! The Phase 0 stdio gateway: enroll, authenticate, heartbeat, ping, revocation and surface
//! pairing.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.2–§4.3, §8.2–§8.3, §12.1 and §12.3.
//! [`serve`] reads one JSON [`AgentFrame`] per line from stdin and writes one JSON frame per
//! line to stdout. Standard error is the caller's; this module never logs a code, surface
//! token, signature, public-key value or path map.
//!
//! Phase 0 runs the gateway as a home command with the same store access as `session ingest`.
//! The low-privilege gateway behind the supervisor socket is Phase 4. There are no runs,
//! leases, local chat, offload or job dispatch here. Surface pairing is answered on the
//! authenticated connection; a refused `SurfacePair` keeps the connection open so the agent
//! can retry with a fresh code, while an internal store failure closes it.

use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use vowp_protocol::{
    AGENT_PROTOCOL, AgentCapabilities, AgentFrame, ErrorKind, Level, NodeName, NodeStatus, SurfaceState,
};
use vowp_store::{NodeCredential, Store};

use super::{auth, enroll, pair};
use crate::deps::Deps;
use crate::error::CoreError;

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
            connection_id,
        },
    )?;
    Ok(Some(Session {
        node: hello.node,
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
    loop {
        let Some(frame) = read_frame(incoming, None, writer, options.max_frame_bytes)? else {
            return Ok(GatewayOutcome::Closed {
                node: Some(session.node),
            });
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
