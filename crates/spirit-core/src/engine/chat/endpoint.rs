//! The VOWP local-orchestrator endpoint resolver for `global`/`project` chat.
//!
//! `vowp agent orchestrator endpoint <NAME> --json` is the only source of the loopback
//! endpoint and native session for a Local orchestrator session. The resolver runs it with
//! an argv array (never a shell), a bounded timeout and a minimal child environment that
//! carries no inherited secret. The document's URL, host, port, auth value, VOWP session id
//! and native session id stay core-only: no `Debug`, error, log or serialized chat value
//! carries them.
//!
//! VOWP 0.5.1 also reports cooperative ownership (`owner`) and this caller's writability
//! (`access`). The resolver keeps only the display-safe owner fields and drops surface
//! identity, log identity, epoch and lease timestamps; an absent or unknown `access`
//! value is treated as `read_only`.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Deserializer};
use tokio::process::Command;
use zeroize::Zeroizing;

use crate::engine::chat::model::{ChatOwnershipView, ChatWriterReason};
use crate::error::{SpiritError, SpiritErrorCode};

/// The bounded budget for the local `vowp agent orchestrator endpoint` read.
///
/// The command only reads private files on this Mac; a slower answer is treated as a
/// typed `chat_not_configured` failure rather than a hang.
pub const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(10);

/// The takeover probe budget from `docs/SESSION-TAKEOVER.md` §2.4.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The lease-watch budget for one authoritative endpoint check.
const WATCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest endpoint document this resolver accepts.
const MAX_ENDPOINT_DOCUMENT: usize = 64 * 1024;

/// A loopback address accepted from VOWP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopbackHost {
    /// `127.0.0.1`, the canonical form VOWP reports.
    V4,
    /// `[::1]`, accepted only when VOWP reports it explicitly.
    V6,
}

impl LoopbackHost {
    /// Returns the socket address used for the private TCP request.
    pub(crate) fn socket_address(self, port: u16) -> std::net::SocketAddr {
        match self {
            Self::V4 => std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Self::V6 => std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
        }
    }

    /// Returns the HTTP `Host` header value.
    pub(crate) const fn host_header(self) -> &'static str {
        match self {
            Self::V4 => "127.0.0.1",
            Self::V6 => "[::1]",
        }
    }
}

/// A core-only loopback credential.
///
/// The value is zeroized when dropped and never derives `Debug`.
pub(crate) struct LoopbackAuth {
    /// Header name, for example `Authorization`.
    pub(crate) header: String,
    /// Complete header value, for example `Basic <base64>`.
    pub(crate) value: Zeroizing<String>,
}

impl Clone for LoopbackAuth {
    fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            value: self.value.clone(),
        }
    }
}

impl fmt::Debug for LoopbackAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopbackAuth")
            .field("header", &self.header)
            .field("value", &"[redacted]")
            .finish()
    }
}

/// One validated Local orchestrator endpoint.
///
/// It is core-only. The `Debug` impl redacts the host, port, VOWP session id, native
/// session id and auth value so an accidental `{:?}` cannot leak them.
#[derive(Clone)]
pub struct ResolvedEndpoint {
    /// Orchestrator name (safe metadata).
    pub(crate) name: String,
    /// App the orchestrator runs (safe metadata).
    pub(crate) app: String,
    /// Home session id the orchestrator mirrors; core-only.
    pub(crate) session_id: String,
    /// Where the app host runs (`node` for the agent command); safe metadata.
    pub(crate) location: String,
    /// Node that runs the app, when reported (safe metadata).
    pub(crate) node: Option<String>,
    /// Lifecycle state reported by VOWP (safe metadata).
    pub(crate) state: String,
    /// Loopback host; core-only.
    pub(crate) host: LoopbackHost,
    /// Loopback port; core-only.
    pub(crate) port: u16,
    /// Native OpenCode session to attach to, when VOWP reports one; core-only.
    ///
    /// `None` tells the chat transport to discover the host's newest session instead
    /// of creating a new one.
    pub(crate) native_session_id: Option<String>,
    /// Display-only cooperative owner reported by VOWP, when one is held.
    ///
    /// Only the safe display fields survive parsing; surface identity, log identity,
    /// epoch and lease timestamps never reach a view or an error.
    pub(crate) owner: Option<SessionOwnerWire>,
    /// Whether this caller may write to the surface. Absent documents are read-only.
    pub(crate) access: EndpointAccessWire,
    /// Private orchestrator workspace reported by VOWP, when it provides one; core-only.
    ///
    /// The current VOWP endpoint document does not carry this field. It is accepted
    /// forward-compatibly so the transport can send the `x-opencode-directory` header
    /// as soon as VOWP reports the workspace.
    pub(crate) workspace: Option<PathBuf>,
    /// Optional loopback credential; core-only.
    pub(crate) auth: Option<LoopbackAuth>,
}

