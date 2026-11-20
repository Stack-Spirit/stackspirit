//! The provider catalog reported by the running engine.
//!
//! `/provider` returns the full supported-provider catalog plus the ids the engine is
//! currently connected to. `/config/providers` returns only the configured providers.
//! This module keeps the two facts together, orders connected and popular providers
//! first, and never exposes provider configuration, environment values or keys.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::EngineProvider;

/// The provider order the connect dialog uses. `opencode` is StackSpirit's bundled
/// free provider; `opencode-go` is its paid Go/Zen account option. The rest mirror
/// OpenCode's own popular-first list.
pub const POPULAR_PROVIDER_IDS: [&str; 8] = [
    "opencode",
    "opencode-go",
    "anthropic",
    "github-copilot",
    "openai",
    "google",
    "openrouter",
    "vercel",
];

/// A deduplicated, grouped provider catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCatalog {
    /// Providers in display order: connected first, then the popular order, then label.
    pub providers: Vec<EngineProvider>,
}

impl ProviderCatalog {
    /// Builds a grouped catalog from parsed provider rows.
    #[must_use]
    pub fn new(mut providers: Vec<EngineProvider>) -> Self {
        providers.sort_by(|left, right| {
            right
                .connected
                .cmp(&left.connected)
                .then_with(|| provider_rank(&left.id).cmp(&provider_rank(&right.id)))
                .then_with(|| {
                    left.label
                        .to_ascii_lowercase()
                        .cmp(&right.label.to_ascii_lowercase())
                })
                .then_with(|| left.id.cmp(&right.id))
        });
        providers.dedup_by(|left, right| left.id == right.id);
        Self { providers }
    }

    /// True when no provider row was parsed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Number of provider rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Connected providers in display order.
    #[must_use]
    pub fn connected(&self) -> Vec<&EngineProvider> {
        self.providers
            .iter()
            .filter(|provider| provider.connected)
            .collect()
    }

    /// Providers that are in the catalog but not connected yet, in display order.
    #[must_use]
    pub fn available(&self) -> Vec<&EngineProvider> {
        self.providers
            .iter()
            .filter(|provider| !provider.connected)
            .collect()
    }

    /// The bundled free provider, when the engine reports it.
    #[must_use]
    pub fn opencode(&self) -> Option<&EngineProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id == "opencode")
    }

    /// Case-insensitive search across the provider id and display label.
    ///
    /// An empty or whitespace-only query returns every provider in display order.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&EngineProvider> {
        let query = query.trim().to_ascii_lowercase();
        self.providers
            .iter()
            .filter(|provider| {
                query.is_empty()
                    || provider.id.to_ascii_lowercase().contains(&query)
                    || provider.label.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }
}

/// Returns the popular-list position for a provider id, or `usize::MAX` when it is
/// not one of the providers the UI highlights.
#[must_use]
pub fn provider_rank(id: &str) -> usize {
    POPULAR_PROVIDER_IDS
        .iter()
        .position(|candidate| *candidate == id)
        .unwrap_or(usize::MAX)
}

/// Parses `/provider` or `/config/providers` into a grouped catalog.
///
/// `/provider` carries a `connected` id array; `/config/providers` is itself the
/// configured list, so its rows are connected. Only ids, display labels, the engine
/// source class, model counts and free-model counts are retained.
#[must_use]
pub fn parse_provider_catalog(body: &[u8]) -> ProviderCatalog {
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return ProviderCatalog::default();
    };
    let connected = document
        .get("connected")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
        });
    let config_shaped = document.get("all").is_none() && document.get("providers").is_some();
    let mut providers = Vec::new();
    for key in ["all", "providers"] {
        match document.get(key) {
            Some(Value::Array(entries)) => {
                for entry in entries {
                    if let Some(provider) =
                        provider_from_value(entry, None, connected.as_ref(), config_shaped)
                    {
                        providers.push(provider);
                    }
                }
            }
            Some(Value::Object(entries)) => {
                for (id, entry) in entries {
                    if let Some(provider) =
                        provider_from_value(entry, Some(id), connected.as_ref(), config_shaped)
                    {
                        providers.push(provider);
                    }
                }
            }
            _ => {}
        }
    }
    ProviderCatalog::new(providers)
}

fn provider_from_value(
    entry: &Value,
    id_hint: Option<&String>,
    connected: Option<&HashSet<&str>>,
    config_shaped: bool,
) -> Option<EngineProvider> {
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .or(id_hint.map(String::as_str))?
        .trim();
    if id.is_empty() {
        return None;
    }
    let label = entry
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| entry.get("label").and_then(Value::as_str))
        .unwrap_or(id)
        .trim()
        .to_owned();
    let source = entry
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_owned);
    let (model_count, free_model_count) = model_counts(entry.get("models"));
    Some(EngineProvider {
        id: id.to_owned(),
        label: if label.is_empty() {
            id.to_owned()
        } else {
            label
        },
        connected: connected
            .map(|connected| connected.contains(id))
            .unwrap_or(config_shaped),
        source,
        model_count,
        free_model_count,
    })
}

fn model_counts(models: Option<&Value>) -> (usize, usize) {
    match models {
        Some(Value::Object(entries)) => {
            let mut free = 0;
            for model in entries.values() {
                if model_input_cost(model) == Some(0.0) {
                    free += 1;
                }
            }
            (entries.len(), free)
        }
        Some(Value::Array(entries)) => {
            let free = entries
                .iter()
                .filter(|model| model_input_cost(model) == Some(0.0))
                .count();
            (entries.len(), free)
        }
        _ => (0, 0),
    }
}

fn model_input_cost(model: &Value) -> Option<f64> {
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
    fn provider_catalog_groups_connected_first_and_keeps_opencode_popular() {
        let body = br#"{
          "connected": ["opencode", "deepseek"],
          "all": [
            {"id": "zed", "name": "Zed", "models": {}},
            {"id": "openai", "name": "OpenAI", "models": {}},
            {"id": "deepseek", "name": "DeepSeek", "models": {"chat": {"cost": {"input": 1}}}},
            {"id": "opencode", "name": "OpenCode", "models": {
              "free-a": {"cost": {"input": 0}},
              "paid-a": {"cost": {"input": 2}}
            }},
            {"id": "anthropic", "name": "Anthropic", "models": {}}
          ]
        }"#;
        let catalog = parse_provider_catalog(body);
        let ids: Vec<&str> = catalog
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect();
        assert_eq!(ids, ["opencode", "deepseek", "anthropic", "openai", "zed"]);
        assert_eq!(catalog.connected().len(), 2);
        assert_eq!(catalog.available().len(), 3);
        let opencode = catalog.opencode().unwrap();
        assert_eq!(opencode.free_model_count, 1);
        assert_eq!(opencode.model_count, 2);
        assert_eq!(catalog.search("DEEP").len(), 1);
    }

    #[test]
    fn config_providers_are_all_connected_and_object_maps_are_accepted() {
        let body = br#"{
          "providers": {
            "groq": {"name": "Groq", "models": []},
            "opencode": {"name": "OpenCode", "models": []}
          }
        }"#;
        let catalog = parse_provider_catalog(body);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.connected().len(), 2);
        assert!(
            catalog
                .connected()
                .iter()
                .any(|provider| provider.id == "groq")
        );
    }
}
