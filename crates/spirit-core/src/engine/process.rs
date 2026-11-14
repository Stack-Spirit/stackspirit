//! Bundled sidecar launch, loopback health, bounded redacted logs and clean shutdown.
//!
//! The runtime starts only the binary resolved by [`EngineBinaryLocator`] from the
//! Tauri resource/binary directories. It never resolves the bundled engine from `PATH`.
//! All HTTP traffic is loopback-only and carries no provider key: key verification uses a
//! provider inventory read, never a credential write.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;

use crate::engine::SecretString;
use crate::engine::runtime::{
    EngineHealth, EngineRuntime, EngineRuntimeInfo, KeyVerification, RuntimeModel,
};
use crate::error::{SpiritError, SpiritErrorCode, redact};

/// The only host a bundled engine may listen on.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// How many engine log lines are retained.
pub const MAX_LOG_LINES: usize = 200;

const MAX_HTTP_BODY: usize = 2 * 1024 * 1024;

/// Returns the loopback host used for every bundled engine launch.
#[must_use]
pub const fn loopback_host() -> &'static str {
    LOOPBACK_HOST
}

/// Returns this build's Tauri target triple when it is a supported desktop target.
#[must_use]
pub fn current_target_triple() -> Option<&'static str> {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" if cfg!(target_env = "musl") => match arch {
            "x86_64" => Some("x86_64-unknown-linux-musl"),
            "aarch64" => Some("aarch64-unknown-linux-musl"),
            _ => None,
        },
        "linux" => match arch {
            "x86_64" => Some("x86_64-unknown-linux-gnu"),
            "aarch64" => Some("aarch64-unknown-linux-gnu"),
            _ => None,
        },
        "macos" => match arch {
            "x86_64" => Some("x86_64-apple-darwin"),
            "aarch64" => Some("aarch64-apple-darwin"),
            _ => None,
        },
        "windows" => match arch {
            "x86_64" => Some("x86_64-pc-windows-msvc"),
            "aarch64" => Some("aarch64-pc-windows-msvc"),
            _ => None,
        },
        _ => None,
    }
}

/// Tauri's sidecar file name for a target triple.
#[must_use]
pub fn sidecar_file_name(triple: &str) -> String {
    let executable = if triple.contains("windows") {
        "opencode.exe"
    } else {
        "opencode"
    };
    format!("{executable}-{triple}")
}

/// Resolves the bundled sidecar from Tauri's binary/resource locations.
///
/// The locator takes an explicit list of directories. It never consults `PATH`, the
/// current directory, or a shell profile.
#[derive(Debug, Clone, Default)]
pub struct EngineBinaryLocator {
    directories: Vec<PathBuf>,
}

impl EngineBinaryLocator {
    /// Builds a locator over explicit directories, de-duplicating them.
    #[must_use]
    pub fn new(directories: Vec<PathBuf>) -> Self {
        let mut unique = Vec::new();
        for directory in directories {
            if !unique.contains(&directory) {
                unique.push(directory);
            }
        }
        Self {
            directories: unique,
        }
    }

    /// Builds a locator for the installed Tauri application.
    ///
    /// Tauri places `externalBin` sidecars next to the main executable. On macOS the
    /// resource directory is `../Resources`; Linux bundles may also use
    /// `../lib/stackspirit`. Development builds use the same executable-relative name.
    #[must_use]
    pub fn for_current_exe() -> Self {
        let mut directories = Vec::new();
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
        {
            directories.push(directory.to_path_buf());
            directories.push(directory.join("binaries"));
            directories.push(directory.join("../Resources"));
            directories.push(directory.join("../Resources/binaries"));
            directories.push(directory.join("../lib/stackspirit"));
        }
        Self::new(directories)
    }