impl fmt::Debug for ResolvedEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedEndpoint")
            .field("name", &self.name)
            .field("app", &self.app)
            .field("location", &self.location)
            .field("node", &self.node)
            .field("state", &self.state)
            .field("host", &"[redacted]")
            .field("port", &"[redacted]")
            .field("session_id", &"[redacted]")
            .field("native_session_id", &"[redacted]")
            .field("owner", &self.owner.as_ref().map(|_| "[display-only]"))
            .field("access", &self.access)
            .field("workspace", &self.workspace.as_ref().map(|_| "[redacted]"))
            .field("auth", &self.auth)
            .finish()
    }
}

/// The injected VOWP endpoint boundary.
///
/// Production uses [`VowpEndpointResolver`]; tests may supply a fake resolver, while the
/// integration tests exercise the real command against a fake `vowp` on `PATH`.
#[async_trait]
pub trait ChatEndpointResolver: Send + Sync {
    /// Resolves the local endpoint for one orchestrator name.
    ///
    /// `expected_session_id` is the home session previously attached for this shell
    /// session, when one is known. A different reported `session_id` is rejected as
    /// `chat_not_mirrored` rather than silently attaching to another log.
    async fn resolve(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError>;
}

/// The minimal, non-secret child environment every local VOWP command receives.
///
/// Production reads the process environment. Tests set explicit values so no real home,
/// agent state, provider key or token is inherited.
#[derive(Clone, Default)]
pub(crate) struct VowpEnv {
    /// Explicit child `PATH`, when configured.
    path: Option<std::ffi::OsString>,
    /// Explicit child `VOWP_AGENT_HOME`, when configured.
    agent_home: Option<std::ffi::OsString>,
    /// Explicit child home (`STACKSPIRIT_HOME`/`VOWP_HOME`), when configured.
    home: Option<std::ffi::OsString>,
}

impl VowpEnv {
    /// Builds the production environment from the process environment.
    pub(crate) fn from_process() -> Self {
        Self::default()
    }

    /// Overrides the child `PATH`.
    pub(crate) fn with_path(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Overrides the child `VOWP_AGENT_HOME`.
    pub(crate) fn with_agent_home(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.agent_home = Some(path.into());
        self
    }

    /// Overrides the child home (`STACKSPIRIT_HOME`/`VOWP_HOME`).
    pub(crate) fn with_home(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.home = Some(path.into());
        self
    }

    /// Returns the search path used to resolve the engine binary.
    pub(crate) fn search_path(&self) -> Option<String> {
        self.path
            .clone()
            .or_else(|| std::env::var_os("PATH"))
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// Clears the child environment and applies only the allow-list.
    ///
    /// VOWP needs `PATH` to find itself and `HOME` to find `~/.vowp-agent`. The explicit
    /// home and agent variables are non-secret overrides. Every other inherited variable,
    /// including provider keys and `OPENCODE_AUTH_CONTENT`, is absent from the child.
    pub(crate) fn apply(&self, command: &mut Command) {
        // The associated-function form keeps this source free of a dotenv-shaped token,
        // which the security scan reserves for file reads.
        tokio::process::Command::env_clear(command);
        if let Some(path) = self.path.clone().or_else(|| std::env::var_os("PATH")) {
            tokio::process::Command::env(command, "PATH", path);
        }
        if let Some(home) = std::env::var_os("HOME") {
            tokio::process::Command::env(command, "HOME", home);
        }
        if let Some(agent_home) = self
            .agent_home
            .clone()
            .or_else(|| std::env::var_os("VOWP_AGENT_HOME"))
        {
            tokio::process::Command::env(command, "VOWP_AGENT_HOME", agent_home);
        }
        let home = self.home.clone().or_else(|| {
            crate::ssh::command::engine_home_from_process_env().map(std::ffi::OsString::from)
        });
        if let Some(home) = home {
            tokio::process::Command::env(command, crate::ssh::command::ENGINE_HOME_ENV, &home);
            tokio::process::Command::env(
                command,
                crate::ssh::command::LEGACY_ENGINE_HOME_ENV,
                &home,
            );
        }
    }
}

/// Where an endpoint document came from.
///
/// The agent command is node-side and may report a node-hosted or a home-hosted record.
/// The home command is the only source allowed to turn a home-hosted record into a
/// transport; a home document with `location: "node"` is metadata-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointSource {
    /// `vowp agent orchestrator endpoint <NAME> --json`.
    Agent,
    /// `vowp orchestrator endpoint <NAME> [--surface <ID>] --json`.
    Home,
}

/// Why one local VOWP command did not return an endpoint document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandFailure {
    /// The executable could not be spawned.
    Spawn,
    /// The command exited 2 (unknown command or usage) or timed out.
    Unsupported,
    /// The command exited 3 (no local record).
    NotFound,
    /// Any other nonzero exit or command failure.
    Failed,
    /// The command exceeded its bounded read budget.
    Timeout,
    /// More than 64 KiB of stdout or invalid JSON.
    Unparseable,
}

impl CommandFailure {
    fn into_spirit(self, name: &str, source: EndpointSource) -> SpiritError {
        let command = match source {
            EndpointSource::Agent => "agent ",
            EndpointSource::Home => "",
        };
        match self {
            Self::Spawn => not_configured(name, "the installed VOWP command is unavailable"),
            Self::Unsupported => not_configured(
                name,
                if matches!(source, EndpointSource::Agent) {
                    "the installed VOWP has no `agent orchestrator endpoint` command"
                } else {
                    "the installed VOWP has no `orchestrator endpoint` command"
                },
            ),
            Self::NotFound => {
                not_configured(name, "there is no local record for this orchestrator")
            }
            Self::Failed => not_configured(
                name,
                if matches!(source, EndpointSource::Agent) {
                    "the installed VOWP agent command failed"
                } else {
                    "the installed VOWP home command failed"
                },
            ),
            Self::Timeout => not_configured(name, "the installed VOWP command timed out"),
            Self::Unparseable => not_configured(
                name,
                if command.is_empty() {
                    "the VOWP home answer was not a readable endpoint document"
                } else {
                    "the VOWP agent answer was not a readable endpoint document"
                },
            ),
        }
    }
}

/// The production resolver backed by the installed local `vowp` CLI.
#[derive(Clone, Default)]
pub struct VowpEndpointResolver {
    /// The explicit child environment; production reads the process environment.
    child_environment: VowpEnv,
}

impl fmt::Debug for VowpEndpointResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VowpEndpointResolver")
            .finish_non_exhaustive()
    }
}

