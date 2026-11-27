//! Desktop harness commands: registry CRUD, probes, and the managed one-click connect.
//!
//! Every registry, probe, SSH and connect-plan rule lives in `spirit-core::harnesses`.
//! This module only:
//!
//! - resolves the saved custom directory and the saved Home VPS target,
//! - spawns the temporary loopback forward and the shell-free sign-in command,
//! - opens the browser URL the sign-in command prints, and
//! - keeps both child processes in shared state so they are torn down when the sign-in
//!   finishes or the app exits.
//!
//! No command returns a local path, an SSH option, a secret value or raw harness output.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::harnesses::{
    HarnessBinaryState, HarnessListView, HarnessManifest, HarnessProbeReport, HarnessProbeTarget,
    HarnessRegistry, HarnessTargetKind, build_connect_plan, probe_harness,
};
use spirit_core::ssh::command::CommandSpec;
use tauri::State;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use crate::error::CommandResult;
use crate::state::AppState;

const FORWARD_READY: Duration = Duration::from_secs(8);
const URL_WAIT: Duration = Duration::from_secs(15);
const MONITOR_INTERVAL: Duration = Duration::from_millis(250);

/// One probed harness. The renderer sends only the id and the target.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessProbeInput {
    /// Manifest id.
    pub id: String,
    /// Local or Home VPS.
    pub target: HarnessTargetKind,
}

/// One one-click connect request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConnectInput {
    /// Manifest id.
    pub id: String,
    /// Local or Home VPS.
    pub target: HarnessTargetKind,
}

/// What the renderer receives after a connect starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConnectStarted {
    /// Manifest id.
    pub harness_id: String,
    /// Target where the sign-in runs.
    pub target: HarnessTargetKind,
    /// True when a sign-in command was spawned.
    pub auth_started: bool,
    /// True when StackSpirit opened a browser URL or the documentation.
    pub browser_opened: bool,
    /// The temporary loopback port, when a forward is running.
    pub forward_port: Option<u16>,
    /// Safe explanation for the owner.
    pub message: String,
}

/// The app-wide registry of running harness connects.
#[derive(Debug, Default)]
pub struct HarnessConnects {
    entries: Arc<Mutex<HashMap<String, ManagedConnect>>>,
}

impl HarnessConnects {
    fn contains(&self, key: &str) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(key)
    }

    fn insert(
        &self,
        key: String,
        forward: Option<Child>,
        auth: Option<Child>,
    ) -> CommandResult<()> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.contains_key(&key) {
            return Err(SpiritError::new(
                SpiritErrorCode::Conflict,
                "a connect is already running for this harness and target",
            )
            .with_hint("Finish or close the current sign-in, then retry."));
        }
        entries.insert(key, ManagedConnect { forward, auth });
        Ok(())
    }

    /// Stops every managed forward and sign-in. Safe to call more than once.
    pub fn stop_all(&self) {
        let drained = {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            entries.drain().map(|(_, entry)| entry).collect::<Vec<_>>()
        };
        drop(drained);
    }

    fn spawn_monitor(&self, key: String) {
        let entries = self.entries.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(MONITOR_INTERVAL).await;
                let mut map = entries
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(entry) = map.get_mut(&key) else {
                    return;
                };
                let auth_done = entry
                    .auth
                    .as_mut()
                    .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
                let forward_done = entry
                    .forward
                    .as_mut()
                    .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
                if auth_done || forward_done {
                    map.remove(&key);
                    return;
                }
            }
        });
    }
}

#[derive(Debug)]
struct ManagedConnect {
    forward: Option<Child>,
    auth: Option<Child>,
}

impl ManagedConnect {
    fn stop(&mut self) {
        if let Some(child) = &mut self.forward {
            let _ = child.start_kill();
        }
        if let Some(child) = &mut self.auth {
            let _ = child.start_kill();
        }
    }
}

impl Drop for ManagedConnect {
    fn drop(&mut self) {
        self.stop();
    }
}

#[tauri::command]
pub fn harnesses_list() -> CommandResult<HarnessListView> {
    HarnessRegistry::for_home()?.list()
}

#[tauri::command]
pub fn harnesses_save_custom(manifest: HarnessManifest) -> CommandResult<HarnessManifest> {
    HarnessRegistry::for_home()?.save_custom(manifest)
}

