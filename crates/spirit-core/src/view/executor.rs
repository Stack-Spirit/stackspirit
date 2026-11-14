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

/// Returns a friendly label for a few common models, otherwise the raw model id.
#[must_use]
pub fn friendly_model_name(model: &str) -> String {
    match model {
        "deepseek/deepseek-flash" => "DeepSeek V4.1 Flash".to_owned(),
        "deepseek/deepseek-chat" => "DeepSeek Chat".to_owned(),
        "anthropic/claude-sonnet-4" => "Claude Sonnet 4".to_owned(),
        "anthropic/claude-opus-4" => "Claude Opus 4".to_owned(),
        "openai/gpt-5" => "GPT-5".to_owned(),
        other => other.to_owned(),
    }
}