    /// Returns the searched directories in order.
    #[must_use]
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    /// Returns the first executable sidecar in the configured directories.
    #[must_use]
    pub fn locate(&self) -> Option<PathBuf> {
        let triple = current_target_triple();
        let mut names = Vec::new();
        if let Some(triple) = triple {
            names.push(sidecar_file_name(triple));
        } else if cfg!(windows) {
            names.push("opencode.exe".to_owned());
        } else {
            names.push("opencode".to_owned());
        }
        names.push(if cfg!(windows) {
            "opencode.exe".to_owned()
        } else {
            "opencode".to_owned()
        });
        for directory in &self.directories {
            for name in &names {
                let candidate = directory.join(name);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Reserves a free port bound only to loopback.
///
/// The listener is closed immediately before the child starts. The window is tiny and
/// the child is given the explicit port, so it can never bind a public interface.
pub fn reserve_loopback_port() -> Result<u16, SpiritError> {
    let listener = TcpListener::bind((LOOPBACK_HOST, 0))
        .map_err(|error| SpiritError::io("reserving a loopback port for the engine", &error))?;
    let port = listener
        .local_addr()
        .map_err(|error| SpiritError::io("reading the reserved engine port", &error))?
        .port();
    drop(listener);
    Ok(port)
}

/// Returns the exact arguments used to start the bundled engine.
#[must_use]
pub fn serve_args(port: u16) -> Vec<String> {
    vec![
        "serve".to_owned(),
        "--hostname".to_owned(),
        LOOPBACK_HOST.to_owned(),
        "--port".to_owned(),
        port.to_string(),
    ]
}

/// A bounded, redacted log ring.
#[derive(Debug)]
pub struct EngineLogBuffer {
    lines: VecDeque<String>,
    limit: usize,
}

impl Default for EngineLogBuffer {
    fn default() -> Self {
        Self::new(MAX_LOG_LINES)
    }
}

impl EngineLogBuffer {
    /// Builds a buffer capped at `limit` lines.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(limit.max(1)),
            limit: limit.max(1),
        }
    }

    /// Pushes one engine line after redaction and length bounding.
    pub fn push(&mut self, line: &str) {
        let redacted = redact(line);
        let bounded: String = redacted.chars().take(1_000).collect();
        if self.lines.len() == self.limit {
            self.lines.pop_front();
        }
        self.lines.push_back(bounded);
    }

    /// Returns a stable snapshot of the retained lines.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    /// Returns the number of retained lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Returns true when no line has been retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[derive(Debug)]
struct ProcessState {
    child: Option<Child>,
}

/// The bundled sidecar runtime backed by a real child process.
pub struct ProcessEngineRuntime {
    locator: EngineBinaryLocator,
    explicit_binary: Option<PathBuf>,
    state: Mutex<ProcessState>,
    logs: Arc<Mutex<EngineLogBuffer>>,
}

impl std::fmt::Debug for ProcessEngineRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessEngineRuntime")
            .field("binary", &self.explicit_binary)
            .field("directories", &self.locator.directories())
            .finish_non_exhaustive()
    }
}

impl ProcessEngineRuntime {
    /// Builds a runtime that resolves the sidecar from `locator`.
    #[must_use]
    pub fn new(locator: EngineBinaryLocator) -> Self {
        Self {
            locator,
            explicit_binary: None,
            state: Mutex::new(ProcessState { child: None }),
            logs: Arc::new(Mutex::new(EngineLogBuffer::default())),
        }
    }

    /// Builds a runtime around an exact binary. Used by tests and repair flows.
    #[must_use]
    pub fn with_binary(binary: PathBuf) -> Self {
        Self {
            locator: EngineBinaryLocator::default(),
            explicit_binary: Some(binary),
            state: Mutex::new(ProcessState { child: None }),
            logs: Arc::new(Mutex::new(EngineLogBuffer::default())),
        }
    }

    /// Builds a runtime for the installed application.
    #[must_use]
    pub fn for_current_exe() -> Self {
        Self::new(EngineBinaryLocator::for_current_exe())
    }

    fn resolve_binary(&self) -> Result<PathBuf, SpiritError> {
        if let Some(binary) = &self.explicit_binary {
            if is_executable(binary) {
                return Ok(binary.clone());
            }
            return Err(unavailable_error());
        }
        self.locator.locate().ok_or_else(unavailable_error)
    }

    fn http_get(&self, port: u16, path: &str) -> Result<HttpResponse, SpiritError> {
        http_get(port, path)
    }

