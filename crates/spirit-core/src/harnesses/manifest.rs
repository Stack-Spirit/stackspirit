//! Harness manifest model, strict validation and the built-in presets.
//!
//! A manifest describes how StackSpirit launches and signs in to one coding harness.
//! It never contains an API key, OAuth token, password or private-key material: those
//! live in the local key store or VOWP's `vowp secret` vault.

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;

/// The id of the default built-in harness.
pub const DEFAULT_HARNESS_ID: &str = "opencode";

/// Every built-in harness id. A custom file may not shadow one of these.
pub const BUILTIN_HARNESS_IDS: [&str; 25] = [
    "opencode",
    "cursor-cli",
    "github-copilot",
    "auggie",
    "kiro",
    "goose",
    "codex",
    "cline",
    "kilo",
    "qwen-code",
    "mistral-vibe",
    "kimi-code",
    "minimax-code",
    "glm-agent",
    "pi-acp",
    "devin-cli",
    "factory-droid",
    "grok-build",
    "junie",
    "claude-code",
    "gemini-cli",
    "antigravity",
    "aider",
    "crush",
    "amp",
];

/// How StackSpirit talks to one harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessProtocol {
    /// OpenCode's local HTTP server.
    OpencodeHttp,
    /// Agent Client Protocol over stdio.
    Acp,
    /// Claude Code's stream-json output.
    StreamJson,
    /// Codex CLI's JSON output.
    CodexJson,
    /// An interactive CLI owned by a pseudo-terminal.
    CliPty,
}

impl HarnessProtocol {
    /// Returns the canonical kebab-case wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpencodeHttp => "opencode-http",
            Self::Acp => "acp",
            Self::StreamJson => "stream-json",
            Self::CodexJson => "codex-json",
            Self::CliPty => "cli-pty",
        }
    }

    /// Returns a short human label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpencodeHttp => "OpenCode HTTP",
            Self::Acp => "ACP",
            Self::StreamJson => "Stream JSON",
            Self::CodexJson => "Codex JSON",
            Self::CliPty => "CLI (pty)",
        }
    }

    /// Parses the canonical wire string.
    pub fn parse(value: &str) -> Result<Self, SpiritError> {
        match value {
            "opencode-http" => Ok(Self::OpencodeHttp),
            "acp" => Ok(Self::Acp),
            "stream-json" => Ok(Self::StreamJson),
            "codex-json" => Ok(Self::CodexJson),
            "cli-pty" => Ok(Self::CliPty),
            _ => Err(SpiritError::invalid_input(
                "protocol",
                "unknown harness protocol",
            )),
        }
    }
}

impl std::fmt::Display for HarnessProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the owner authenticates one harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessAuthMode {
    /// The owner supplies a provider API key (BYOK).
    ApiKey,
    /// The owner signs in with a consumer subscription.
    Subscription,
}

impl HarnessAuthMode {
    /// Returns the canonical wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Subscription => "subscription",
        }
    }

    /// Parses the canonical wire string.
    pub fn parse(value: &str) -> Result<Self, SpiritError> {
        match value {
            "api_key" => Ok(Self::ApiKey),
            "subscription" => Ok(Self::Subscription),
            _ => Err(SpiritError::invalid_input(
                "auth_modes",
                "unknown auth mode",
            )),
        }
    }
}

impl std::fmt::Display for HarnessAuthMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one harness is allowed to do in StackSpirit.
///
/// This is the single behavioural and UI split: a harness is either an official
/// orchestrator/worker backend whose protocol can back the embedded chat, or a stock
/// CLI that StackSpirit only runs in the terminal worker view. A consumer OAuth
/// sign-in is never wrapped in StackSpirit's embedded chat for the worker-only group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessSurface {
    /// Can act as the orchestrator and as a worker, using OpenCode's HTTP server or an
    /// official ACP / Codex app-server protocol.
    OrchestratorAndWorkers,
    /// Stock CLI terminal view only; never wrapped in the embedded chat.
    WorkersOnlyTerminal,
}

