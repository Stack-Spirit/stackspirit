//! The local agent socket: newline-delimited JSON between a local client and the home.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §5.4, §8.3 and §10.4–§10.5, and
//! `docs/design/crates/vowp-agent.md`. `vowp agent run` binds `<state_dir>/agent.sock` mode
//! 0600. A local client such as `vowp mcp --agent-socket … --surface …` writes one JSON
//! request per line. The server never opens the store, never reads a home secret and never
//! logs a request or response payload; tool calls travel over the one outbound agent
//! connection and the home stays authoritative for token scopes and leases.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vowp_protocol::SessionId;
use vowp_protocol::SurfaceId;
use vowp_protocol::agent::surface::{ToolCall, ToolError};

use crate::client::validate_surface_id;
use crate::config::ensure_private_dir;
use crate::error::AgentError;

/// File name of the local agent socket below the state directory.
pub const AGENT_SOCKET_FILE: &str = "agent.sock";

/// Largest accepted request line, including its trailing newline.
pub const MAX_SOCKET_REQUEST_BYTES: usize = 1024 * 1024;

/// How long a tool call waits for the home's `ToolResult` before it expires.
pub const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the frame loop wakes to service local socket requests.
pub(crate) const SOCKET_POLL: Duration = Duration::from_millis(50);

/// Poll interval of the accept thread; it also lets the thread observe the stop flag.
const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// Read and write timeout of one accepted local client.
const CLIENT_IO_TIMEOUT: Duration = Duration::from_millis(250);

/// Upper bound on an attach or detach reply; a tool call has its own timeout.
const REPLY_TIMEOUT: Duration = Duration::from_secs(65);

/// One request written by a local client, one JSON object per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SocketRequest {
    /// Local liveness probe; answered without touching the home.
    Ping,
    /// Attach a surface to a session; forwarded as `SurfaceAttach`.
    Attach {
        /// Surface being attached.
        surface: SurfaceId,
        /// Session the surface will mirror.
        session: SessionId,
    },
    /// Detach a surface; forwarded as `SurfaceDetach`.
    Detach {
        /// Surface being detached.
        surface: SurfaceId,
        /// Session the surface was mirroring.
        session: SessionId,
    },
    /// Forward one MCP tool call as a `ToolCall` frame.
    ToolCall {
        /// Client-chosen request id, echoed by the matching `ToolResult`.
        request_id: String,
        /// Surface that made the call.
        surface: SurfaceId,
        /// Session the call is scoped to.
        session: SessionId,
        /// MCP tool name.
        tool: String,
        /// MCP arguments as a JSON object; never a local path or secret value.
        arguments: Value,
    },
}

/// One response written by the server, one JSON object per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SocketResponse {
    /// The attach or detach frame was written to the home connection.
    Ok,
    /// Reply to a local [`SocketRequest::Ping`].
    Pong,
    /// The home's answer to one tool call; exactly one of `result` and `error` is set.
    ToolResult {
        /// Request id from the matching tool call.
        request_id: String,
        /// Tool result as normal MCP JSON, when the call succeeded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        /// Typed failure, passed through unchanged when the home supplied it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ToolError>,
    },
    /// A local protocol failure that is not a tool result.
    Error {
        /// Machine-readable code, for example `unknown_kind` or `home_offline`.
        code: String,
        /// Human-readable message; never a payload, token or absolute local path.
        message: String,
    },
}