#[tauri::command]
pub fn harnesses_delete_custom(id: String) -> CommandResult<bool> {
    HarnessRegistry::for_home()?.delete_custom(&id)
}

#[tauri::command]
pub async fn harnesses_probe(
    state: State<'_, AppState>,
    input: HarnessProbeInput,
) -> CommandResult<HarnessProbeReport> {
    let registry = HarnessRegistry::for_home()?;
    let manifest = find_manifest(&registry, &input.id)?;
    let target = probe_target(state.inner(), input.target)?;
    let probe = probe_harness(&manifest, &target, state.runner()).await;
    let plan = build_connect_plan(&manifest, &target, &probe)?;
    Ok(HarnessProbeReport {
        probe,
        plan: plan.view(),
    })
}

#[tauri::command]
pub async fn harnesses_connect_start(
    state: State<'_, AppState>,
    connects: State<'_, HarnessConnects>,
    input: HarnessConnectInput,
) -> CommandResult<HarnessConnectStarted> {
    let registry = HarnessRegistry::for_home()?;
    let manifest = find_manifest(&registry, &input.id)?;
    let target = probe_target(state.inner(), input.target)?;
    let probe = probe_harness(&manifest, &target, state.runner()).await;
    let plan = build_connect_plan(&manifest, &target, &probe)?;

    if matches!(probe.binary, HarnessBinaryState::Missing) {
        let hint = plan.install_command.as_ref().map_or_else(
            || format!("Install {} on the target and retry.", manifest.binary),
            |command| format!("Install it on the target, then retry: {command}"),
        );
        return Err(SpiritError::new(
            SpiritErrorCode::Refused,
            format!(
                "{} is not installed on {}",
                manifest.binary,
                target.kind().label()
            ),
        )
        .with_hint(hint));
    }

    let key = format!("{}:{}", manifest.id, target.kind().as_str());
    if connects.contains(&key) {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            "a connect is already running for this harness and target",
        )
        .with_hint("Finish or close the current sign-in, then retry."));
    }

    // Start the forward only for a Home VPS sign-in that uses a loopback callback.
    // `build_connect_plan` already requires a sign-in command before it emits a plan.
    let mut forward_child = None;
    if let Some(forward) = &plan.forward {
        let mut child = spawn_spec(&forward.spec, false)?;
        wait_for_forward(&mut child, forward.port).await?;
        forward_child = Some(child);
    }

    let mut auth_child = None;
    let mut browser_opened = false;
    let mut auth_started = false;
    let message;
    if let Some(spec) = &plan.auth_spec {
        let mut child = spawn_spec(spec, true)?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let url = if let (Some(stdout), Some(stderr)) = (stdout, stderr) {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            tokio::spawn(drain_for_url(stdout, stderr, sender));
            tokio::time::timeout(URL_WAIT, receiver.recv())
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        if let Some(url) = url {
            browser_opened = open_url(&url);
        }
        auth_started = true;
        let command = plan
            .auth_command
            .as_deref()
            .unwrap_or("the harness sign-in");
        message = if browser_opened {
            format!("Started {command}. Opened the sign-in in your browser.")
        } else {
            format!(
                "Started {command}. It did not print a browser URL yet; finish the sign-in in \
                 the harness."
            )
        };
        auth_child = Some(child);
    } else {
        browser_opened = open_url(&manifest.docs_url);
        message = format!(
            "{} signs in inside its own terminal. Opened its documentation.",
            manifest.name
        );
    }

    let forward_port = plan.forward.as_ref().map(|forward| forward.port);
    if forward_child.is_some() || auth_child.is_some() {
        connects.insert(key.clone(), forward_child, auth_child)?;
        connects.spawn_monitor(key);
    }

    Ok(HarnessConnectStarted {
        harness_id: manifest.id,
        target: target.kind(),
        auth_started,
        browser_opened,
        forward_port,
        message,
    })
}

fn find_manifest(registry: &HarnessRegistry, id: &str) -> CommandResult<HarnessManifest> {
    registry
        .get(id)?
        .ok_or_else(|| SpiritError::not_found(&format!("harness {id}")))
}