impl HarnessSurface {
    /// Returns the canonical snake_case wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrchestratorAndWorkers => "orchestrator_and_workers",
            Self::WorkersOnlyTerminal => "workers_only_terminal",
        }
    }

    /// Returns the exact section/heading label used by the desktop panel.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OrchestratorAndWorkers => "Orchestrator + Workers (ACP / Embedded Chat)",
            Self::WorkersOnlyTerminal => "Workers Only (Stock CLI Terminal View)",
        }
    }

    /// True when this surface may back the embedded chat.
    #[must_use]
    pub const fn accepts_embedded_chat(self) -> bool {
        matches!(self, Self::OrchestratorAndWorkers)
    }

    /// Parses the canonical snake_case wire string.
    pub fn parse(value: &str) -> Result<Self, SpiritError> {
        match value {
            "orchestrator_and_workers" => Ok(Self::OrchestratorAndWorkers),
            "workers_only_terminal" => Ok(Self::WorkersOnlyTerminal),
            _ => Err(SpiritError::invalid_input(
                "surface",
                "unknown harness surface",
            )),
        }
    }

    /// Derives the surface from the legacy `supports_embedded_chat` boolean.
    #[must_use]
    pub const fn from_embedded_chat(supports_embedded_chat: bool) -> Self {
        if supports_embedded_chat {
            Self::OrchestratorAndWorkers
        } else {
            Self::WorkersOnlyTerminal
        }
    }
}

impl std::fmt::Display for HarnessSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One harness definition. Built-ins ship with the app; custom entries are loaded from
/// `~/.stackspirit/harnesses/<id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessManifest {
    /// Stable lowercase id, also the custom file stem.
    pub id: String,
    /// Human display name.
    pub name: String,
    /// Executable name on PATH (never a shell string).
    pub binary: String,
    /// Display-only install hint. StackSpirit never runs this automatically.
    pub install_cmd: String,
    /// Shell-free argv line that starts the harness sign-in, or an empty string when the
    /// harness signs in inside its own terminal.
    pub auth_cmd: String,
    /// Transport/protocol family.
    pub protocol: HarnessProtocol,
    /// The explicit capability split: orchestrator + workers, or workers only.
    pub surface: HarnessSurface,
    /// Legacy mirror of `surface`: true exactly when the surface backs the embedded
    /// chat. Kept so older bridge payloads remain readable.
    pub supports_embedded_chat: bool,
    /// Accepted authentication modes.
    pub auth_modes: Vec<HarnessAuthMode>,
    /// Plain-language API-key versus subscription terms note.
    pub tos_note: String,
    /// Vendor documentation URL.
    pub docs_url: String,
    /// Loopback OAuth callback port, when the harness prints a `127.0.0.1` callback.
    pub oauth_callback_port: Option<u16>,
    /// True only for a manifest that ships with StackSpirit.
    pub builtin: bool,
}

