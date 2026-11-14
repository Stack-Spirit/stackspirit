//! The canonical executor grammar `harness:model[@effort][#variant]`.
//!
//! Parsing is pure and never normalises a malformed string: a bad value keeps `raw` and
//! reports no harness, so the UI can show an **Unknown harness** chip.

use crate::wire::enums::HarnessKind;

/// A parsed executor reference. Unknown or malformed values are preserved raw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorRef {
    /// The exact VOWP executor string.
    pub raw: String,
    /// Parsed harness, or `None` when the string is malformed or unknown.
    pub harness: Option<HarnessKind>,
    /// Provider key, matching VOWP's own `Executor::provider()`.
    pub provider: Option<String>,
    /// Model name as the harness knows it.
    pub model: Option<String>,
    /// `claude-code` effort, when present.
    pub effort: Option<String>,
    /// `opencode` variant, when present.
    pub variant: Option<String>,
}

impl ExecutorRef {
    /// Parses a canonical executor string.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut parsed = Self {
            raw: raw.to_owned(),
            harness: None,
            provider: None,
            model: None,
            effort: None,
            variant: None,
        };
        let Some((harness_raw, rest)) = raw.split_once(':') else {
            return parsed;
        };
        let harness = HarnessKind::parse(harness_raw);
        if !harness.is_known() {
            return parsed;
        }
        let Some((model_part, variant)) = split_once_char(rest, '#') else {
            return parsed;
        };
        let Some((model, effort)) = split_once_char(model_part, '@') else {
            return parsed;
        };
        if model.is_empty()
            || !model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
        {
            return parsed;
        }
        if let Some(effort) = effort {
            if !is_modifier(effort) || harness != HarnessKind::ClaudeCode {
                return parsed;
            }
        }
        if let Some(variant) = variant {
            if !is_modifier(variant) || harness != HarnessKind::Opencode {
                return parsed;
            }
        }
        let provider = match harness {
            HarnessKind::ClaudeCode => "claude".to_owned(),
            HarnessKind::Opencode => model.split('/').next().unwrap_or(model).to_owned(),
            HarnessKind::Cli | HarnessKind::Acp | HarnessKind::Vowp => model.to_owned(),
            HarnessKind::Unknown(_) => return parsed,
        };
        parsed.harness = Some(harness);
        parsed.provider = Some(provider);
        parsed.model = Some(model.to_owned());
        parsed.effort = effort.map(str::to_owned);
        parsed.variant = variant.map(str::to_owned);
        parsed
    }

    /// Returns true when the executor parsed fully.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        self.harness.is_some()
    }

    /// Returns the harness wire string, or `unknown`.
    #[must_use]
    pub fn harness_str(&self) -> String {
        self.harness
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string)
    }
}

fn split_once_char(input: &str, needle: char) -> Option<(&str, Option<&str>)> {
    if let Some(index) = input.find(needle) {
        let (left, right) = input.split_at(index);
        let right = right.strip_prefix(needle).unwrap_or(right);
        if right.contains(needle) {
            return None;
        }
        Some((left, Some(right)))
    } else {
        Some((input, None))
    }
}

pub(crate) fn is_modifier(input: &str) -> bool {
    !input.is_empty()
        && input
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
}