impl SocketResponse {
    pub(crate) fn error(code: &str, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn tool_error(request_id: &str, code: &str, message: impl Into<String>, retryable: bool) -> Self {
        Self::ToolResult {
            request_id: request_id.to_owned(),
            result: None,
            error: Some(ToolError {
                code: code.to_owned(),
                message: message.into(),
                retryable,
            }),
        }
    }
}

/// One local client request queued for the frame loop.
pub(crate) enum SocketEvent {
    Attach {
        surface: SurfaceId,
        session: SessionId,
        reply: Sender<SocketResponse>,
    },
    Detach {
        surface: SurfaceId,
        session: SessionId,
        reply: Sender<SocketResponse>,
    },
    ToolCall {
        call: ToolCall,
        reply: Sender<SocketResponse>,
    },
}

struct SocketShared {
    stop: AtomicBool,
    connected: AtomicBool,
    clients: AtomicUsize,
    inbox: Sender<SocketEvent>,
}

/// The mode-0600 Unix socket served by `vowp agent run`.
///
/// The listener accepts several local clients, one thread each, and queues their requests
/// for the single frame loop that owns the outbound home connection. Dropping the value
/// stops accepting, wakes the handler threads and removes the socket file only when it is
/// still the inode this server bound.
pub struct AgentSocket {
    path: PathBuf,
    bound: Option<(u64, u64)>,
    shared: Arc<SocketShared>,
    events: Receiver<SocketEvent>,
    accept: Option<JoinHandle<()>>,
}

impl AgentSocket {
    /// Binds `<state_dir>/agent.sock` mode 0600 and starts the accept thread.
    ///
    /// A live server is refused, so a second `vowp agent run` cannot take over the path. A
    /// stale socket file left by a crashed process is removed and rebound.
    pub fn bind(state_dir: &Path) -> Result<Self, AgentError> {
        ensure_private_dir(state_dir)?;
        let path = state_dir.join(AGENT_SOCKET_FILE);
        let listener = bind_listener(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let metadata = fs::metadata(&path)?;
        let bound = (metadata.dev(), metadata.ino());
        listener.set_nonblocking(true)?;

        let (sender, events) = mpsc::channel();
        let shared = Arc::new(SocketShared {
            stop: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            clients: AtomicUsize::new(0),
            inbox: sender,
        });
        let accept_shared = Arc::clone(&shared);
        let accept = match thread::Builder::new()
            .name("vowp-agent-socket".to_owned())
            .spawn(move || accept_loop(listener, accept_shared))
        {
            Ok(handle) => handle,
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(AgentError::Io(error));
            }
        };
        Ok(Self {
            path,
            bound: Some(bound),
            shared,
            events,
            accept: Some(accept),
        })
    }

    /// Returns the bound socket path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Updates whether the frame loop currently has a usable home connection.
    pub(crate) fn set_connected(&self, connected: bool) {
        self.shared.connected.store(connected, Ordering::SeqCst);
    }

    /// Drains requests queued by local client threads without blocking.
    pub(crate) fn drain_events(&self) -> Vec<SocketEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for AgentSocket {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.connected.store(false, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        let owned = fs::metadata(&self.path)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()))
            .is_some_and(|identity| Some(identity) == self.bound);
        if owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn bind_listener(path: &Path) -> Result<UnixListener, AgentError> {
    if path.exists() {
        match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                return Err(AgentError::Config("the agent socket is already in use".to_owned()));
            }
            Err(error) if is_stale_socket_error(&error) => remove_socket_file(path)?,
            Err(_) => {
                return Err(AgentError::Config(
                    "the agent socket exists and could not be probed".to_owned(),
                ));
            }
        }
    }
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        // The path can appear between the probe and the bind. Probe once more so only a
        // genuinely stale file is removed.
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                Err(AgentError::Config("the agent socket is already in use".to_owned()))
            }
            Err(probe) if is_stale_socket_error(&probe) => {
                remove_socket_file(path)?;
                UnixListener::bind(path).map_err(AgentError::Io)
            }
            Err(_) => Err(AgentError::Config("the agent socket is already in use".to_owned())),
        },
        Err(error) => Err(AgentError::Io(error)),
    }
}

fn remove_socket_file(path: &Path) -> Result<(), AgentError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AgentError::Io(error)),
    }
}

fn is_stale_socket_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound | io::ErrorKind::NotConnected
    )
}