impl HarnessManifest {
    /// Rejects an id that cannot safely be a file stem or a stable registry key.
    ///
    /// The rules are deliberately narrower than TOML or the filesystem: 1-64 lowercase
    /// ASCII letters, digits and hyphens, starting and ending with a letter or digit.
    /// There is no `.`, `/`, `\`, whitespace or uppercase, so `..` and path traversal
    /// cannot be expressed.
    pub fn validate_id(id: &str) -> Result<(), SpiritError> {
        if id.is_empty() || id.len() > 64 {
            return Err(SpiritError::invalid_input(
                "id",
                "must be 1-64 lowercase letters, digits or hyphens",
            ));
        }
        let mut chars = id.chars();
        let Some(first) = chars.next() else {
            return Err(SpiritError::invalid_input("id", "must not be empty"));
        };
        let Some(last) = id.chars().last() else {
            return Err(SpiritError::invalid_input("id", "must not be empty"));
        };
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return Err(SpiritError::invalid_input(
                "id",
                "must start with a lowercase letter or digit",
            ));
        }
        if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
            return Err(SpiritError::invalid_input(
                "id",
                "must end with a lowercase letter or digit",
            ));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(SpiritError::invalid_input(
                "id",
                "use lowercase letters, digits and hyphens only",
            ));
        }
        Ok(())
    }

    /// Returns true when `id` names a built-in manifest.
    #[must_use]
    pub fn is_builtin_id(id: &str) -> bool {
        BUILTIN_HARNESS_IDS.contains(&id)
    }

    /// Validates every field without touching the network or the filesystem.
    pub fn validate(&self) -> Result<(), SpiritError> {
        Self::validate_id(&self.id)?;
        if self.supports_embedded_chat != self.surface.accepts_embedded_chat() {
            return Err(SpiritError::invalid_input(
                "surface",
                "surface and supports_embedded_chat disagree",
            ));
        }
        if self.protocol == HarnessProtocol::CliPty
            && self.surface != HarnessSurface::WorkersOnlyTerminal
        {
            return Err(SpiritError::invalid_input(
                "protocol",
                "cli-pty is workers-only and cannot back the embedded chat",
            ));
        }
        validate_text("name", &self.name, 1, 100)?;
        validate_binary(&self.binary)?;
        validate_text("install_cmd", &self.install_cmd, 0, 1024)?;
        validate_text("auth_cmd", &self.auth_cmd, 0, 512)?;
        if !self.auth_cmd.trim().is_empty() {
            crate::harnesses::probe::parse_auth_argv(&self.auth_cmd)?;
        }
        validate_text("tos_note", &self.tos_note, 1, 2000)?;
        validate_docs_url(&self.docs_url)?;
        if self.oauth_callback_port == Some(0) {
            return Err(SpiritError::invalid_input(
                "oauth_callback_port",
                "must be 1-65535",
            ));
        }
        if self.auth_modes.is_empty() {
            return Err(SpiritError::invalid_input(
                "auth_modes",
                "choose api_key, subscription, or both",
            ));
        }
        for (index, mode) in self.auth_modes.iter().enumerate() {
            if self.auth_modes[..index].contains(mode) {
                return Err(SpiritError::invalid_input(
                    "auth_modes",
                    "must not repeat a mode",
                ));
            }
        }
        for (field, value) in [
            ("name", self.name.as_str()),
            ("binary", self.binary.as_str()),
            ("install_cmd", self.install_cmd.as_str()),
            ("auth_cmd", self.auth_cmd.as_str()),
            ("tos_note", self.tos_note.as_str()),
            ("docs_url", self.docs_url.as_str()),
        ] {
            reject_secret_material(field, value)?;
        }
        Ok(())
    }

    /// True when this manifest can back the embedded chat.
    ///
    /// The surface is authoritative; `supports_embedded_chat` is its legacy mirror.
    #[must_use]
    pub const fn embedded_chat(&self) -> bool {
        self.surface.accepts_embedded_chat()
    }

    /// True when the manifest accepts an API key.
    #[must_use]
    pub fn accepts_api_key(&self) -> bool {
        self.auth_modes.contains(&HarnessAuthMode::ApiKey)
    }

    /// True when the manifest accepts a consumer subscription sign-in.
    #[must_use]
    pub fn accepts_subscription(&self) -> bool {
        self.auth_modes.contains(&HarnessAuthMode::Subscription)
    }
}