    fn spawn_process(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError> {
        let binary = self.resolve_binary()?;
        let port = if port == 0 {
            reserve_loopback_port()?
        } else {
            port
        };
        let mut command = Command::new(&binary);
        command
            .args(serve_args(port))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            let detail = redact(&error.to_string());
            SpiritError::new(
                SpiritErrorCode::Engine,
                format!("StackSpirit Engine could not start: {detail}"),
            )
            .with_hint("Repair the installation and retry.")
        })?;
        let pid = child.id();
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, "stdout", self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, "stderr", self.logs.clone());
        }
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .child = Some(child);
        Ok(EngineRuntimeInfo {
            pid: Some(pid),
            port,
            version: None,
            binary_path: Some(binary),
        })
    }

    fn shutdown_blocking(&self) -> Result<(), SpiritError> {
        let child = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .child
            .take();
        if let Some(child) = child {
            terminate_child(child);
        }
        Ok(())
    }

    fn health(&self, port: u16) -> Result<EngineHealth, SpiritError> {
        let response = self.http_get(port, "/global/health")?;
        if !(200..300).contains(&response.status) {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine health returned HTTP {}", response.status),
            ));
        }
        let document: Value = serde_json::from_slice(&response.body).map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine health is not valid JSON: {error}"),
            )
        })?;
        if document.get("healthy").and_then(Value::as_bool) != Some(true) {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine health reported unhealthy",
            ));
        }
        Ok(EngineHealth {
            version: document
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    fn fetch_models(&self, port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        let mut last_error = None;
        for path in ["/provider", "/config/providers"] {
            match self.http_get(port, path) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return Ok(parse_models(&response.body));
                }
                Ok(response) => {
                    last_error = Some(SpiritError::new(
                        SpiritErrorCode::Engine,
                        format!("engine model list returned HTTP {}", response.status),
                    ));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            SpiritError::new(SpiritErrorCode::Engine, "engine model list is unavailable")
        }))
    }

    fn provider_ids(&self, port: u16) -> Result<Vec<String>, SpiritError> {
        let response = self.http_get(port, "/provider")?;
        if !(200..300).contains(&response.status) {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine provider list returned HTTP {}", response.status),
            ));
        }
        let document: Value = serde_json::from_slice(&response.body).map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                format!("engine provider list is not valid JSON: {error}"),
            )
        })?;
        let mut ids = Vec::new();
        collect_provider_ids(&document, &mut ids);
        Ok(ids)
    }
}

impl Drop for ProcessEngineRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_blocking();
    }
}

#[async_trait]
impl EngineRuntime for ProcessEngineRuntime {
    async fn spawn(&self, port: u16) -> Result<EngineRuntimeInfo, SpiritError> {
        self.spawn_process(port)
    }

    async fn probe_health(&self, info: &EngineRuntimeInfo) -> Result<EngineHealth, SpiritError> {
        self.health(info.port)
    }

    async fn shutdown(&self, _info: &EngineRuntimeInfo) -> Result<(), SpiritError> {
        self.shutdown_blocking()
    }

    async fn models(&self, port: u16) -> Result<Vec<RuntimeModel>, SpiritError> {
        self.fetch_models(port)
    }

    async fn verify_key(
        &self,
        port: u16,
        provider: &str,
        _secret: &SecretString,
    ) -> KeyVerification {
        // Never send the key to the engine: opencode's auth PUT persists credentials,
        // which would violate the keychain-only rule. A provider inventory read is the
        // strongest local check that does not move the secret.
        match self.provider_ids(port) {
            Ok(ids) if ids.iter().any(|id| id == provider) => KeyVerification::Unverified,
            Ok(_) => KeyVerification::Invalid,
            Err(_) => KeyVerification::Unverified,
        }
    }

    fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn kill_now(&self) {
        let _ = self.shutdown_blocking();
    }
}

fn unavailable_error() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Engine,
        "StackSpirit Engine is unavailable — repair the installation",
    )
    .with_hint("Reinstall StackSpirit to restore the bundled engine. StackSpirit does not use PATH for the bundled choice.")
}

fn spawn_log_reader<R: Read + Send + 'static>(
    reader: R,
    label: &'static str,
    logs: Arc<Mutex<EngineLogBuffer>>,
) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let line = if label == "stderr" {
                        format!("stderr: {line}")
                    } else {
                        line
                    };
                    logs.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(&line);
                }
                Err(_) => break,
            }
        }
    });
}

