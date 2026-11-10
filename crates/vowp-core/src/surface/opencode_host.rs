//! The opencode app host: the managed v2 service, its pair link and its transcript.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.5 and the S3 spike
//! (`docs/maintainers/v0.2-spikes/app-surfaces.md`). The host runs opencode v2 as a managed
//! user service under a **private** `XDG_STATE_HOME`/`XDG_DATA_HOME` below the orchestrator
//! directory, binds `127.0.0.1` only, and talks to the server with HTTP Basic
//! `opencode:<token>`. The pair link is printed on demand by `opencode pair`; this module
//! never stores or logs it.
//!
//! The HTTP client is deliberately small and blocking: opencode is a loopback service and
//! the workspace already prefers plain threads over an async runtime.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vowp_protocol::{JoinInstructions, OrchestratorApp, Timestamp};

use crate::CoreError;

use super::mirror::MirrorCursor;
use super::native::{
    AppHandle, AppHost, HostHealth, HostRequest, TranscriptBatch, TranscriptEntry, TranscriptKind, process_alive,
    remove_private, shell_quote_text, stop_pid, write_private_json,
};

/// How long `start` waits for the service and its first authenticated answer.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// How long one HTTP request may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);

/// How long one `/api/event` read may take after the request is sent.
const SSE_WINDOW: Duration = Duration::from_millis(750);

/// The opencode app host.
///
/// Construct it with [`OpencodeHost::new`] in tests to point at a fake binary; the default
/// runs `opencode` from `PATH`.
#[derive(Debug, Clone)]
pub struct OpencodeHost {
    binary: PathBuf,
    startup: Duration,
    sse_window: Duration,
}

impl Default for OpencodeHost {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("opencode"),
            startup: STARTUP_TIMEOUT,
            sse_window: SSE_WINDOW,
        }
    }
}

impl OpencodeHost {
    /// Creates a host that runs `binary`.
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            startup: STARTUP_TIMEOUT,
            sse_window: SSE_WINDOW,
        }
    }

    /// Overrides how long `start` waits for the service.
    pub fn with_startup(mut self, timeout: Duration) -> Self {
        self.startup = timeout;
        self
    }

    /// Overrides how long one live-event read waits.
    pub fn with_sse_window(mut self, window: Duration) -> Self {
        self.sse_window = window;
        self
    }

    /// Returns the shell command that prints a fresh one-time pair link.
    ///
    /// The command is safe to show; the link it prints is a secret with a five-minute life.
    pub fn pair_command(&self, handle: &AppHandle) -> Result<String, CoreError> {
        let layout = Layout::for_workdir(&handle.workdir);
        if read_host_state(&layout)?.is_none() {
            return Err(CoreError::NotFound("opencode host".to_owned()));
        }
        Ok(format!(
            "cd {} && XDG_STATE_HOME={} {} pair",
            shell_quote(&layout.workdir),
            shell_quote(&layout.state),
            shell_quote(&self.binary)
        ))
    }

    /// Reads the newest transcript entries after `cursor`.
    ///
    /// The first pass backfills from the session export; later passes consume `/api/event`.
    /// The mirror's `native_events` map makes overlapping entries harmless.
    fn read_transcript(&self, handle: &AppHandle, cursor: &MirrorCursor) -> Result<TranscriptBatch, CoreError> {
        let layout = Layout::for_workdir(&handle.workdir);
        let Some(state) = read_host_state(&layout)? else {
            return Ok(TranscriptBatch {
                entries: Vec::new(),
                cursor: cursor.clone(),
            });
        };
        let native = state
            .native_session_id
            .clone()
            .ok_or_else(|| CoreError::Invalid("opencode host has no app session".to_owned()))?;
        let client = self.client(&layout)?;
        let mut entries = Vec::new();
        if cursor.last_native_id.is_none() {
            entries.extend(self.backfill(&client, &native)?);
        }
        entries.extend(self.live_events(&client, &native)?);
        for entry in &mut entries {
            if entry.native_session.is_none() {
                entry.native_session = Some(native.clone());
            }
        }
        let entries = after_cursor(entries, cursor);
        let last = entries
            .last()
            .map(|entry| entry.native_id.clone())
            .or_else(|| cursor.last_native_id.clone());
        Ok(TranscriptBatch {
            entries,
            cursor: MirrorCursor {
                last_native_id: last,
                updated_at: Timestamp::now(),
            },
        })
    }

    /// Backfills from the export endpoint, falling back to the message list.
    fn backfill(&self, client: &HttpClient, native: &str) -> Result<Vec<TranscriptEntry>, CoreError> {
        if super::native::safe_segment(native) {
            let export = format!("/api/experimental/session/{native}/export");
            if let Ok((status, value)) = client.get_json(&export) {
                if (200..300).contains(&status) {
                    let entries = parse_message_list(&value);
                    if !entries.is_empty() {
                        return Ok(entries);
                    }
                }
            }
            let messages = format!("/api/session/{native}/message");
            if let Ok((status, value)) = client.get_json(&messages) {
                if (200..300).contains(&status) {
                    return Ok(parse_message_list(&value));
                }
            }
        }
        Ok(Vec::new())
    }

    /// Reads one bounded window of the live `/api/event` stream.
    fn live_events(&self, client: &HttpClient, native: &str) -> Result<Vec<TranscriptEntry>, CoreError> {
        let events = client.stream_events("/api/event", self.sse_window)?;
        let mut entries = Vec::new();
        for event in events {
            if session_of(&event).is_some_and(|session| session != native) {
                continue;
            }
            entries.extend(entries_from_api_event(&event));
        }
        Ok(entries)
    }

    /// Builds a client from the service state in `layout`.
    fn client(&self, layout: &Layout) -> Result<HttpClient, CoreError> {
        let service = read_service_state(layout)?;
        let url = service
            .url
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", service.port.unwrap_or_default()));
        HttpClient::new(&url, &service.password)
    }

    /// Runs one `opencode service …` command in the private state tree.
    fn run_service(&self, layout: &Layout, args: &[&str]) -> Result<(), CoreError> {
        let status = self
            .service_command(layout, args)?
            .status()
            .map_err(|source| CoreError::Io {
                path: self.binary.display().to_string(),
                source,
            })?;
        if !status.success() {
            return Err(CoreError::Command(format!(
                "opencode {} failed: exit {:?}: {}",
                args.join(" "),
                status.code(),
                read_tail(&layout.service_log)
            )));
        }
        Ok(())
    }

    /// Runs one `opencode service …` command, ignoring a non-zero exit.
    fn run_service_best_effort(&self, layout: &Layout, args: &[&str]) {
        if let Ok(mut command) = self.service_command(layout, args) {
            let _ = command.status();
        }
    }

    /// Builds the isolated service command.
    ///
    /// Output goes to the private `service.log`; a background child that inherits the
    /// write end must never keep `Command::output` blocked.
    fn service_command(&self, layout: &Layout, args: &[&str]) -> Result<Command, CoreError> {
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&layout.service_log)
            .map_err(|source| CoreError::Io {
                path: layout.service_log.display().to_string(),
                source,
            })?;
        let _ = fs::set_permissions(&layout.service_log, fs::Permissions::from_mode(0o600));
        let errors = log.try_clone().map_err(|source| CoreError::Io {
            path: layout.service_log.display().to_string(),
            source,
        })?;
        let mut command = Command::new(&self.binary);
        command
            .args(args)
            .current_dir(&layout.workdir)
            .env("XDG_CONFIG_HOME", &layout.config)
            .env("XDG_DATA_HOME", &layout.data)
            .env("XDG_STATE_HOME", &layout.state)
            .env("XDG_CACHE_HOME", &layout.cache)
            .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
            .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors));
        Ok(command)
    }
}