fn accept_loop(listener: UnixListener, shared: Arc<SocketShared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                shared.clients.fetch_add(1, Ordering::SeqCst);
                let client_shared = Arc::clone(&shared);
                let spawned = thread::Builder::new()
                    .name("vowp-agent-socket-client".to_owned())
                    .spawn(move || handle_client(stream, client_shared));
                if spawned.is_err() {
                    shared.clients.fetch_sub(1, Ordering::SeqCst);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(_) => thread::sleep(ACCEPT_POLL),
        }
    }
}

fn handle_client(stream: UnixStream, shared: Arc<SocketShared>) {
    let _ = stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT));
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => {
            shared.clients.fetch_sub(1, Ordering::SeqCst);
            return;
        }
    };
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;
    let mut line = Vec::new();
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        match read_request_line(&mut reader, &mut line) {
            Ok(false) => break,
            Ok(true) => {
                let (response, close) = handle_request(&shared, &line);
                line.clear();
                if write_response(&mut writer, &response).is_err() || close {
                    break;
                }
            }
            Err(ReadLineError::Timeout) => {}
            Err(ReadLineError::TooLarge) => {
                let response = SocketResponse::error(
                    "frame_too_large",
                    format!("the request exceeds {MAX_SOCKET_REQUEST_BYTES} bytes"),
                );
                let _ = write_response(&mut writer, &response);
                break;
            }
            Err(ReadLineError::Io) => break,
        }
    }
    shared.clients.fetch_sub(1, Ordering::SeqCst);
}

fn handle_request(shared: &SocketShared, line: &[u8]) -> (SocketResponse, bool) {
    let request = match parse_request(line) {
        Ok(request) => request,
        Err(failure) => return (SocketResponse::error(failure.code, failure.message), false),
    };
    match request {
        SocketRequest::Ping => (SocketResponse::Pong, false),
        SocketRequest::Attach { surface, session } => {
            let (reply, receiver) = mpsc::channel();
            if !shared.connected.load(Ordering::SeqCst) {
                return (offline_response(), false);
            }
            if shared
                .inbox
                .send(SocketEvent::Attach {
                    surface,
                    session,
                    reply,
                })
                .is_err()
            {
                return (offline_response(), false);
            }
            (wait_for_reply(&receiver, shared, None), false)
        }
        SocketRequest::Detach { surface, session } => {
            let (reply, receiver) = mpsc::channel();
            if !shared.connected.load(Ordering::SeqCst) {
                return (offline_response(), false);
            }
            if shared
                .inbox
                .send(SocketEvent::Detach {
                    surface,
                    session,
                    reply,
                })
                .is_err()
            {
                return (offline_response(), false);
            }
            (wait_for_reply(&receiver, shared, None), false)
        }
        SocketRequest::ToolCall {
            request_id,
            surface,
            session,
            tool,
            arguments,
        } => {
            let (reply, receiver) = mpsc::channel();
            if !shared.connected.load(Ordering::SeqCst) {
                return (offline_tool_result(&request_id), false);
            }
            let call = ToolCall {
                request_id: request_id.clone(),
                surface,
                session,
                tool,
                arguments,
            };
            if shared.inbox.send(SocketEvent::ToolCall { call, reply }).is_err() {
                return (offline_tool_result(&request_id), false);
            }
            (wait_for_reply(&receiver, shared, Some(&request_id)), false)
        }
    }
}

fn wait_for_reply(
    receiver: &Receiver<SocketResponse>,
    shared: &SocketShared,
    tool_request: Option<&str>,
) -> SocketResponse {
    let started = Instant::now();
    loop {
        match receiver.recv_timeout(SOCKET_POLL) {
            Ok(response) => return response,
            Err(RecvTimeoutError::Disconnected) => return offline_for(tool_request),
            Err(RecvTimeoutError::Timeout) => {
                if !shared.connected.load(Ordering::SeqCst) {
                    return offline_for(tool_request);
                }
                if started.elapsed() >= REPLY_TIMEOUT {
                    return match tool_request {
                        Some(request_id) => SocketResponse::tool_error(
                            request_id,
                            "tool_timeout",
                            "the agent did not answer the tool call",
                            true,
                        ),
                        None => SocketResponse::error("agent_timeout", "the agent did not answer the local request"),
                    };
                }
            }
        }
    }
}