fn terminate_child(mut child: Child) {
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_get(port: u16, path: &str) -> Result<HttpResponse, SpiritError> {
    if !path.starts_with('/') {
        return Err(SpiritError::new(
            SpiritErrorCode::Internal,
            "engine HTTP path must start with '/'",
        ));
    }
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|error| SpiritError::io("connecting to the local engine", &error))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| SpiritError::io("setting the engine read timeout", &error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| SpiritError::io("setting the engine write timeout", &error))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {LOOPBACK_HOST}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| SpiritError::io("writing the engine request", &error))?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| SpiritError::io("reading the engine response", &error))?;
        if read == 0 {
            break;
        }
        if raw.len() + read > MAX_HTTP_BODY {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response exceeded the local size cap",
            ));
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse, SpiritError> {
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response has no HTTP headers",
            )
        })?;
    let headers = String::from_utf8_lossy(&raw[..boundary]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Engine,
                "engine response has no HTTP status",
            )
        })?;
    let body = &raw[boundary + 4..];
    let chunked = headers.lines().skip(1).any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok(HttpResponse { status, body })
}

fn decode_chunked(input: &[u8]) -> Result<Vec<u8>, SpiritError> {
    let mut output = Vec::new();
    let mut cursor = 0;
    loop {
        let line_end = input[cursor..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| {
                SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
            })?;
        let header = std::str::from_utf8(&input[cursor..cursor + line_end]).map_err(|_| {
            SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
        })?;
        let size = usize::from_str_radix(header.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| {
                SpiritError::new(SpiritErrorCode::Engine, "malformed chunked engine response")
            })?;
        cursor += line_end + 2;
        if size == 0 {
            break;
        }
        if cursor + size > input.len() {
            return Err(SpiritError::new(
                SpiritErrorCode::Engine,
                "truncated chunked engine response",
            ));
        }
        output.extend_from_slice(&input[cursor..cursor + size]);
        cursor += size;
        if input.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        }
    }
    Ok(output)
}

fn collect_provider_ids(document: &Value, ids: &mut Vec<String>) {
    let arrays = ["all", "providers"]
        .iter()
        .filter_map(|key| document.get(*key).and_then(Value::as_array));
    for providers in arrays {
        for provider in providers {
            if let Some(id) = provider.get("id").and_then(Value::as_str) {
                ids.push(id.to_owned());
            }
        }
    }
}

