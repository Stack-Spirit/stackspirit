//! Executor parsing for display. Keeps the exact VOWP string and never normalises a
//! malformed value.

use serde::{Deserialize, Serialize};

use crate::wire::executor::ExecutorRef;

/// The UI-facing executor badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorView {
    /// Exact VOWP executor string.
    pub raw: String,
    /// `claude-code`, `opencode`, `cli`, `acp`, `vowp`, or `unknown`.
    pub harness: String,
    /// Provider key, for example `deepseek` or `claude`.
    pub provider: String,
    /// Model name as the harness knows it.
    pub model: String,
    /// `claude-code` effort, when present.
    pub effort: Option<String>,
    /// `opencode` variant, when present.
    pub variant: Option<String>,
    /// Friendly label when known, otherwise the raw model id or the raw string.
    pub display: String,
}

impl ExecutorView {
    /// Parses one executor string into a display model.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let parsed = ExecutorRef::parse(raw);
        if !parsed.is_known() {
            return Self {
                raw: raw.to_owned(),
                harness: "unknown".to_owned(),
                provider: String::new(),
                model: String::new(),
                effort: None,
                variant: None,
                display: raw.to_owned(),
            };
        }
        let model = parsed.model.clone().unwrap_or_default();
        let mut display = friendly_model_name(&model);
        if let Some(variant) = &parsed.variant {
            display.push_str(" · ");
            display.push_str(variant);
        }
        if let Some(effort) = &parsed.effort {
            display.push_str(" · ");
            display.push_str(effort);
        }
        Self {
            raw: raw.to_owned(),
            harness: parsed.harness_str(),
            provider: parsed.provider.unwrap_or_default(),
            model,
            effort: parsed.effort,
            variant: parsed.variant,
            display,
        }
    }

    /// Builds the shell placeholder for a null executor.
    #[must_use]
    pub fn shell() -> Self {
        Self {
            raw: String::new(),
            harness: "shell".to_owned(),
            provider: String::new(),
            model: String::new(),
            effort: None,
            variant: None,
            display: "shell".to_owned(),
        }
    }
}

/// Friendly provider name: `claude`/`anthropic` are both Anthropic, otherwise a known
/// brand or the capitalised raw key.
#[must_use]
pub fn provider_label(provider: &str) -> String {
    match provider.to_ascii_lowercase().as_str() {
        "" => String::new(),
        "claude" | "anthropic" => "Anthropic".to_owned(),
        "deepseek" => "DeepSeek".to_owned(),
        "openrouter" => "OpenRouter".to_owned(),
        "openai" => "OpenAI".to_owned(),
        "google" | "gemini" => "Google".to_owned(),
        "opencode" => "OpenCode".to_owned(),
        _ => {
            let mut chars = provider.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        }
    }
}

/// Friendly harness name, or the raw value when unknown.
#[must_use]
pub fn harness_label(harness: &str) -> String {
    match harness {
        "claude-code" => "Claude Code".to_owned(),
        "opencode" => "OpenCode".to_owned(),
        "cli" => "CLI".to_owned(),
        "acp" => "ACP".to_owned(),
        other => other.to_owned(),
    }
}

impl ExecutorView {
    /// Friendly provider label for this executor.
    #[must_use]
    pub fn provider_label(&self) -> String {
        provider_label(&self.provider)
    }

    /// Friendly harness label for this executor.
    #[must_use]
    pub fn harness_label(&self) -> String {
        harness_label(&self.harness)
    }
}

/// Returns a friendly label for a few common models, otherwise the raw model id.
#[must_use]
pub fn friendly_model_name(model: &str) -> String {
    match model {
        "deepseek/deepseek-flash" => "DeepSeek V4.1 Flash".to_owned(),
        "deepseek/deepseek-chat" => "DeepSeek Chat".to_owned(),
        "anthropic/claude-sonnet-4" => "Claude Sonnet 4".to_owned(),
        "anthropic/claude-opus-4" => "Claude Opus 4".to_owned(),
        "openai/gpt-5" => "GPT-5".to_owned(),
        "claude-sonnet-5-5" => "Claude Sonnet 5.5".to_owned(),
        "claude-opus-5-5" => "Claude Opus 5.5".to_owned(),
        "claude-haiku-5-5" => "Claude Haiku 5.5".to_owned(),
        "claude-fable-5-1" => "Claude Fable 5.1".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_executor_has_friendly_fields() {
        let view = ExecutorView::parse("claude-code:claude-sonnet-5-5@medium");
        assert_eq!(view.harness, "claude-code");
        assert_eq!(view.provider, "claude");
        assert_eq!(view.display, "Claude Sonnet 5.5 · medium");
        assert_eq!(view.harness_label(), "Claude Code");
        assert_eq!(view.provider_label(), "Anthropic");
    }

    #[test]
    fn opencode_executor_has_friendly_fields() {
        let view = ExecutorView::parse("opencode:deepseek/deepseek-flash#max");
        assert_eq!(view.harness, "opencode");
        assert_eq!(view.provider, "deepseek");
        assert_eq!(view.display, "DeepSeek V4.1 Flash · max");
        assert_eq!(view.harness_label(), "OpenCode");
        assert_eq!(view.provider_label(), "DeepSeek");
    }

    #[test]
    fn labels_fall_back() {
        assert_eq!(provider_label("mistral"), "Mistral");
        assert_eq!(provider_label("anthropic"), "Anthropic");
        assert_eq!(harness_label("weird"), "weird");
        assert_eq!(harness_label("acp"), "ACP");
    }
}