fn offline_for(tool_request: Option<&str>) -> SocketResponse {
    match tool_request {
        Some(request_id) => offline_tool_result(request_id),
        None => offline_response(),
    }
}

fn offline_response() -> SocketResponse {
    SocketResponse::error(
        "home_offline",
        "the agent is not connected to the home; start `vowp agent run`",
    )
}

fn offline_tool_result(request_id: &str) -> SocketResponse {
    SocketResponse::tool_error(
        request_id,
        "home_offline",
        "the agent is not connected to the home; start `vowp agent run`",
        true,
    )
}

struct SocketFailure {
    code: &'static str,
    message: String,
}

fn parse_request(line: &[u8]) -> Result<SocketRequest, SocketFailure> {
    let value: Value = serde_json::from_slice(line).map_err(|_| invalid_request())?;
    let kind = value.get("kind").and_then(Value::as_str).ok_or_else(invalid_request)?;
    if !matches!(kind, "ping" | "attach" | "detach" | "tool_call") {
        return Err(SocketFailure {
            code: "unknown_kind",
            message: "unknown request kind".to_owned(),
        });
    }
    let request: SocketRequest = serde_json::from_value(value).map_err(|_| invalid_request())?;
    validate_request(&request)?;
    Ok(request)
}

fn invalid_request() -> SocketFailure {
    SocketFailure {
        code: "invalid_request",
        message: "the request is not a valid socket request".to_owned(),
    }
}

fn validate_request(request: &SocketRequest) -> Result<(), SocketFailure> {
    match request {
        SocketRequest::Ping => Ok(()),
        SocketRequest::Attach { surface, .. } | SocketRequest::Detach { surface, .. } => validate_surface_id(surface)
            .map_err(|_| SocketFailure {
                code: "invalid_request",
                message: "surface must be a 26-character ULID".to_owned(),
            }),
        SocketRequest::ToolCall {
            request_id,
            surface,
            tool,
            arguments,
            ..
        } => {
            if request_id.is_empty() || request_id.len() > 128 || request_id.chars().any(char::is_control) {
                return Err(SocketFailure {
                    code: "invalid_request",
                    message: "request_id must be a printable string of at most 128 bytes".to_owned(),
                });
            }
            if tool.is_empty() || tool.len() > 128 || tool.chars().any(char::is_control) {
                return Err(SocketFailure {
                    code: "invalid_request",
                    message: "tool must be a printable name of at most 128 bytes".to_owned(),
                });
            }
            if !arguments.is_object() {
                return Err(SocketFailure {
                    code: "invalid_request",
                    message: "arguments must be a JSON object".to_owned(),
                });
            }
            validate_surface_id(surface).map_err(|_| SocketFailure {
                code: "invalid_request",
                message: "surface must be a 26-character ULID".to_owned(),
            })
        }
    }
}

fn read_request_line(reader: &mut BufReader<UnixStream>, line: &mut Vec<u8>) -> Result<bool, ReadLineError> {
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if is_io_timeout(&error) => return Err(ReadLineError::Timeout),
            Err(_) => return Err(ReadLineError::Io),
        };
        if buffer.is_empty() {
            return Ok(false);
        }
        if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let total = line.len().saturating_add(end).saturating_add(1);
            if total > MAX_SOCKET_REQUEST_BYTES {
                return Err(ReadLineError::TooLarge);
            }
            line.extend_from_slice(&buffer[..=end]);
            reader.consume(end + 1);
            return Ok(true);
        }
        let consumed = buffer.len();
        let total = line.len().saturating_add(consumed);
        if total > MAX_SOCKET_REQUEST_BYTES {
            return Err(ReadLineError::TooLarge);
        }
        line.extend_from_slice(buffer);
        reader.consume(consumed);
    }
}

fn is_io_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

enum ReadLineError {
    Timeout,
    TooLarge,
    Io,
}

fn write_response(writer: &mut UnixStream, response: &SocketResponse) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}