impl AppHost for OpencodeHost {
    fn kind(&self) -> OrchestratorApp {
        OrchestratorApp::Opencode
    }

    fn start(&self, req: &HostRequest) -> Result<AppHandle, CoreError> {
        let layout = Layout::for_workdir(&req.workdir);
        fs::create_dir_all(&layout.root).map_err(|source| CoreError::Io {
            path: layout.root.display().to_string(),
            source,
        })?;
        let _ = fs::set_permissions(&layout.root, fs::Permissions::from_mode(0o700));

        let previous = read_host_state(&layout)?;
        if let Some(state) = &previous {
            if process_alive(state.pid) {
                return Err(CoreError::Invalid(format!(
                    "opencode host for {} is already running (pid {})",
                    layout.workdir.display(),
                    state.pid
                )));
            }
            remove_private(&layout.host_json);
        }
        let reuse_native = previous.and_then(|state| state.native_session_id);

        let port = match req.port {
            Some(port) if port > 0 => port,
            Some(_) => return Err(CoreError::Invalid("port 0 is not a valid opencode port".to_owned())),
            None => free_port()?,
        };
        self.run_service_best_effort(&layout, &["service", "stop"]);
        remove_private(&layout.service_json);
        self.run_service(&layout, &["service", "set", "port", &port.to_string()])?;
        self.run_service(&layout, &["service", "start"])?;

        let started = (|| -> Result<AppHandle, CoreError> {
            let started_at = Timestamp::now();
            let service = self.wait_for_service(&layout, port)?;
            let client = self.client(&layout)?;
            let native = match reuse_native {
                Some(native) if self.session_exists(&client, &native) => native,
                _ => self.create_native_session(&client, req)?,
            };
            let pid = service
                .pid
                .ok_or_else(|| CoreError::Invalid("opencode service.json has no pid".to_owned()))?;
            let state = HostState {
                kind: "opencode".to_owned(),
                name: req.name.clone(),
                pid,
                port: Some(port),
                url: service
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("http://127.0.0.1:{port}")),
                started_at,
                workdir: layout.workdir.display().to_string(),
                service_json: layout.service_json.display().to_string(),
                native_session_id: Some(native),
                vowp_session: req.session.to_string(),
            };
            write_private_json(&layout.host_json, &state)?;
            Ok(AppHandle {
                kind: OrchestratorApp::Opencode,
                pid,
                port: Some(port),
                started_at,
                workdir: req.workdir.clone(),
            })
        })();
        if started.is_err() {
            self.run_service_best_effort(&layout, &["service", "stop"]);
            remove_private(&layout.host_json);
            remove_private(&layout.service_json);
        }
        started
    }

    fn stop(&self, handle: &AppHandle) -> Result<(), CoreError> {
        let layout = Layout::for_workdir(&handle.workdir);
        self.run_service_best_effort(&layout, &["service", "stop"]);
        let state = read_host_state(&layout)?;
        if let Some(state) = &state {
            stop_pid(state.pid);
        } else if process_alive(handle.pid) {
            stop_pid(handle.pid);
        }
        remove_private(&layout.host_json);
        remove_private(&layout.service_json);
        Ok(())
    }

    fn health(&self, handle: &AppHandle) -> Result<HostHealth, CoreError> {
        let layout = Layout::for_workdir(&handle.workdir);
        if read_host_state(&layout)?.is_none() {
            return Ok(HostHealth {
                ok: false,
                detail: "opencode host is not running".to_owned(),
            });
        }
        let client = match self.client(&layout) {
            Ok(client) => client,
            Err(error) => {
                return Ok(HostHealth {
                    ok: false,
                    detail: error.to_string(),
                });
            }
        };
        match client.get_json("/api/session") {
            Ok((status, _)) if (200..300).contains(&status) => Ok(HostHealth {
                ok: true,
                detail: "authenticated GET /api/session succeeded".to_owned(),
            }),
            Ok((status, _)) => Ok(HostHealth {
                ok: false,
                detail: format!("opencode answered HTTP {status} on /api/session"),
            }),
            Err(error) => Ok(HostHealth {
                ok: false,
                detail: error.to_string(),
            }),
        }
    }

    fn join_instructions(&self, handle: &AppHandle) -> Result<JoinInstructions, CoreError> {
        let layout = Layout::for_workdir(&handle.workdir);
        let state = read_host_state(&layout)?.ok_or_else(|| CoreError::NotFound("opencode host".to_owned()))?;
        let port = state.port.unwrap_or_default();
        let native = state
            .native_session_id
            .clone()
            .unwrap_or_else(|| "<opencode-session>".to_owned());
        let pair = self.pair_command(handle)?;
        let remote = format!(
            "cd {} && {} --server http://127.0.0.1:{port} --session {}",
            shell_quote(&layout.workdir),
            shell_quote(&self.binary),
            shell_quote_text(&native)
        );
        Ok(JoinInstructions {
            desktop: format!(
                "1. On the control machine: vowp tunnel open {} --remote-port {port}\n\
                 2. On the home: {pair}\n\
                 3. Open the one-time link it prints (5 minutes, single use) in the opencode desktop app, replacing its port with the tunnel's local port.",
                shell_quote_text(&state.name)
            ),
            phone: format!("ssh -t <home-ssh-target> {}", shell_quote_text(&remote)),
            chat: format!("vowp chat {}", state.vowp_session),
        })
    }

    fn transcript_since(&self, handle: &AppHandle, cursor: &MirrorCursor) -> Result<TranscriptBatch, CoreError> {
        self.read_transcript(handle, cursor)
    }
}