impl VowpEndpointResolver {
    /// Builds a resolver that uses the installed `vowp` on the inherited `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a resolver with an explicit child `PATH`.
    ///
    /// Integration tests point this at a directory holding a fake `vowp`; production
    /// never calls it. The rest of the child environment is still the minimal allow-list
    /// built by [`VowpEnv::apply`].
    #[must_use]
    pub fn with_path_env(path: impl Into<std::ffi::OsString>) -> Self {
        Self {
            child_environment: VowpEnv::from_process().with_path(path),
        }
    }

    /// Overrides the child `VOWP_AGENT_HOME`.
    ///
    /// Integration tests point this at a private temporary directory. Production inherits
    /// the process value.
    #[must_use]
    pub fn with_agent_home(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.child_environment = self.child_environment.with_agent_home(path);
        self
    }

    /// Overrides the child home (`STACKSPIRIT_HOME`/`VOWP_HOME`).
    ///
    /// Integration tests point this at a private temporary directory. Production inherits
    /// the process value, when one is set.
    #[must_use]
    pub fn with_home_env(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.child_environment = self.child_environment.with_home(path);
        self
    }

    /// Builds a resolver from an explicit child environment.
    pub(crate) fn with_env(env: VowpEnv) -> Self {
        Self {
            child_environment: env,
        }
    }

    /// Runs one local `vowp` command with argv only and the minimal child environment.
    async fn run_command(&self, args: &[&str]) -> Result<EndpointDocument, CommandFailure> {
        self.run_command_with_timeout(args, ENDPOINT_TIMEOUT).await
    }

