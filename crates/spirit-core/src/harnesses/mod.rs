//! Universal harness registry and one-click connect planning.
//!
//! The registry describes how StackSpirit launches a coding harness on this Mac or the
//! Home VPS, whether it can back the embedded chat or only run as a worker, and what
//! the owner must know about API-key versus subscription terms. Every manifest carries
//! an explicit [`HarnessSurface`]: `OrchestratorAndWorkers` for OpenCode or an official
//! ACP / Codex app-server harness, or `WorkersOnlyTerminal` for a stock CLI whose
//! consumer sign-in is never wrapped in the embedded chat.
//!
//! [`acp`] is the reusable ACP stdio JSON-RPC 2.0 client: handshake, session creation,
//! prompt turns, streaming updates, permission requests and cancel framing. It never
//! reads a credential file or extracts an OAuth token.
//!
//! Built-in presets ship in code. Owner-defined manifests live in
//! `~/.stackspirit/harnesses/<id>.toml`; the schema has no credential field, unknown
//! fields are rejected, writes are atomic and owner-only, and no file is ever read for a
//! secret value. Probe commands only ask for a version and a non-interactive sign-in
//! status, and every returned detail is fixed copy or redacted text.
//!
//! The connect plan is the desktop one-click path: for a Home VPS harness with a
//! loopback OAuth callback it carries the exact temporary
//! `ssh -N -L 127.0.0.1:<port>:127.0.0.1:<port>` command, and for every target it
//! carries the shell-free sign-in command. The desktop host owns both child processes.

pub mod acp;
pub mod manifest;
pub mod probe;
mod store;
mod toml;

pub use acp::{
    ACP_MAX_LINE_BYTES, ACP_PROMPT_TIMEOUT, ACP_PROTOCOL_VERSION, ACP_REQUEST_TIMEOUT, AcpClient,
    AcpClientCapabilities, AcpContentBlock, AcpFsCapabilities, AcpInitializeResult, AcpLaunchSpec,
    AcpObserver, AcpPermissionDecision, AcpPermissionOption, AcpPermissionRequest,
    AcpPromptOutcome, AcpPromptResult, AcpSessionUpdate, AcpUpdate, CollectingObserver,
};
pub use manifest::{
    BUILTIN_HARNESS_IDS, DEFAULT_HARNESS_ID, HarnessAuthMode, HarnessManifest, HarnessProtocol,
    HarnessSurface, builtin_harnesses,
};
pub use probe::{
    AUTH_TIMEOUT, FORWARD_TIMEOUT, HarnessAuthState, HarnessBinaryState, HarnessConnectPlan,
    HarnessConnectPlanView, HarnessProbe, HarnessProbeReport, HarnessProbeTarget, HarnessReadiness,
    HarnessTargetKind, LoopbackForwardPlan, PROBE_TIMEOUT, build_connect_plan,
    loopback_forward_plan, parse_auth_argv, probe_harness,
};
pub use store::{HarnessListView, HarnessRegistry, default_harness_dir};