impl OpencodeHost {
    /// Polls the service state until an authenticated request answers.
    fn wait_for_service(&self, layout: &Layout, port: u16) -> Result<ServiceState, CoreError> {
        let deadline = Instant::now() + self.startup;
        let mut last = "service.json was not written".to_owned();
        while Instant::now() < deadline {
            match read_service_state(layout) {
                Ok(service) => {
                    let url = service
                        .url
                        .clone()
                        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
                    match HttpClient::new(&url, &service.password) {
                        Ok(client) => match client.get_json("/api/session") {
                            Ok((status, _)) if (200..300).contains(&status) => return Ok(service),
                            Ok((status, _)) => last = format!("opencode answered HTTP {status} on /api/session"),
                            Err(error) => last = error.to_string(),
                        },
                        Err(error) => last = error.to_string(),
                    }
                }
                Err(error) => last = error.to_string(),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.run_service_best_effort(layout, &["service", "stop"]);
        Err(CoreError::Command(format!(
            "opencode service did not become healthy within {:?}: {last}",
            self.startup
        )))
    }

    /// Creates the app-side session in the orchestrator directory.
    fn create_native_session(&self, client: &HttpClient, req: &HostRequest) -> Result<String, CoreError> {
        let body = serde_json::json!({
            "title": req.name,
            "location": {"directory": req.workdir.display().to_string()},
        });
        let (status, value) = client.post_json("/api/session", &body)?;
        if !(200..300).contains(&status) {
            return Err(CoreError::Command(format!(
                "opencode refused POST /api/session with HTTP {status}"
            )));
        }
        value
            .pointer("/data/id")
            .and_then(Value::as_str)
            .or_else(|| value.get("id").and_then(Value::as_str))
            .map(str::to_owned)
            .ok_or_else(|| CoreError::Invalid("opencode POST /api/session returned no session id".to_owned()))
    }

    /// Returns true when the named app session still exists.
    fn session_exists(&self, client: &HttpClient, native: &str) -> bool {
        if !super::native::safe_segment(native) {
            return false;
        }
        let path = format!("/api/session/{native}/message");
        matches!(client.get_json(&path), Ok((status, _)) if (200..300).contains(&status))
    }
}

/// The private directory layout of one opencode orchestrator.
#[derive(Debug, Clone)]
struct Layout {
    workdir: PathBuf,
    root: PathBuf,
    config: PathBuf,
    data: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    service_json: PathBuf,
    service_log: PathBuf,
    host_json: PathBuf,
}

impl Layout {
    /// Builds the layout below `workdir`.
    fn for_workdir(workdir: &Path) -> Self {
        let root = workdir.join(".opencode");
        let state = root.join("state");
        Self {
            workdir: workdir.to_path_buf(),
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            service_json: state.join("opencode").join("service.json"),
            service_log: root.join("service.log"),
            host_json: workdir.join("host.json"),
            root,
            state,
        }
    }
}

/// What `host.json` records while the host runs (mode 0600).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostState {
    kind: String,
    name: String,
    pid: u32,
    port: Option<u16>,
    url: String,
    started_at: Timestamp,
    workdir: String,
    service_json: String,
    native_session_id: Option<String>,
    vowp_session: String,
}

/// The subset of opencode's own `service.json` this host needs.
///
/// Deliberately not `Debug`: `password` is a secret.
#[derive(Clone, Deserialize)]
struct ServiceState {
    url: Option<String>,
    pid: Option<u32>,
    port: Option<u16>,
    password: String,
}

/// Reads `host.json`, returning `None` when the file is absent.
fn read_host_state(layout: &Layout) -> Result<Option<HostState>, CoreError> {
    match fs::read(&layout.host_json) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| CoreError::Invalid(format!("host.json: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CoreError::Io {
            path: layout.host_json.display().to_string(),
            source,
        }),
    }
}