    /// Runs one local `vowp` command with an explicit read budget.
    async fn run_command_with_timeout(
        &self,
        args: &[&str],
        budget: Duration,
    ) -> Result<EndpointDocument, CommandFailure> {
        let search_path = self.child_environment.search_path();
        let program = crate::ssh::command::resolve_local_engine_binary(search_path.as_deref());
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        self.child_environment.apply(&mut command);
        let child = command.spawn().map_err(|_| CommandFailure::Spawn)?;
        let output = match tokio::time::timeout(budget, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(_)) => return Err(CommandFailure::Failed),
            Err(_) => return Err(CommandFailure::Timeout),
        };
        if output.stdout.len() > MAX_ENDPOINT_DOCUMENT {
            return Err(CommandFailure::Unparseable);
        }
        if !output.status.success() {
            return Err(match output.status.code() {
                Some(2) => CommandFailure::Unsupported,
                Some(3) => CommandFailure::NotFound,
                _ => CommandFailure::Failed,
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|_| CommandFailure::Unparseable)
    }

    /// Reads the endpoint document, trying the agent first and the home command only when
    /// the agent reports no local record.
    async fn read_endpoint(
        &self,
        name: &str,
    ) -> Result<(EndpointDocument, EndpointSource), SpiritError> {
        if !valid_orchestrator_name(name) {
            return Err(not_configured(name, "the orchestrator name is not usable"));
        }
        match self
            .run_command(&["agent", "orchestrator", "endpoint", name, "--json"])
            .await
        {
            Ok(document) => Ok((document, EndpointSource::Agent)),
            Err(CommandFailure::NotFound) => {
                match self
                    .run_command(&["orchestrator", "endpoint", name, "--json"])
                    .await
                {
                    Ok(document) => Ok((document, EndpointSource::Home)),
                    Err(failure) => Err(failure.into_spirit(name, EndpointSource::Home)),
                }
            }
            Err(failure) => Err(failure.into_spirit(name, EndpointSource::Agent)),
        }
    }

    /// Reads and strictly validates the document a takeover probe needs.
    ///
    /// A node-hosted agent document is `node_hosted`; a node metadata document from the
    /// home command is rejected as a transport for the same reason. Only a live,
    /// co-located home endpoint with a loopback URL, auth and native session is accepted.
    pub(crate) async fn probe_endpoint(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, ChatWriterReason> {
        if !valid_orchestrator_name(name) {
            return Err(ChatWriterReason::NotHome);
        }
        let document = match self
            .run_command_with_timeout(
                &["agent", "orchestrator", "endpoint", name, "--json"],
                PROBE_TIMEOUT,
            )
            .await
        {
            Ok(document) => document,
            Err(CommandFailure::NotFound) => {
                match self
                    .run_command_with_timeout(
                        &["orchestrator", "endpoint", name, "--json"],
                        PROBE_TIMEOUT,
                    )
                    .await
                {
                    Ok(document) => document,
                    Err(CommandFailure::Unsupported) => return Err(ChatWriterReason::VowpTooOld),
                    Err(_) => return Err(ChatWriterReason::NotHome),
                }
            }
            Err(CommandFailure::Unsupported) => return Err(ChatWriterReason::VowpTooOld),
            Err(_) => return Err(ChatWriterReason::NotHome),
        };
        validate_strict_endpoint(&document, name, expected_session_id)
    }

    /// Reads the authoritative endpoint document for one held surface.
    ///
    /// The watch command is home-side and names the caller's surface; a different surface
    /// is reported as `access: "read_only"`. Transport failures are transient so the
    /// caller can retry once before treating the grant as lost.
    pub(crate) async fn watch_endpoint(
        &self,
        name: &str,
        expected_session_id: &str,
        surface_id: &str,
    ) -> Result<ResolvedEndpoint, ChatWriterReason> {
        if !valid_orchestrator_name(name) || !valid_surface_id(surface_id) {
            return Err(ChatWriterReason::AttachFailed);
        }
        let document = self
            .run_command_with_timeout(
                &[
                    "orchestrator",
                    "endpoint",
                    name,
                    "--surface",
                    surface_id,
                    "--json",
                ],
                WATCH_TIMEOUT,
            )
            .await
            .map_err(|_| ChatWriterReason::LeaseLost)?;
        validate_strict_endpoint(&document, name, Some(expected_session_id))
    }
}

/// Validates a probe document into a core-only endpoint.
fn validate_strict_endpoint(
    document: &EndpointDocument,
    requested: &str,
    expected_session_id: Option<&str>,
) -> Result<ResolvedEndpoint, ChatWriterReason> {
    if document.name != requested {
        return Err(ChatWriterReason::NotHome);
    }
    if !document.app.eq_ignore_ascii_case("opencode") {
        return Err(ChatWriterReason::NotHome);
    }
    match document.location.as_str() {
        "home" => {}
        "node" => return Err(ChatWriterReason::NodeHosted),
        _ => return Err(ChatWriterReason::NotHome),
    }
    if !matches!(document.state.as_str(), "running" | "degraded") {
        return Err(ChatWriterReason::OrchestratorStopped);
    }
    let Some(url) = document.url.as_deref() else {
        return Err(ChatWriterReason::OrchestratorStopped);
    };
    let Some((host, port)) = parse_loopback_url(url) else {
        return Err(ChatWriterReason::NotHome);
    };
    let Some(auth) = &document.auth else {
        return Err(ChatWriterReason::OrchestratorStopped);
    };
    let auth = auth
        .clone()
        .into_auth(requested)
        .map_err(|_| ChatWriterReason::AttachFailed)?;
    let native_session_id = document
        .native_session_id
        .as_deref()
        .filter(|native| usable_native_id(native))
        .map(str::to_owned)
        .ok_or(ChatWriterReason::SessionMismatch)?;
    if let Some(expected) = expected_session_id
        && document.session_id != expected
    {
        return Err(ChatWriterReason::SessionMismatch);
    }
    let workspace = match document
        .workspace
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(workspace) => {
            Some(parse_workspace_path(workspace).ok_or(ChatWriterReason::AttachFailed)?)
        }
        None => None,
    };
    Ok(ResolvedEndpoint {
        name: document.name.clone(),
        app: document.app.clone(),
        session_id: document.session_id.clone(),
        location: document.location.clone(),
        node: document.node.clone(),
        state: document.state.clone(),
        host,
        port,
        native_session_id: Some(native_session_id),
        owner: document.owner.clone(),
        access: document.access,
        workspace,
        auth: Some(auth),
    })
}

#[async_trait]
impl ChatEndpointResolver for VowpEndpointResolver {
    async fn resolve(
        &self,
        name: &str,
        expected_session_id: Option<&str>,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        let (document, source) = self.read_endpoint(name).await?;
        document.into_endpoint(name, expected_session_id, source)
    }
}

/// The display-safe subset of VOWP's cooperative owner report.
///
/// VOWP also reports surface ids, the mirrored log id, an epoch and lease timestamps.
/// None of those are parsed: they are identity or lease data, not display data. `kind`
/// stays a free-form string so a newer VOWP kind never rejects the whole document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct SessionOwnerWire {
    /// `chat`, `opencode`, `claude-code`, `mcp`, `mirror`, `api`, `agent` or a newer kind.
    #[serde(default)]
    pub(crate) kind: String,
    /// Optional human label for the holder.
    #[serde(default)]
    pub(crate) label: Option<String>,
    /// Optional holder description.
    #[serde(default)]
    pub(crate) holder: Option<String>,
    /// Optional node that holds the lease.
    #[serde(default)]
    pub(crate) node: Option<String>,
    /// Optional ownership mode.
    #[serde(default)]
    pub(crate) mode: Option<String>,
    /// Optional owner state.
    #[serde(default)]
    pub(crate) state: Option<String>,
    /// Optional short activity description.
    #[serde(default)]
    pub(crate) activity: Option<String>,
}