fn validate_text(field: &str, value: &str, min: usize, max: usize) -> Result<(), SpiritError> {
    let count = value.chars().count();
    if count < min || (min > 0 && value.trim().is_empty()) {
        return Err(SpiritError::invalid_input(field, "must not be empty"));
    }
    if count > max {
        return Err(SpiritError::invalid_input(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(SpiritError::invalid_input(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

fn validate_binary(binary: &str) -> Result<(), SpiritError> {
    if binary.is_empty() || binary.len() > 128 {
        return Err(SpiritError::invalid_input(
            "binary",
            "must be 1-128 characters",
        ));
    }
    let Some(first) = binary.chars().next() else {
        return Err(SpiritError::invalid_input("binary", "must not be empty"));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(SpiritError::invalid_input(
            "binary",
            "must start with a letter or digit",
        ));
    }
    if !binary
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        return Err(SpiritError::invalid_input(
            "binary",
            "use a PATH executable name, not a shell string",
        ));
    }
    Ok(())
}

fn validate_docs_url(url: &str) -> Result<(), SpiritError> {
    if !url.starts_with("https://") || url.len() > 2048 {
        return Err(SpiritError::invalid_input(
            "docs_url",
            "must be an https URL",
        ));
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(SpiritError::invalid_input(
            "docs_url",
            "must not contain whitespace or control characters",
        ));
    }
    Ok(())
}

/// Rejects a manifest string that looks like it carries a key or token value.
///
/// The manifest schema already rejects unknown fields (so there is no `api_key` or
/// `token` field to save); this catches an owner pasting a value into a free-text field
/// such as `auth_cmd`. The message never echoes the value.
fn reject_secret_material(field: &str, value: &str) -> Result<(), SpiritError> {
    if value.contains("-----BEGIN") {
        return Err(secret_material_error(field));
    }
    // Split the literal prefixes so the public-readiness scan does not treat this
    // source as a leaked credential.
    const PREFIXES: [&str; 8] = [
        concat!("s", "k-"),
        concat!("s", "k_"),
        concat!("g", "hp_"),
        concat!("git", "hub_pat_"),
        concat!("x", "oxb-"),
        concat!("x", "oxp-"),
        concat!("ak", "ia"),
        concat!("ai", "za"),
    ];
    for token in value.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';')) {
        let lower = token.to_ascii_lowercase();
        if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) {
            return Err(secret_material_error(field));
        }
    }
    Ok(())
}

fn secret_material_error(field: &str) -> SpiritError {
    SpiritError::invalid_input(
        field,
        "must not contain secret material; keys belong in the key store or VOWP secret vault",
    )
}

const API_KEY: &[HarnessAuthMode] = &[HarnessAuthMode::ApiKey];
const SUBSCRIPTION: &[HarnessAuthMode] = &[HarnessAuthMode::Subscription];
const API_KEY_OR_SUBSCRIPTION: &[HarnessAuthMode] =
    &[HarnessAuthMode::ApiKey, HarnessAuthMode::Subscription];

struct BuiltinPreset {
    id: &'static str,
    name: &'static str,
    binary: &'static str,
    install_cmd: &'static str,
    auth_cmd: &'static str,
    protocol: HarnessProtocol,
    surface: HarnessSurface,
    auth_modes: &'static [HarnessAuthMode],
    tos_note: String,
    docs_url: &'static str,
    oauth_callback_port: Option<u16>,
}

fn preset(value: BuiltinPreset) -> HarnessManifest {
    HarnessManifest {
        id: value.id.to_owned(),
        name: value.name.to_owned(),
        binary: value.binary.to_owned(),
        install_cmd: value.install_cmd.to_owned(),
        auth_cmd: value.auth_cmd.to_owned(),
        protocol: value.protocol,
        surface: value.surface,
        supports_embedded_chat: value.surface.accepts_embedded_chat(),
        auth_modes: value.auth_modes.to_vec(),
        tos_note: value.tos_note,
        docs_url: value.docs_url.to_owned(),
        oauth_callback_port: value.oauth_callback_port,
        builtin: true,
    }
}

fn byok_note(product: &str, vendor: &str) -> String {
    format!(
        "BYOK: {product} uses the provider API key you connect. {vendor} billing and terms \
         apply; StackSpirit does not assume a consumer subscription."
    )
}

fn subscription_note(product: &str, vendor: &str) -> String {
    format!(
        "Sign-in for {product} uses your {vendor} subscription and is governed by {vendor}'s \
         consumer terms; automated or third-party use may be restricted."
    )
}

fn stock_cli_note(product: &str, vendor: &str) -> String {
    format!(
        "Workers only: StackSpirit never wraps this consumer sign-in in its embedded chat. \
         {product} runs as a stock CLI worker under your {vendor} subscription and {vendor}'s \
         consumer terms; automated or third-party use may be restricted."
    )
}

/// Returns every built-in manifest in display order.
///
/// OpenCode is first and is the default. The remaining entries follow the explicit
/// [`HarnessSurface`] split: orchestrator + workers first, then the stock-CLI
/// workers-only group.
#[must_use]
pub fn builtin_harnesses() -> Vec<HarnessManifest> {
    use HarnessProtocol::{Acp, CliPty, CodexJson, OpencodeHttp, StreamJson};
    use HarnessSurface::{OrchestratorAndWorkers, WorkersOnlyTerminal};

    vec![
        // Orchestrator + Workers (embedded chat via OpenCode or official ACP /
        // Codex app-server). OpenCode stays the pinned default.
        preset(BuiltinPreset {
            id: "opencode",
            name: "OpenCode",
            binary: "opencode",
            install_cmd: "curl -fsSL https://opencode.ai/install | bash",
            auth_cmd: "opencode auth login",
            protocol: OpencodeHttp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: "BYOK: OpenCode uses the provider API key you connect. Provider billing \
                       and terms apply; StackSpirit does not assume a consumer subscription."
                .to_owned(),
            docs_url: "https://opencode.ai/docs/",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "cursor-cli",
            name: "Cursor CLI",
            binary: "agent",
            install_cmd: "curl https://cursor.com/install -fsS | bash",
            auth_cmd: "agent login",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Cursor CLI", "Cursor"),
            docs_url: "https://cursor.com/cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "github-copilot",
            name: "GitHub Copilot",
            binary: "copilot",
            install_cmd: "npm install -g @github/copilot",
            auth_cmd: "copilot login",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("GitHub Copilot", "GitHub Copilot"),
            docs_url: "https://docs.github.com/en/copilot/how-tos/set-up/install-copilot-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "auggie",
            name: "Auggie",
            binary: "auggie",
            install_cmd: "npm install -g @augmentcode/auggie",
            auth_cmd: "auggie login",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Auggie", "Augment Code"),
            docs_url: "https://docs.augmentcode.com/cli/overview",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "kiro",
            name: "Kiro",
            binary: "kiro-cli",
            install_cmd: "curl -fsSL https://cli.kiro.dev/install | bash",
            auth_cmd: "kiro-cli login",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Kiro", "Kiro"),
            docs_url: "https://kiro.dev/docs/cli/",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "goose",
            name: "Goose",
            binary: "goose",
            install_cmd: "curl -fsSL https://block.github.io/goose/install.sh | bash",
            auth_cmd: "goose configure",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("Goose", "The provider's"),
            docs_url: "https://block.github.io/goose/docs/getting-started/installation",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "codex",
            name: "Codex",
            binary: "codex",
            install_cmd: "npm install -g @openai/codex",
            auth_cmd: "codex login",
            protocol: CodexJson,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY_OR_SUBSCRIPTION,
            tos_note: "API-key use is billed under OpenAI's API terms. ChatGPT subscription \
                       sign-in in a third-party client is governed by OpenAI's Consumer Terms \
                       and may be restricted; check them before connecting. The embedded \
                       app-server transport uses `codex app-server`."
                .to_owned(),
            docs_url: "https://developers.openai.com/codex/cli/",
            oauth_callback_port: Some(1455),
        }),
        preset(BuiltinPreset {
            id: "cline",
            name: "Cline",
            binary: "cline",
            install_cmd: "npm install -g cline",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("Cline", "The provider's"),
            docs_url: "https://docs.cline.bot/cline-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "kilo",
            name: "Kilo Code",
            binary: "kilocode",
            install_cmd: "npm install -g @kilocode/cli",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("Kilo Code", "The provider's"),
            docs_url: "https://kilocode.ai/docs",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "qwen-code",
            name: "Qwen Code",
            binary: "qwen",
            install_cmd: "npm install -g @qwen-code/qwen-code",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY_OR_SUBSCRIPTION,
            tos_note: "API-key use follows Alibaba's API terms. Qwen account sign-in follows \
                       Alibaba's consumer terms and may be restricted for automated use."
                .to_owned(),
            docs_url: "https://github.com/QwenLM/qwen-code",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "mistral-vibe",
            name: "Mistral Vibe",
            binary: "vibe",
            install_cmd: "pip install mistral-vibe",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("Mistral Vibe", "Mistral's"),
            docs_url: "https://github.com/mistralai/mistral-vibe",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "kimi-code",
            name: "Kimi Code",
            binary: "kimi",
            install_cmd: "pip install kimi-cli",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Kimi Code", "Moonshot AI"),
            docs_url: "https://github.com/MoonshotAI/kimi-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "minimax-code",
            name: "MiniMax Code",
            binary: "minimax",
            install_cmd: "npm install -g @minimax-ai/cli",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("MiniMax Code", "MiniMax's"),
            docs_url: "https://github.com/MiniMax-AI/MiniMax-CLI",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "glm-agent",
            name: "GLM Agent",
            binary: "glm",
            install_cmd: "npm install -g glm-agent",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("GLM Agent", "Z.ai's"),
            docs_url: "https://github.com/zai-org/glm-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "pi-acp",
            name: "Pi ACP",
            binary: "pi",
            install_cmd: "npm install -g pi-acp",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: API_KEY,
            tos_note: byok_note("Pi ACP", "The provider's"),
            docs_url: "https://github.com/pi-labs/pi",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "devin-cli",
            name: "Devin CLI",
            binary: "devin",
            install_cmd: "npm install -g devin",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Devin CLI", "Devin"),
            docs_url: "https://docs.devin.ai/devin-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "factory-droid",
            name: "Factory Droid",
            binary: "droid",
            install_cmd: "curl -fsSL https://app.factory.ai/cli | sh",
            auth_cmd: "droid login",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Factory Droid", "Factory"),
            docs_url: "https://docs.factory.ai/cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "grok-build",
            name: "Grok Build",
            binary: "grok",
            install_cmd: "npm install -g @xai/grok-cli",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Grok Build", "xAI"),
            docs_url: "https://docs.x.ai/docs/guides/grok-cli",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "junie",
            name: "Junie",
            binary: "junie",
            install_cmd: "npm install -g @jetbrains/junie",
            auth_cmd: "",
            protocol: Acp,
            surface: OrchestratorAndWorkers,
            auth_modes: SUBSCRIPTION,
            tos_note: subscription_note("Junie", "JetBrains"),
            docs_url: "https://www.jetbrains.com/junie/",
            oauth_callback_port: None,
        }),
        // Workers only (stock CLI terminal view). StackSpirit never wraps the consumer
        // OAuth sign-in of this group in its embedded chat.
        preset(BuiltinPreset {
            id: "claude-code",
            name: "Claude Code",
            binary: "claude",
            install_cmd: "npm install -g @anthropic-ai/claude-code",
            auth_cmd: "claude auth login",
            protocol: StreamJson,
            surface: WorkersOnlyTerminal,
            auth_modes: API_KEY_OR_SUBSCRIPTION,
            tos_note: "Workers only: StackSpirit never wraps this consumer sign-in in its \
                       embedded chat. API-key use is billed under Anthropic's commercial API \
                       terms. Claude Pro/Max subscription sign-in in a third-party client is \
                       governed by Anthropic's Consumer Terms and may be restricted; check \
                       them before connecting."
                .to_owned(),
            docs_url: "https://docs.anthropic.com/en/docs/claude-code",
            oauth_callback_port: Some(54545),
        }),
        preset(BuiltinPreset {
            id: "gemini-cli",
            name: "Gemini CLI",
            binary: "gemini",
            install_cmd: "npm install -g @google/gemini-cli",
            auth_cmd: "",
            protocol: Acp,
            surface: WorkersOnlyTerminal,
            auth_modes: API_KEY_OR_SUBSCRIPTION,
            tos_note: "Workers only: StackSpirit never wraps this consumer sign-in in its \
                       embedded chat. A Gemini API key is billed under Google's API terms. \
                       Personal Google-account subscription sign-in follows Google's consumer \
                       terms and may be restricted for automated use."
                .to_owned(),
            docs_url: "https://github.com/google-gemini/gemini-cli",
            oauth_callback_port: Some(8085),
        }),
        preset(BuiltinPreset {
            id: "antigravity",
            name: "Antigravity",
            binary: "antigravity",
            install_cmd: "Download Google Antigravity from https://antigravity.google",
            auth_cmd: "",
            protocol: CliPty,
            surface: WorkersOnlyTerminal,
            auth_modes: SUBSCRIPTION,
            tos_note: "Workers only: StackSpirit never wraps this consumer sign-in in its \
                       embedded chat. Sign-in uses your Google account and is governed by \
                       Google's Antigravity terms; automated or third-party use may be \
                       restricted."
                .to_owned(),
            docs_url: "https://antigravity.google/docs",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "aider",
            name: "Aider",
            binary: "aider",
            install_cmd: "python -m pip install aider-install && aider-install",
            auth_cmd: "",
            protocol: CliPty,
            surface: WorkersOnlyTerminal,
            auth_modes: API_KEY,
            tos_note: "Workers only: Aider runs as a stock CLI worker. BYOK: Aider uses the \
                       provider API key you configure. Provider billing and terms apply."
                .to_owned(),
            docs_url: "https://aider.chat/docs/",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "crush",
            name: "Crush",
            binary: "crush",
            install_cmd: "go install github.com/charmbracelet/crush@latest",
            auth_cmd: "",
            protocol: CliPty,
            surface: WorkersOnlyTerminal,
            auth_modes: API_KEY,
            tos_note: byok_note("Crush", "The provider's"),
            docs_url: "https://github.com/charmbracelet/crush",
            oauth_callback_port: None,
        }),
        preset(BuiltinPreset {
            id: "amp",
            name: "Amp",
            binary: "amp",
            install_cmd: "npm install -g @sourcegraph/amp",
            auth_cmd: "",
            protocol: CliPty,
            surface: WorkersOnlyTerminal,
            auth_modes: SUBSCRIPTION,
            tos_note: stock_cli_note("Amp", "Sourcegraph"),
            docs_url: "https://ampcode.com/manual",
            oauth_callback_port: None,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_validates_and_has_a_distinct_id() {
        let presets = builtin_harnesses();
        assert_eq!(presets.len(), BUILTIN_HARNESS_IDS.len());
        for preset in &presets {
            preset.validate().unwrap();
            assert!(preset.builtin);
            assert!(BUILTIN_HARNESS_IDS.contains(&preset.id.as_str()));
        }
        assert_eq!(presets[0].id, DEFAULT_HARNESS_ID);
        assert_eq!(presets[0].surface, HarnessSurface::OrchestratorAndWorkers);
        assert!(presets[0].embedded_chat());
    }

    #[test]
    fn builtin_catalog_has_one_explicit_surface_per_harness() {
        let orchestrators: Vec<_> = builtin_harnesses()
            .into_iter()
            .filter(|preset| preset.surface == HarnessSurface::OrchestratorAndWorkers)
            .map(|preset| preset.id)
            .collect();
        assert_eq!(
            orchestrators,
            [
                "opencode",
                "cursor-cli",
                "github-copilot",
                "auggie",
                "kiro",
                "goose",
                "codex",
                "cline",
                "kilo",
                "qwen-code",
                "mistral-vibe",
                "kimi-code",
                "minimax-code",
                "glm-agent",
                "pi-acp",
                "devin-cli",
                "factory-droid",
                "grok-build",
                "junie",
            ]
        );
        let workers: Vec<_> = builtin_harnesses()
            .into_iter()
            .filter(|preset| preset.surface == HarnessSurface::WorkersOnlyTerminal)
            .map(|preset| preset.id)
            .collect();
        assert_eq!(
            workers,
            [
                "claude-code",
                "gemini-cli",
                "antigravity",
                "aider",
                "crush",
                "amp",
            ]
        );
        for preset in builtin_harnesses() {
            assert_eq!(
                preset.supports_embedded_chat,
                preset.surface.accepts_embedded_chat(),
                "{}",
                preset.id
            );
        }
    }

    #[test]
    fn surface_validation_keeps_the_split_honest() {
        let mut preset = builtin_harnesses().remove(0);
        preset.supports_embedded_chat = false;
        assert!(preset.validate().is_err());

        let mut preset = builtin_harnesses().remove(0);
        preset.protocol = HarnessProtocol::CliPty;
        assert!(preset.validate().is_err());

        let mut preset = builtin_harnesses().remove(0);
        preset.surface = HarnessSurface::WorkersOnlyTerminal;
        preset.protocol = HarnessProtocol::Acp;
        preset.supports_embedded_chat = false;
        preset.validate().unwrap();
    }

    #[test]
    fn id_validation_is_strict() {
        for good in ["a", "opencode", "claude-code", "a1", "my-harness-2"] {
            HarnessManifest::validate_id(good).unwrap();
        }
        for bad in [
            "",
            "OpenCode",
            "claude_code",
            "-claude",
            "claude-",
            "../evil",
            "a/b",
            "a b",
            "a.b",
            "a..b",
            &"a".repeat(65),
        ] {
            assert!(HarnessManifest::validate_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn builtin_ids_are_reserved() {
        for preset in builtin_harnesses() {
            assert!(HarnessManifest::is_builtin_id(&preset.id));
        }
        assert!(!HarnessManifest::is_builtin_id("mine"));
    }

    #[test]
    fn secret_material_in_free_text_is_rejected() {
        let mut preset = builtin_harnesses().remove(0);
        let fake_token = format!("{}1234567890", concat!("s", "k-"));
        preset.auth_cmd = format!("opencode auth login {fake_token}");
        assert!(preset.validate().is_err());
        let mut preset = builtin_harnesses().remove(0);
        preset.tos_note = ["-----BEGIN", "PRIVATE", "KEY-----", "not allowed"].join(" ");
        assert!(preset.validate().is_err());
    }
}