/// Reads opencode's `service.json`.
fn read_service_state(layout: &Layout) -> Result<ServiceState, CoreError> {
    read_service_state_or_none(layout)?
        .ok_or_else(|| CoreError::NotFound(format!("opencode service state {}", layout.service_json.display())))
}

/// Reads opencode's `service.json`, returning `None` when the file is absent.
fn read_service_state_or_none(layout: &Layout) -> Result<Option<ServiceState>, CoreError> {
    match fs::read(&layout.service_json) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| CoreError::Invalid(format!("opencode service.json: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CoreError::Io {
            path: layout.service_json.display().to_string(),
            source,
        }),
    }
}

/// Returns a free loopback port.
fn free_port() -> Result<u16, CoreError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|source| CoreError::Io {
        path: "127.0.0.1:0".to_owned(),
        source,
    })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|source| CoreError::Io {
            path: "127.0.0.1:0".to_owned(),
            source,
        })
}

/// Reads the last 500 characters of a log file.
fn read_tail(path: &Path) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    chars[chars.len().saturating_sub(500)..].iter().collect()
}

/// Quotes one shell argument with single quotes.
fn shell_quote(value: &Path) -> String {
    shell_quote_text(&value.to_string_lossy())
}

/// A minimal blocking HTTP client for the loopback opencode API.
///
/// Deliberately not `Debug`: `auth` carries the service password.
#[derive(Clone)]
struct HttpClient {
    host: String,
    port: u16,
    auth: String,
    timeout: Duration,
}

impl HttpClient {
    /// Builds a client for `url` and an `opencode:<password>` Basic header.
    fn new(url: &str, password: &str) -> Result<Self, CoreError> {
        let (host, port) = loopback_address(url)?;
        Ok(Self {
            host,
            port,
            auth: format!("Basic {}", base64_encode(format!("opencode:{password}").as_bytes())),
            timeout: HTTP_TIMEOUT,
        })
    }