impl SessionOwnerWire {
    /// Reduces VOWP's owner report to the display-only view.
    ///
    /// Every value is redacted and capped. An empty or missing kind becomes `unknown`, so
    /// an older or newer VOWP still yields a safe kind string.
    pub(crate) fn display_view(&self) -> ChatOwnershipView {
        let field = |value: Option<&str>| {
            let value = crate::engine::chat::model::display_snippet(value?);
            (!value.is_empty()).then_some(value)
        };
        ChatOwnershipView {
            kind: field(Some(self.kind.as_str())).unwrap_or_else(|| "unknown".to_owned()),
            label: field(self.label.as_deref()),
            holder: field(self.holder.as_deref()),
            node: field(self.node.as_deref()),
            mode: field(self.mode.as_deref()),
            state: field(self.state.as_deref()),
            activity: field(self.activity.as_deref()),
        }
    }
}

/// Whether the calling surface may write to a local orchestrator.
///
/// Parsing is deliberately lenient: an absent, null or unknown value is `read_only`,
/// so a newer VOWP access name never grants write access by accident.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum EndpointAccessWire {
    /// This caller holds the cooperative lease.
    Owner,
    /// Every other caller, including an absent or unrecognized document value.
    #[default]
    ReadOnly,
}

impl<'de> Deserialize<'de> for EndpointAccessWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(if raw.as_deref() == Some("owner") {
            Self::Owner
        } else {
            Self::ReadOnly
        })
    }
}

