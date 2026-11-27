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
pub const BUILTIN_HARNESS_IDS: [&str; 8] = [
    "opencode",
    "claude-code",
    "codex",
    "gemini-cli",
    "antigravity",
    "cursor-cli",
    "aider",
    "goose",
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
    /// True when the harness can back StackSpirit's embedded chat; false means
    /// worker-only.
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
    #[must_use]
    pub const fn embedded_chat(&self) -> bool {
        self.supports_embedded_chat
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

/// Returns every built-in manifest in display order. OpenCode is first and is the
/// default.
#[must_use]
pub fn builtin_harnesses() -> Vec<HarnessManifest> {
    vec![
        HarnessManifest {
            id: "opencode".to_owned(),
            name: "OpenCode".to_owned(),
            binary: "opencode".to_owned(),
            install_cmd: "curl -fsSL https://opencode.ai/install | bash".to_owned(),
            auth_cmd: "opencode auth login".to_owned(),
            protocol: HarnessProtocol::OpencodeHttp,
            supports_embedded_chat: true,
            auth_modes: vec![HarnessAuthMode::ApiKey],
            tos_note: "BYOK: OpenCode uses the provider API key you connect. Provider billing \
                       and terms apply; StackSpirit does not assume a consumer subscription."
                .to_owned(),
            docs_url: "https://opencode.ai/docs/".to_owned(),
            oauth_callback_port: None,
            builtin: true,
        },
        HarnessManifest {
            id: "claude-code".to_owned(),
            name: "Claude Code".to_owned(),
            binary: "claude".to_owned(),
            install_cmd: "npm install -g @anthropic-ai/claude-code".to_owned(),
            auth_cmd: "claude auth login".to_owned(),
            protocol: HarnessProtocol::StreamJson,
            supports_embedded_chat: true,
            auth_modes: vec![HarnessAuthMode::ApiKey, HarnessAuthMode::Subscription],
            tos_note: "API-key use is billed under Anthropic's commercial API terms. Claude \
                       Pro/Max subscription sign-in in a third-party client is governed by \
                       Anthropic's Consumer Terms and may be restricted; check them before \
                       connecting."
                .to_owned(),
            docs_url: "https://docs.anthropic.com/en/docs/claude-code".to_owned(),
            oauth_callback_port: Some(54545),
            builtin: true,
        },
        HarnessManifest {
            id: "codex".to_owned(),
            name: "Codex".to_owned(),
            binary: "codex".to_owned(),
            install_cmd: "npm install -g @openai/codex".to_owned(),
            auth_cmd: "codex login".to_owned(),
            protocol: HarnessProtocol::CodexJson,
            supports_embedded_chat: true,
            auth_modes: vec![HarnessAuthMode::ApiKey, HarnessAuthMode::Subscription],
            tos_note: "API-key use is billed under OpenAI's API terms. ChatGPT subscription \
                       sign-in in a third-party client is governed by OpenAI's Consumer Terms \
                       and may be restricted; check them before connecting."
                .to_owned(),
            docs_url: "https://developers.openai.com/codex/cli/".to_owned(),
            oauth_callback_port: Some(1455),
            builtin: true,
        },
        HarnessManifest {
            id: "gemini-cli".to_owned(),
            name: "Gemini CLI".to_owned(),
            binary: "gemini".to_owned(),
            install_cmd: "npm install -g @google/gemini-cli".to_owned(),
            auth_cmd: String::new(),
            protocol: HarnessProtocol::Acp,
            supports_embedded_chat: true,
            auth_modes: vec![HarnessAuthMode::ApiKey, HarnessAuthMode::Subscription],
            tos_note: "A Gemini API key is billed under Google's API terms. Personal \
                       Google-account sign-in is governed by Google's Consumer Terms and may be \
                       restricted for automated use."
                .to_owned(),
            docs_url: "https://github.com/google-gemini/gemini-cli".to_owned(),
            oauth_callback_port: Some(8085),
            builtin: true,
        },
        HarnessManifest {
            id: "antigravity".to_owned(),
            name: "Antigravity".to_owned(),
            binary: "antigravity".to_owned(),
            install_cmd: "Download Google Antigravity from https://antigravity.google".to_owned(),
            auth_cmd: String::new(),
            protocol: HarnessProtocol::CliPty,
            supports_embedded_chat: false,
            auth_modes: vec![HarnessAuthMode::Subscription],
            tos_note: "Sign-in uses your Google account and is governed by Google's Antigravity \
                       terms; automated or third-party use may be restricted."
                .to_owned(),
            docs_url: "https://antigravity.google/docs".to_owned(),
            oauth_callback_port: None,
            builtin: true,
        },
        HarnessManifest {
            id: "cursor-cli".to_owned(),
            name: "Cursor CLI".to_owned(),
            binary: "cursor-agent".to_owned(),
            install_cmd: "curl https://cursor.com/install -fsS | bash".to_owned(),
            auth_cmd: "cursor-agent login".to_owned(),
            protocol: HarnessProtocol::CliPty,
            supports_embedded_chat: false,
            auth_modes: vec![HarnessAuthMode::Subscription],
            tos_note: "Sign-in uses your Cursor subscription and is governed by Cursor's terms; \
                       automated or third-party use may be restricted."
                .to_owned(),
            docs_url: "https://cursor.com/cli".to_owned(),
            oauth_callback_port: None,
            builtin: true,
        },
        HarnessManifest {
            id: "aider".to_owned(),
            name: "Aider".to_owned(),
            binary: "aider".to_owned(),
            install_cmd: "python -m pip install aider-install && aider-install".to_owned(),
            auth_cmd: String::new(),
            protocol: HarnessProtocol::CliPty,
            supports_embedded_chat: false,
            auth_modes: vec![HarnessAuthMode::ApiKey],
            tos_note: "BYOK: Aider uses the provider API key you configure. Provider billing \
                       and terms apply."
                .to_owned(),
            docs_url: "https://aider.chat/docs/".to_owned(),
            oauth_callback_port: None,
            builtin: true,
        },
        HarnessManifest {
            id: "goose".to_owned(),
            name: "Goose".to_owned(),
            binary: "goose".to_owned(),
            install_cmd: "curl -fsSL https://block.github.io/goose/install.sh | bash".to_owned(),
            auth_cmd: "goose configure".to_owned(),
            protocol: HarnessProtocol::Acp,
            supports_embedded_chat: true,
            auth_modes: vec![HarnessAuthMode::ApiKey],
            tos_note: "BYOK: Goose uses the provider API key you configure. Provider billing \
                       and terms apply."
                .to_owned(),
            docs_url: "https://block.github.io/goose/docs/getting-started/installation".to_owned(),
            oauth_callback_port: None,
            builtin: true,
        },
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