    /// Sends a request and reads the whole response.
    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, String), CoreError> {
        let mut stream = TcpStream::connect((self.host.as_str(), self.port)).map_err(|source| CoreError::Io {
            path: format!("{}:{}", self.host, self.port),
            source,
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|source| CoreError::Io {
                path: format!("{}:{}", self.host, self.port),
                source,
            })?;
        let payload = match body {
            Some(body) => serde_json::to_vec(body).map_err(|error| CoreError::Invalid(error.to_string()))?,
            None => Vec::new(),
        };
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.host, self.port, self.auth
        );
        if body.is_some() {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).map_err(|source| CoreError::Io {
            path: format!("{}:{}", self.host, self.port),
            source,
        })?;
        if !payload.is_empty() {
            stream.write_all(&payload).map_err(|source| CoreError::Io {
                path: format!("{}:{}", self.host, self.port),
                source,
            })?;
        }
        let bytes = read_to_end(&mut stream, self.timeout)?;
        parse_response(&bytes)
    }

    /// Sends a GET and parses a JSON body.
    fn get_json(&self, path: &str) -> Result<(u16, Value), CoreError> {
        let (status, body) = self.request("GET", path, None)?;
        let value = serde_json::from_str(&body).unwrap_or(Value::Null);
        Ok((status, value))
    }

    /// Sends a POST with a JSON body and parses a JSON response.
    fn post_json(&self, path: &str, body: &Value) -> Result<(u16, Value), CoreError> {
        let (status, text) = self.request("POST", path, Some(body))?;
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }

    /// Reads one bounded window of an SSE stream and parses its `data:` payloads.
    fn stream_events(&self, path: &str, window: Duration) -> Result<Vec<Value>, CoreError> {
        let mut stream = TcpStream::connect((self.host.as_str(), self.port)).map_err(|source| CoreError::Io {
            path: format!("{}:{}", self.host, self.port),
            source,
        })?;
        stream
            .set_read_timeout(Some(Duration::from_millis(150)))
            .map_err(|source| CoreError::Io {
                path: format!("{}:{}", self.host, self.port),
                source,
            })?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: {}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n",
            self.host, self.port, self.auth
        );
        stream.write_all(request.as_bytes()).map_err(|source| CoreError::Io {
            path: format!("{}:{}", self.host, self.port),
            source,
        })?;
        let deadline = Instant::now() + window;
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        while Instant::now() < deadline {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => bytes.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(source) => {
                    return Err(CoreError::Io {
                        path: format!("{}:{}", self.host, self.port),
                        source,
                    });
                }
            }
        }
        let body = sse_body(&bytes);
        let text = String::from_utf8_lossy(&body);
        let mut events = Vec::new();
        for line in text.lines() {
            let Some(data) = line.trim_end_matches('\r').strip_prefix("data:") else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                if value.is_object() {
                    events.push(value);
                }
            }
        }
        Ok(events)
    }
}

/// Reads until EOF, a read timeout or `timeout` overall.
fn read_to_end(stream: &mut TcpStream, timeout: Duration) -> Result<Vec<u8>, CoreError> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    while Instant::now() < deadline {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if !bytes.is_empty() {
                    break;
                }
            }
            Err(source) => {
                return Err(CoreError::Io {
                    path: "http".to_owned(),
                    source,
                });
            }
        }
    }
    Ok(bytes)
}

/// Parses an HTTP/1.1 response into status and body.
fn parse_response(bytes: &[u8]) -> Result<(u16, String), CoreError> {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(CoreError::Invalid("opencode answered without HTTP headers".to_owned()));
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let body_bytes = &bytes[header_end + 4..];
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| CoreError::Invalid("opencode answered without a status code".to_owned()))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
            if name == "content-length" {
                length = value.parse().ok();
            }
        }
    }
    let body = if chunked {
        decode_chunked(body_bytes)
    } else {
        match length {
            Some(length) if length <= body_bytes.len() => body_bytes[..length].to_vec(),
            _ => body_bytes.to_vec(),
        }
    };
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// Decodes a chunked body; malformed framing falls back to the raw bytes.
fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut position = 0usize;
    while position < bytes.len() {
        let Some(relative) = bytes[position..].windows(2).position(|window| window == b"\r\n") else {
            break;
        };
        let end = position + relative;
        let size_text = String::from_utf8_lossy(&bytes[position..end]);
        let Ok(size) = usize::from_str_radix(size_text.split(';').next().unwrap_or_default().trim(), 16) else {
            return bytes.to_vec();
        };
        if size == 0 {
            break;
        }
        let start = end + 2;
        let stop = start.saturating_add(size).min(bytes.len());
        output.extend_from_slice(&bytes[start..stop]);
        position = stop + 2;
    }
    output
}

/// Returns an SSE response body, decoding chunked framing when the server used it.
fn sse_body(bytes: &[u8]) -> Vec<u8> {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return bytes.to_vec();
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let chunked = headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("transfer-encoding") && value.trim().eq_ignore_ascii_case("chunked")
        })
    });
    if chunked {
        decode_chunked(&bytes[header_end + 4..])
    } else {
        bytes[header_end + 4..].to_vec()
    }
}

/// Parses `http://host:port[/…]` and refuses anything but loopback.
fn loopback_address(url: &str) -> Result<(String, u16), CoreError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| CoreError::Refused("opencode must use a loopback http URL".to_owned()))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| CoreError::Refused("opencode service URL has no port".to_owned()))?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(CoreError::Refused(format!(
            "opencode must bind 127.0.0.1; refusing {host}"
        )));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| CoreError::Refused(format!("invalid opencode port {port:?}")))?;
    Ok(("127.0.0.1".to_owned(), port))
}