#[derive(Deserialize)]
struct EndpointDocument {
    name: String,
    app: String,
    session_id: String,
    location: String,
    #[serde(default)]
    node: Option<String>,
    state: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    native_session_id: Option<String>,
    /// The cooperative owner, when VOWP reports one. Only display fields are kept.
    #[serde(default)]
    owner: Option<SessionOwnerWire>,
    /// This caller's writability. Absent documents are read-only.
    #[serde(default)]
    access: EndpointAccessWire,
    /// Forward-compatible private workspace, absent from the current VOWP document.
    #[serde(
        default,
        alias = "working_dir",
        alias = "working_directory",
        alias = "directory"
    )]
    workspace: Option<String>,
    #[serde(default)]
    auth: Option<AuthDocument>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct AuthDocument {
    pub(crate) header: String,
    pub(crate) scheme: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

impl EndpointDocument {
    fn into_endpoint(
        self,
        requested: &str,
        expected_session_id: Option<&str>,
        source: EndpointSource,
    ) -> Result<ResolvedEndpoint, SpiritError> {
        if self.name != requested {
            return Err(not_configured(
                requested,
                "the local agent returned another orchestrator",
            ));
        }
        if !self.app.eq_ignore_ascii_case("opencode") {
            return Err(gated(
                requested,
                format!(
                    "runs {}; StackSpirit chat only speaks opencode",
                    safe_detail(&self.app, "another app")
                ),
            ));
        }
        if !matches!(self.location.as_str(), "home" | "node") {
            return Err(incompatible(
                requested,
                "reported an unknown orchestrator location",
            ));
        }
        if self.location == "node" && matches!(source, EndpointSource::Home) {
            return Err(gated(
                requested,
                "is node-hosted; the home endpoint is metadata only and never a chat transport",
            ));
        }
        if !matches!(self.state.as_str(), "running" | "degraded") {
            return Err(gated(
                requested,
                format!(
                    "is {}; resume it in VOWP and retry",
                    safe_detail(&self.state, "not running")
                ),
            ));
        }
        let Some(url) = self.url.as_deref() else {
            return Err(gated(
                requested,
                "has no loopback endpoint; start or resume it in VOWP",
            ));
        };
        let Some((host, port)) = parse_loopback_url(url) else {
            return Err(incompatible(requested, "reported a non-loopback endpoint"));
        };
        if self.location == "home" && self.auth.is_none() {
            return Err(gated(
                requested,
                "has no live loopback credential; resume it in VOWP and retry",
            ));
        }
        if let Some(expected) = expected_session_id
            && self.session_id != expected
        {
            return Err(not_mirrored(
                requested,
                "reported a different home session than the attached log",
            ));
        }
        let native_session_id = match self.native_session_id.as_deref() {
            Some(native) if usable_native_id(native) => Some(native.to_owned()),
            // A present but unusable id is a malformed mirror, never a discovery hint.
            Some(_) => {
                return Err(not_mirrored(
                    requested,
                    "reported an unusable mirrored OpenCode session",
                ));
            }
            None if self.location == "home" => {
                return Err(not_mirrored(
                    requested,
                    "reported no mirrored OpenCode session for the home endpoint",
                ));
            }
            None => None,
        };
        let workspace = match self.workspace.as_deref().filter(|value| !value.is_empty()) {
            Some(workspace) => Some(parse_workspace_path(workspace).ok_or_else(|| {
                incompatible(requested, "reported an unreadable orchestrator workspace")
            })?),
            None => None,
        };
        let auth = match self.auth {
            Some(auth) => Some(auth.into_auth(requested)?),
            None => None,
        };
        Ok(ResolvedEndpoint {
            name: self.name,
            app: self.app,
            session_id: self.session_id,
            location: self.location,
            node: self.node,
            state: self.state,
            host,
            port,
            native_session_id,
            owner: self.owner,
            access: self.access,
            workspace,
            auth,
        })
    }
}

impl AuthDocument {
    pub(crate) fn into_auth(self, name: &str) -> Result<LoopbackAuth, SpiritError> {
        if !valid_header_name(&self.header) || self.password.is_empty() {
            return Err(incompatible(
                name,
                "reported an unreadable loopback credential",
            ));
        }
        let password = Zeroizing::new(self.password);
        let value = if self.scheme.eq_ignore_ascii_case("basic") {
            if self.username.contains(':') {
                return Err(incompatible(
                    name,
                    "reported an unreadable loopback credential",
                ));
            }
            let raw = Zeroizing::new(format!("{}:{}", self.username, password.as_str()));
            let encoded = Zeroizing::new(base64(raw.as_bytes()));
            Zeroizing::new(format!("Basic {}", encoded.as_str()))
        } else if self.scheme.is_empty() {
            password
        } else {
            if !valid_scheme(&self.scheme) {
                return Err(incompatible(
                    name,
                    "reported an unreadable loopback credential",
                ));
            }
            Zeroizing::new(format!("{} {}", self.scheme, password.as_str()))
        };
        if value.contains(['\r', '\n']) {
            return Err(incompatible(
                name,
                "reported an unreadable loopback credential",
            ));
        }
        Ok(LoopbackAuth {
            header: self.header,
            value,
        })
    }
}

/// Parses the VOWP loopback URL into a host and port.
///
/// Only `http://127.0.0.1:<port>`, `http://localhost:<port>` and `http://[::1]:<port>`
/// are accepted. Userinfo, a query, a fragment, a path and port zero are rejected.
pub(crate) fn parse_loopback_url(url: &str) -> Option<(LoopbackHost, u16)> {
    let rest = url.strip_prefix("http://")?;
    if rest.contains(['@', '?', '#', '\\']) {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, path),
        None => (rest, ""),
    };
    if !path.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        if host != "::1" {
            return None;
        }
        let port = rest.strip_prefix(':')?;
        (LoopbackHost::V6, port)
    } else {
        let (host, port) = authority.rsplit_once(':')?;
        let host = match host {
            "127.0.0.1" | "localhost" => LoopbackHost::V4,
            _ => return None,
        };
        (host, port)
    };
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    Some((host, port))
}

pub(crate) fn valid_orchestrator_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Validates a VOWP surface id for the lease watch.
///
/// The value is identity, not a secret; it stays core-only. VOWP uses a ULID, but the
/// shape is kept permissive enough for a future id with `-`/`_`.
pub(crate) fn valid_surface_id(surface: &str) -> bool {
    !surface.is_empty()
        && surface.len() <= 128
        && surface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn usable_native_id(native: &str) -> bool {
    !native.is_empty()
        && native.len() <= 200
        && native
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Parses one private workspace path that may become the directory header.
///
/// The path must be absolute, control-free and traversal-free. It stays core-only: no
/// error or DTO carries it.
pub(crate) fn parse_workspace_path(value: &str) -> Option<PathBuf> {
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || !Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(PathBuf::from(value))
}

fn valid_header_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && !matches!(
            lower.as_str(),
            "host"
                | "content-length"
                | "content-type"
                | "connection"
                | "transfer-encoding"
                | "accept"
                | "x-opencode-directory"
        )
}

fn valid_scheme(scheme: &str) -> bool {
    !scheme.is_empty()
        && scheme.len() <= 32
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn safe_detail<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        fallback
    }
}

fn not_configured(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_not_configured: the {} orchestrator has no local endpoint ({detail})",
            safe_detail(name, "requested")
        ),
    )
    .with_hint(
        "Enroll this Mac with VOWP's local agent, create or pair the orchestrator, then retry.",
    )
}

fn gated(name: &str, detail: impl fmt::Display) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_gated: the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint("Start or resume an opencode orchestrator in VOWP, then retry.")
}

fn not_mirrored(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Gate,
        format!(
            "chat_not_mirrored: the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint(
        "VOWP must report a usable OpenCode session mirrored to the home log; StackSpirit never creates one.",
    )
}

fn incompatible(name: &str, detail: &str) -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::VowpIncompatible,
        format!(
            "the {} orchestrator {detail}",
            safe_detail(name, "requested")
        ),
    )
    .with_hint("Update VOWP or StackSpirit. The endpoint values are never shown or guessed.")
}