fn probe_target(state: &AppState, target: HarnessTargetKind) -> CommandResult<HarnessProbeTarget> {
    match target {
        HarnessTargetKind::Local => Ok(HarnessProbeTarget::Local),
        HarnessTargetKind::HomeVps => {
            let host = state.active_host().ok_or_else(|| {
                SpiritError::new(
                    SpiritErrorCode::Conflict,
                    "connect to a Home VPS before probing remote harnesses",
                )
                .with_hint("Use the sidebar to connect a saved server, then retry.")
            })?;
            Ok(HarnessProbeTarget::HomeVps {
                target: host.ssh.clone(),
                options: state.ssh_options(host.identity_file.clone()),
            })
        }
    }
}

fn spawn_spec(spec: &CommandSpec, capture: bool) -> CommandResult<Child> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true);
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    command
        .spawn()
        .map_err(|error| SpiritError::io(&format!("starting {}", spec.program), &error))
}

async fn wait_for_forward(child: &mut Child, port: u16) -> CommandResult<()> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = tokio::time::Instant::now() + FORWARD_READY;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(SpiritError::new(
                SpiritErrorCode::Remote,
                format!("the loopback forward exited before it opened port {port} ({status})"),
            )
            .with_hint("Check the SSH connection and whether the local port is already in use."));
        }
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(150)).is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    let _ = child.start_kill();
    Err(SpiritError::new(
        SpiritErrorCode::RemoteTimeout,
        format!("the loopback forward did not open 127.0.0.1:{port}"),
    )
    .retryable()
    .with_hint("Check the SSH connection and retry; StackSpirit never asks for a manual tunnel."))
}

/// Reads both child streams, sends the first browser URL once, and keeps draining so a
/// full pipe can never block the harness sign-in.
async fn drain_for_url<R1, R2>(stdout: R1, stderr: R2, sender: tokio::sync::mpsc::Sender<String>)
where
    R1: AsyncRead + Unpin,
    R2: AsyncRead + Unpin,
{
    let mut stdout = BufReader::new(stdout).lines();
    let mut stderr = BufReader::new(stderr).lines();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut sent = false;
    while stdout_open || stderr_open {
        let line = tokio::select! {
            result = stdout.next_line(), if stdout_open => match result {
                Ok(Some(line)) => Some(line),
                Ok(None) | Err(_) => {
                    stdout_open = false;
                    None
                }
            },
            result = stderr.next_line(), if stderr_open => match result {
                Ok(Some(line)) => Some(line),
                Ok(None) | Err(_) => {
                    stderr_open = false;
                    None
                }
            },
        };
        if let Some(line) = line
            && !sent
            && let Some(url) = find_url(&line)
        {
            sent = true;
            let _ = sender.send(url).await;
        }
    }
}

/// Extracts the first http(s) URL from one output line, without interpreting the rest.
fn find_url(line: &str) -> Option<String> {
    for raw in line.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            matches!(
                c,
                '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '<' | '>' | ',' | ';'
            )
        });
        if (token.starts_with("https://") || token.starts_with("http://"))
            && token.len() > "https://".len()
        {
            return Some(
                token
                    .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '"', '\''])
                    .to_owned(),
            );
        }
    }
    None
}

/// Opens one http(s) URL with the OS handler. A non-http scheme is refused.
fn open_url(url: &str) -> bool {
    if !(url.starts_with("https://")
        || url.starts_with("http://127.0.0.1:")
        || url.starts_with("http://localhost:")
        || url.starts_with("http://[::1]:"))
    {
        return false;
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_browser_url_and_ignores_punctuation() {
        assert_eq!(
            find_url("Open this URL to continue: https://claude.ai/oauth?code=abc)."),
            Some("https://claude.ai/oauth?code=abc".to_owned())
        );
        assert_eq!(
            find_url("callback: http://127.0.0.1:54545/callback"),
            Some("http://127.0.0.1:54545/callback".to_owned())
        );
        assert_eq!(find_url("no link here"), None);
        assert_eq!(find_url("ftp://example.com"), None);
    }

    #[test]
    fn open_url_refuses_a_non_http_scheme() {
        assert!(!open_url("file:///etc/passwd"));
        assert!(!open_url("javascript:alert(1)"));
        assert!(!open_url("http://example.com"));
    }
}