/// Standard padded base64, used only for the HTTP Basic header.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(third & 0b11_1111) as usize] as char
        } else {
            '='
        });
    }
    output
}

/// Filters entries to those after the cursor id; an unknown cursor returns everything.
fn after_cursor(entries: Vec<TranscriptEntry>, cursor: &MirrorCursor) -> Vec<TranscriptEntry> {
    let Some(last) = cursor.last_native_id.as_deref() else {
        return entries;
    };
    match entries.iter().position(|entry| entry.native_id == last) {
        Some(position) => entries.into_iter().skip(position + 1).collect(),
        None => entries,
    }
}

/// Parses an opencode message list or export into transcript entries.
pub fn parse_message_list(value: &Value) -> Vec<TranscriptEntry> {
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .or_else(|| value.get("data").and_then(Value::as_array))
        .or_else(|| value.as_array());
    let Some(messages) = messages else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        push_message(message, index, &mut entries);
    }
    entries
}

/// Maps one opencode message (export or message list) into entries.
fn push_message(message: &Value, index: usize, entries: &mut Vec<TranscriptEntry>) {
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| message.get("messageID").and_then(Value::as_str))
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("message-{index}"));
    let role = message
        .get("type")
        .and_then(Value::as_str)
        .filter(|role| matches!(*role, "user" | "assistant"))
        .or_else(|| message.get("role").and_then(Value::as_str))
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    let ts = message_timestamp(message);
    let model = message_model(message);
    let session = message
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("sessionID"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned);
    let content = message
        .get("content")
        .or_else(|| message.get("parts"))
        .or_else(|| message.get("info").and_then(|info| info.get("content")));
    match role {
        "user" => {
            if let Some(content) = content {
                push_parts(content, &id, ts, &model, &session, true, entries);
            }
        }
        "assistant" => {
            if let Some(content) = content {
                push_parts(content, &id, ts, &model, &session, false, entries);
            }
            if let Some(usage) = message
                .get("usage")
                .or_else(|| message.get("tokens"))
                .or_else(|| message.get("info").and_then(|info| info.get("usage")))
            {
                entries.push(cost_from_usage(format!("{id}#usage"), ts, &model, &session, usage));
            }
        }
        _ => {}
    }
}

/// Maps a message content value (string or part array) into entries.
#[allow(clippy::too_many_arguments)]
fn push_parts(
    content: &Value,
    message_id: &str,
    ts: Timestamp,
    model: &Option<String>,
    session: &Option<String>,
    user: bool,
    entries: &mut Vec<TranscriptEntry>,
) {
    if let Some(text) = content.as_str() {
        entries.push(simple_entry(
            message_id.to_owned(),
            if user {
                TranscriptKind::UserMessage
            } else {
                TranscriptKind::AssistantMessage
            },
            ts,
            text.to_owned(),
            model,
            session,
        ));
        return;
    }
    let Some(parts) = content.as_array() else {
        return;
    };
    for (index, part) in parts.iter().enumerate() {
        let part_id = part
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{message_id}#{index}"));
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or_default();
        match part_type {
            "text" => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    entries.push(simple_entry(
                        part_id,
                        if user {
                            TranscriptKind::UserMessage
                        } else {
                            TranscriptKind::AssistantMessage
                        },
                        ts,
                        text.to_owned(),
                        model,
                        session,
                    ));
                }
            }
            "tool_use" | "tool" => {
                let call_id = part
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("callID").and_then(Value::as_str))
                    .unwrap_or(&part_id)
                    .to_owned();
                let input = part
                    .get("input")
                    .or_else(|| part.get("state").and_then(|state| state.get("input")))
                    .cloned()
                    .unwrap_or(Value::Null);
                entries.push(TranscriptEntry {
                    native_id: format!("tool-call:{call_id}"),
                    kind: TranscriptKind::ToolCall,
                    ts,
                    text: None,
                    tool: part
                        .get("name")
                        .or_else(|| part.get("tool"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    input: Some(input),
                    output: None,
                    path: None,
                    call_id: Some(call_id),
                    model: model.clone(),
                    native_session: session.clone(),
                    ok: None,
                    tokens: None,
                    usd: None,
                });
            }
            "tool_result" => {
                let call_id = part
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("callID").and_then(Value::as_str))
                    .unwrap_or(&part_id)
                    .to_owned();
                entries.push(TranscriptEntry {
                    native_id: format!("tool-result:{part_id}"),
                    kind: TranscriptKind::ToolResult,
                    ts,
                    text: None,
                    tool: None,
                    input: None,
                    output: text_value(part.get("content").or_else(|| part.get("output"))),
                    path: None,
                    call_id: Some(call_id),
                    model: model.clone(),
                    native_session: session.clone(),
                    ok: Some(!part.get("is_error").and_then(Value::as_bool).unwrap_or(false)),
                    tokens: None,
                    usd: None,
                });
            }
            _ => {}
        }
    }
}