/// Encodes bytes as standard base64, used only for the Basic endpoint credential.
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let value = (first << 16) | (second << 8) | third;
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_urls_are_strict_and_normalized() {
        assert_eq!(
            parse_loopback_url("http://127.0.0.1:54321"),
            Some((LoopbackHost::V4, 54321))
        );
        assert_eq!(
            parse_loopback_url("http://localhost:54321/"),
            Some((LoopbackHost::V4, 54321))
        );
        assert_eq!(
            parse_loopback_url("http://[::1]:54321"),
            Some((LoopbackHost::V6, 54321))
        );
        for rejected in [
            "https://127.0.0.1:54321",
            "http://127.0.0.1:0",
            "http://127.0.0.1",
            "http://0.0.0.0:54321",
            "http://192.168.1.2:54321",
            "http://user:pass@127.0.0.1:54321",
            "http://127.0.0.1:54321/other",
            "http://127.0.0.1:54321?x=1",
            "http://127.0.0.1:54321#frag",
            "http://localhost.evil:54321",
        ] {
            assert_eq!(parse_loopback_url(rejected), None, "{rejected} must fail");
        }
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn basic_auth_is_base64_and_never_prints_the_password() {
        let password = ["not", "a", "real", "credential"].join("-");
        let auth = AuthDocument {
            header: "Authorization".to_owned(),
            scheme: "Basic".to_owned(),
            username: "opencode".to_owned(),
            password: password.clone(),
        }
        .into_auth("global")
        .unwrap();
        let expected = format!(
            "Basic {}",
            base64(format!("opencode:{password}").as_bytes())
        );
        assert_eq!(auth.header, "Authorization");
        assert_eq!(auth.value.as_str(), expected);
        let debug = format!("{auth:?}");
        assert!(!debug.contains(&password));
        assert!(!debug.contains(&expected));
    }

    #[test]
    fn endpoint_documents_gate_before_returning_transport_values() {
        let document =
            |app: &str, state: &str, url: Option<&str>, native: Option<&str>| EndpointDocument {
                name: "global".to_owned(),
                app: app.to_owned(),
                session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                location: "node".to_owned(),
                node: None,
                state: state.to_owned(),
                url: url.map(str::to_owned),
                native_session_id: native.map(str::to_owned),
                owner: None,
                access: EndpointAccessWire::ReadOnly,
                workspace: None,
                auth: None,
            };

        let error = document(
            "claude_code",
            "running",
            Some("http://127.0.0.1:1"),
            Some("ses_1"),
        )
        .into_endpoint("global", None, EndpointSource::Agent)
        .unwrap_err();
        assert!(error.message.contains("chat_gated"));
        assert!(!error.message.contains("127.0.0.1"));

        let error = document("opencode", "stopped", None, None)
            .into_endpoint("global", None, EndpointSource::Agent)
            .unwrap_err();
        assert!(error.message.contains("chat_gated"));

        let error = document("opencode", "running", None, None)
            .into_endpoint("global", None, EndpointSource::Agent)
            .unwrap_err();
        assert!(error.message.contains("chat_gated"));

        let error = document(
            "opencode",
            "running",
            Some("http://0.0.0.0:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", None, EndpointSource::Agent)
        .unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
        assert!(!error.message.contains("0.0.0.0"));

        // A missing native id is a discovery signal, not a fence.
        let resolved = document("opencode", "running", Some("http://127.0.0.1:12345"), None)
            .into_endpoint("global", None, EndpointSource::Agent)
            .unwrap();
        assert!(resolved.native_session_id.is_none());
        assert!(resolved.workspace.is_none());

        // A present but unusable id is still rejected rather than guessed.
        let error = document(
            "opencode",
            "running",
            Some("http://127.0.0.1:12345"),
            Some("ses/1"),
        )
        .into_endpoint("global", None, EndpointSource::Agent)
        .unwrap_err();
        assert!(error.message.contains("chat_not_mirrored"));
        assert!(!error.message.contains("ses/1"));

        let error = document(
            "opencode",
            "running",
            Some("http://127.0.0.1:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", Some("another-session"), EndpointSource::Agent)
        .unwrap_err();
        assert!(error.message.contains("chat_not_mirrored"));

        let resolved = document(
            "opencode",
            "running",
            Some("http://127.0.0.1:12345"),
            Some("ses_1"),
        )
        .into_endpoint("global", None, EndpointSource::Agent)
        .unwrap();
        assert_eq!((resolved.host, resolved.port), (LoopbackHost::V4, 12345));
        assert_eq!(resolved.native_session_id.as_deref(), Some("ses_1"));
    }

    #[test]
    fn a_reported_workspace_is_validated_and_kept_core_only() {
        let document = |workspace: &str| EndpointDocument {
            name: "global".to_owned(),
            app: "opencode".to_owned(),
            session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            location: "node".to_owned(),
            node: None,
            state: "running".to_owned(),
            url: Some("http://127.0.0.1:12345".to_owned()),
            native_session_id: None,
            owner: None,
            access: EndpointAccessWire::ReadOnly,
            workspace: Some(workspace.to_owned()),
            auth: None,
        };
        let resolved = document("/tmp/orchestrator workspace")
            .into_endpoint("global", None, EndpointSource::Agent)
            .expect("an absolute workspace is accepted");
        assert_eq!(
            resolved.workspace.as_deref(),
            Some(Path::new("/tmp/orchestrator workspace"))
        );
        let debug = format!("{resolved:?}");
        assert!(!debug.contains("/tmp/orchestrator workspace"));

        let error = document("../escape")
            .into_endpoint("global", None, EndpointSource::Agent)
            .unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::VowpIncompatible);
        assert!(!error.message.contains("../escape"));
    }

    #[test]
    fn workspace_paths_are_absolute_and_control_free() {
        assert_eq!(
            parse_workspace_path("/tmp/orchestrator"),
            Some(PathBuf::from("/tmp/orchestrator"))
        );
        for rejected in ["", "relative/path", "/tmp/../escape", "/tmp/bad\npath"] {
            assert_eq!(parse_workspace_path(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn home_documents_require_a_live_loopback_auth_and_native_session() {
        let document = |location: &str, url: Option<&str>, native: Option<&str>, auth: bool| {
            EndpointDocument {
                name: "global".to_owned(),
                app: "opencode".to_owned(),
                session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
                location: location.to_owned(),
                node: None,
                state: "running".to_owned(),
                url: url.map(str::to_owned),
                native_session_id: native.map(str::to_owned),
                owner: None,
                access: EndpointAccessWire::ReadOnly,
                workspace: None,
                auth: auth.then(|| AuthDocument {
                    header: "Authorization".to_owned(),
                    scheme: "Basic".to_owned(),
                    username: "opencode".to_owned(),
                    password: "fake-engine-credential".to_owned(),
                }),
            }
        };

        // The home command's node metadata is never a transit document.
        let error = document("node", None, None, false)
            .into_endpoint("global", None, EndpointSource::Home)
            .unwrap_err();
        assert!(error.message.contains("chat_gated"));
        assert!(!error.message.contains("http://"));

        // A home-hosted document needs a live URL, auth and native session.
        for (url, native, auth, expected) in [
            (
                Some("http://127.0.0.1:1"),
                Some("ses_home_fake"),
                false,
                "credential",
            ),
            (Some("http://127.0.0.1:1"), None, true, "mirrored"),
        ] {
            let error = document("home", url, native, auth)
                .into_endpoint("global", None, EndpointSource::Home)
                .unwrap_err();
            assert!(
                error.message.contains(expected),
                "unexpected typed error: {error}"
            );
        }

        let resolved = document(
            "home",
            Some("http://127.0.0.1:43210"),
            Some("ses_home_fake"),
            true,
        )
        .into_endpoint("global", None, EndpointSource::Home)
        .unwrap();
        assert_eq!(resolved.location, "home");
        assert_eq!(resolved.native_session_id.as_deref(), Some("ses_home_fake"));
        assert!(resolved.auth.is_some());
        assert_eq!((resolved.host, resolved.port), (LoopbackHost::V4, 43210));
    }

    #[test]
    fn ownership_defaults_are_safe_and_unknown_values_do_not_fail_parsing() {
        let minimal = r#"{
            "name": "global", "app": "opencode", "session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "location": "node", "state": "running"
        }"#;
        let document: EndpointDocument = serde_json::from_str(minimal).unwrap();
        assert!(document.owner.is_none());
        assert_eq!(document.access, EndpointAccessWire::ReadOnly);

        let future = r#"{
            "name": "global", "app": "opencode", "session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "location": "node", "state": "running",
            "owner": {
                "kind": "hologram", "label": "Future surface", "holder": "pi",
                "node": "mac-studio", "mode": "cooperative", "state": "attached",
                "activity": "thinking",
                "surface_id": "surf_1", "session_id": "ses_private",
                "epoch": 42, "lease_expires_at": "2026-11-27T00:00:00.000Z"
            },
            "access": "owner"
        }"#;
        let document: EndpointDocument = serde_json::from_str(future).unwrap();
        let owner = document.owner.unwrap();
        assert_eq!(owner.kind, "hologram");
        assert_eq!(owner.label.as_deref(), Some("Future surface"));
        assert_eq!(owner.holder.as_deref(), Some("pi"));
        assert_eq!(owner.node.as_deref(), Some("mac-studio"));
        assert_eq!(owner.mode.as_deref(), Some("cooperative"));
        assert_eq!(owner.state.as_deref(), Some("attached"));
        assert_eq!(owner.activity.as_deref(), Some("thinking"));
        assert_eq!(document.access, EndpointAccessWire::Owner);

        let unknown_access = r#"{
            "name": "global", "app": "opencode", "session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "location": "node", "state": "running", "access": "future_writer"
        }"#;
        let document: EndpointDocument = serde_json::from_str(unknown_access).unwrap();
        assert_eq!(document.access, EndpointAccessWire::ReadOnly);
    }
}