/// Parses `opencode` provider/model documents without depending on optional fields.
#[must_use]
pub fn parse_models(body: &[u8]) -> Vec<RuntimeModel> {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let mut models = Vec::new();
    for key in ["all", "providers"] {
        let Some(providers) = document.get(key).and_then(Value::as_array) else {
            continue;
        };
        for provider in providers {
            let Some(provider_id) = provider.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(entries) = provider.get("models") else {
                continue;
            };
            if let Some(map) = entries.as_object() {
                for (model_id, model) in map {
                    models.push(runtime_model(provider_id, model_id, model));
                }
            } else if let Some(array) = entries.as_array() {
                for model in array {
                    if let Some(model_id) = model.get("id").and_then(Value::as_str) {
                        models.push(runtime_model(provider_id, model_id, model));
                    }
                }
            }
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

fn runtime_model(provider_id: &str, model_id: &str, model: &Value) -> RuntimeModel {
    let id = if model_id.starts_with(&format!("{provider_id}/")) {
        model_id.to_owned()
    } else {
        format!("{provider_id}/{model_id}")
    };
    RuntimeModel {
        id,
        provider: provider_id.to_owned(),
        label: model
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(model_id)
            .to_owned(),
        context_tokens: model
            .get("limit")
            .and_then(|limit| limit.get("context"))
            .and_then(Value::as_u64),
        free: matches!(provider_id, "opencode" | "opencode-go") && input_cost(model) == Some(0.0),
    }
}

fn input_cost(model: &Value) -> Option<f64> {
    model
        .get("cost")
        .and_then(|cost| {
            cost.get("input")
                .or_else(|| cost.get("0").and_then(|tier| tier.get("input")))
        })
        .and_then(Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_locator_prefers_the_resource_dir_and_never_path() {
        let resource = tempfile::tempdir().unwrap();
        let path_dir = tempfile::tempdir().unwrap();
        let triple = current_target_triple().unwrap_or("x86_64-unknown-linux-gnu");
        let binaries = resource.path().join("binaries");
        std::fs::create_dir_all(&binaries).unwrap();
        let bundled = binaries.join(sidecar_file_name(triple));
        std::fs::write(&bundled, b"#!/bin/sh\n").unwrap();
        let decoy = path_dir.path().join("opencode");
        std::fs::write(&decoy, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let locator = EngineBinaryLocator::new(vec![resource.path().join("missing"), binaries]);
        assert_eq!(locator.locate(), Some(bundled));
        assert!(
            !locator
                .directories()
                .iter()
                .any(|dir| dir == path_dir.path())
        );
        assert_eq!(loopback_host(), "127.0.0.1");
    }

    #[test]
    fn engine_serve_args_are_loopback_only() {
        let args = serve_args(43117);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--hostname", "127.0.0.1"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--port", "43117"]));
        assert!(!args.iter().any(|arg| arg == "0.0.0.0"));
        let port = reserve_loopback_port().unwrap();
        assert!(port > 0);
    }

    #[test]
    fn engine_logs_are_bounded_and_redacted() {
        let mut buffer = EngineLogBuffer::new(3);
        let secret = ["sk", "abcdefgh12345678"].join("-");
        buffer.push(&format!("provider token {secret}"));
        buffer.push("one");
        buffer.push("two");
        buffer.push("three");
        buffer.push("four");
        assert_eq!(buffer.len(), 3);
        let snapshot = buffer.snapshot();
        assert!(!snapshot.join("\n").contains(&secret));
        assert!(!snapshot.iter().any(|line| line.contains("abcdefgh")));
        assert_eq!(snapshot.last().map(String::as_str), Some("four"));
    }

    #[test]
    fn engine_model_parser_marks_only_free_opencode_models_included() {
        let body = br#"{
          "all": [
            {"id": "opencode", "models": {
              "big-pickle": {"name": "Big Pickle", "cost": {"input": 0}, "limit": {"context": 200000}},
              "paid": {"name": "Paid", "cost": {"input": 1.5}}
            }},
            {"id": "openai", "models": {"free-looking": {"name": "Not included", "cost": {"input": 0}}}}
          ]
        }"#;
        let models = parse_models(body);
        assert_eq!(models.len(), 3);
        let pickle = models
            .iter()
            .find(|model| model.id == "opencode/big-pickle")
            .unwrap();
        assert!(pickle.free);
        assert_eq!(pickle.context_tokens, Some(200_000));
        let openai = models
            .iter()
            .find(|model| model.id == "openai/free-looking")
            .unwrap();
        assert!(!openai.free);
    }

    #[test]
    fn engine_http_parser_reads_chunked_responses() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello");
    }

    fn python_available() -> bool {
        Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    fn write_fake_engine(directory: &Path) -> PathBuf {
        let path = directory.join("opencode");
        let script = r#"#!/usr/bin/env python3
import http.server, json, sys
port = int(sys.argv[sys.argv.index("--port") + 1])
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/global/health":
            payload = {"healthy": True, "version": "1.19.5"}
        elif self.path == "/provider":
            payload = {"all": [{"id": "opencode", "models": {"big-pickle": {"name": "Big Pickle", "cost": {"input": 0}}}}], "default": {}, "connected": []}
        else:
            self.send_error(404)
            return
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args):
        pass
http.server.HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn process_alive(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_process_runtime_health_models_and_shutdown_have_no_orphan() {
        if !python_available() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let runtime = ProcessEngineRuntime::with_binary(binary);
        let info = runtime.spawn(0).await.unwrap();
        let pid = info.pid.unwrap();
        let mut health = None;
        for _ in 0..80 {
            if let Ok(result) = runtime.probe_health(&info).await {
                health = Some(result);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let health = health.expect("fake engine should become healthy");
        assert_eq!(health.version.as_deref(), Some("1.19.5"));
        let models = runtime.models(info.port).await.unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].free);
        runtime.shutdown(&info).await.unwrap();
        assert!(!process_alive(pid), "shutdown must reap the engine process");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_process_runtime_drop_kills_a_live_child() {
        if !python_available() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let binary = write_fake_engine(directory.path());
        let runtime = ProcessEngineRuntime::with_binary(binary);
        let info = runtime.spawn(0).await.unwrap();
        let pid = info.pid.unwrap();
        drop(runtime);
        assert!(!process_alive(pid), "app exit must leave no engine orphan");
    }
}