/// Builds a message entry.
fn simple_entry(
    native_id: String,
    kind: TranscriptKind,
    ts: Timestamp,
    text: String,
    model: &Option<String>,
    session: &Option<String>,
) -> TranscriptEntry {
    TranscriptEntry {
        native_id,
        kind,
        ts,
        text: Some(text),
        tool: None,
        input: None,
        output: None,
        path: None,
        call_id: None,
        model: model.clone(),
        native_session: session.clone(),
        ok: None,
        tokens: None,
        usd: None,
    }
}

/// Maps one live `/api/event` payload to canonical transcript entries.
pub fn entries_from_api_event(event: &Value) -> Vec<TranscriptEntry> {
    let Some(kind) = event.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };
    let data = event.get("data").unwrap_or(&Value::Null);
    let ts = event_timestamp(event);
    let session = session_of(event);
    let model = data
        .get("model")
        .and_then(model_name)
        .or_else(|| data.get("modelID").and_then(Value::as_str).map(str::to_owned));
    let seq = data
        .get("seq")
        .and_then(Value::as_u64)
        .or_else(|| event.get("seq").and_then(Value::as_u64));
    let marker = seq
        .map(|seq| seq.to_string())
        .or_else(|| data.get("id").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| ts.to_string());
    match kind {
        "session.inbox.enqueued" => {
            let item = data.get("item").unwrap_or(&Value::Null);
            if item.get("type").and_then(Value::as_str) != Some("user") {
                return Vec::new();
            }
            let text = item
                .pointer("/payload/text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if text.is_empty() {
                return Vec::new();
            }
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("user:{marker}"));
            vec![simple_entry(
                id,
                TranscriptKind::UserMessage,
                ts,
                text.to_owned(),
                &model,
                &session,
            )]
        }
        "session.execution.started" => vec![TranscriptEntry {
            native_id: format!("turn-start:{marker}"),
            kind: TranscriptKind::TurnStarted,
            ts,
            text: None,
            tool: None,
            input: None,
            output: None,
            path: None,
            call_id: None,
            model,
            native_session: session,
            ok: None,
            tokens: None,
            usd: None,
        }],
        "session.text.ended" => {
            let text = data.get("text").and_then(Value::as_str).unwrap_or_default();
            if text.is_empty() {
                return Vec::new();
            }
            let id = data
                .get("assistantMessageID")
                .and_then(Value::as_str)
                .or_else(|| data.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .unwrap_or_else(|| format!("assistant:{marker}"));
            vec![simple_entry(
                id,
                TranscriptKind::AssistantMessage,
                ts,
                text.to_owned(),
                &model,
                &session,
            )]
        }
        "session.tool.called" => {
            let call_id = data.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            vec![TranscriptEntry {
                native_id: format!("tool-call:{call_id}"),
                kind: TranscriptKind::ToolCall,
                ts,
                text: None,
                tool: data
                    .get("name")
                    .or_else(|| data.get("tool"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                input: Some(data.get("input").cloned().unwrap_or(Value::Null)),
                output: None,
                path: None,
                call_id: Some(call_id),
                model,
                native_session: session,
                ok: None,
                tokens: None,
                usd: None,
            }]
        }
        "session.tool.success" | "session.tool.failed" => {
            let call_id = data.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let ok = kind == "session.tool.success";
            let output = if ok {
                text_value(data.get("content"))
            } else {
                data.pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            vec![TranscriptEntry {
                native_id: format!("tool-result:{marker}"),
                kind: TranscriptKind::ToolResult,
                ts,
                text: None,
                tool: None,
                input: None,
                output,
                path: None,
                call_id: Some(call_id),
                model,
                native_session: session,
                ok: Some(ok),
                tokens: None,
                usd: None,
            }]
        }
        "session.usage.updated" => {
            let usage = data.get("tokens").or_else(|| data.get("usage")).unwrap_or(data);
            let mut entry = cost_from_usage(format!("cost:{marker}"), ts, &model, &session, usage);
            entry.usd = data.get("cost").and_then(Value::as_f64);
            vec![entry]
        }
        "session.execution.succeeded" | "session.execution.interrupted" => {
            let outcome = if kind.ends_with("interrupted") {
                "interrupted"
            } else {
                "succeeded"
            };
            vec![turn_end_entry(
                format!("turn-end:{marker}"),
                ts,
                outcome,
                &model,
                &session,
            )]
        }
        "session.execution.failed" => {
            let message = data
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("session execution failed")
                .to_owned();
            vec![
                TranscriptEntry {
                    native_id: format!("error:{marker}"),
                    kind: TranscriptKind::Error,
                    ts,
                    text: Some(message),
                    tool: None,
                    input: None,
                    output: None,
                    path: None,
                    call_id: None,
                    model: model.clone(),
                    native_session: session.clone(),
                    ok: None,
                    tokens: None,
                    usd: None,
                },
                turn_end_entry(format!("turn-end:{marker}"), ts, "failed", &model, &session),
            ]
        }
        "session.step.failed" => {
            let message = data
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("step failed")
                .to_owned();
            vec![TranscriptEntry {
                native_id: format!("step-error:{marker}"),
                kind: TranscriptKind::Error,
                ts,
                text: Some(message),
                tool: None,
                input: None,
                output: None,
                path: None,
                call_id: None,
                model,
                native_session: session,
                ok: None,
                tokens: None,
                usd: None,
            }]
        }
        "session.model.selected" => {
            let selected = data
                .get("model")
                .and_then(model_name)
                .or_else(|| data.get("modelID").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            vec![simple_entry(
                format!("model:{marker}"),
                TranscriptKind::Note,
                ts,
                format!("app selected model {selected}"),
                &model,
                &session,
            )]
        }
        _ => Vec::new(),
    }
}

/// Builds a turn-end entry.
fn turn_end_entry(
    native_id: String,
    ts: Timestamp,
    outcome: &str,
    model: &Option<String>,
    session: &Option<String>,
) -> TranscriptEntry {
    TranscriptEntry {
        native_id,
        kind: TranscriptKind::TurnEnded,
        ts,
        text: Some(outcome.to_owned()),
        tool: None,
        input: None,
        output: None,
        path: None,
        call_id: None,
        model: model.clone(),
        native_session: session.clone(),
        ok: None,
        tokens: None,
        usd: None,
    }
}

/// Builds a cost entry from a usage object.
fn cost_from_usage(
    native_id: String,
    ts: Timestamp,
    model: &Option<String>,
    session: &Option<String>,
    usage: &Value,
) -> TranscriptEntry {
    let tokens = vowp_protocol::Tokens {
        input: u64_field(usage, &["input", "input_tokens", "inputTokens"]),
        output: u64_field(usage, &["output", "output_tokens", "outputTokens"]),
        reasoning: u64_field(usage, &["reasoning", "reasoning_tokens", "reasoningTokens"]),
        cache_read: usage
            .pointer("/cache/read")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| u64_field(usage, &["cache_read", "cacheReadInputTokens"])),
        cache_write: usage
            .pointer("/cache/write")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| u64_field(usage, &["cache_write", "cacheCreationInputTokens"])),
    };
    TranscriptEntry {
        native_id,
        kind: TranscriptKind::Cost,
        ts,
        text: None,
        tool: None,
        input: None,
        output: None,
        path: None,
        call_id: None,
        model: model.clone(),
        native_session: session.clone(),
        ok: None,
        tokens: Some(tokens),
        usd: usage.get("cost").and_then(Value::as_f64),
    }
}

/// Reads the first present unsigned field from a usage object.
fn u64_field(value: &Value, names: &[&str]) -> u64 {
    for name in names {
        if let Some(number) = value.get(*name).and_then(Value::as_u64) {
            return number;
        }
    }
    0
}

/// Returns an event's session id.
fn session_of(event: &Value) -> Option<String> {
    let data = event.get("data")?;
    data.get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/data/sessionID").and_then(Value::as_str))
        .map(str::to_owned)
}

/// Reads an event timestamp from `created` or `data.time.created`.
fn event_timestamp(event: &Value) -> Timestamp {
    event
        .get("created")
        .and_then(Value::as_i64)
        .or_else(|| event.pointer("/data/time/created").and_then(Value::as_i64))
        .map(Timestamp::from_unix_millis)
        .unwrap_or_else(Timestamp::now)
}

/// Reads a message timestamp.
fn message_timestamp(message: &Value) -> Timestamp {
    message
        .pointer("/time/created")
        .and_then(Value::as_i64)
        .or_else(|| message.get("created").and_then(Value::as_i64))
        .or_else(|| message.pointer("/info/time/created").and_then(Value::as_i64))
        .map(Timestamp::from_unix_millis)
        .unwrap_or_else(Timestamp::now)
}

/// Reads a message model name.
fn message_model(message: &Value) -> Option<String> {
    message
        .get("model")
        .and_then(model_name)
        .or_else(|| {
            message
                .get("info")
                .and_then(|info| info.get("model"))
                .and_then(model_name)
        })
        .or_else(|| message.get("modelID").and_then(Value::as_str).map(str::to_owned))
}

/// Reads a model value that is either a string or an object with an id.
fn model_name(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.get("modelID").and_then(Value::as_str).map(str::to_owned))
        .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_owned))
}

/// Joins a string-or-block-array value into text.
fn text_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| block.as_str())
                        .map(str::to_owned)
                })
                .collect();
            if parts.is_empty() { None } else { Some(parts.join("\n")) }
        }
        Value::Null => None,
        other => Some(other.to_string()),
    }
}
