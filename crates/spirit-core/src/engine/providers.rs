//! The provider catalog reported by the running engine.
//!
//! `/provider` returns the full supported-provider catalog plus the ids the engine is
//! currently connected to. `/config/providers` returns only the configured providers.
//! This module keeps the connection fact, the engine source class and model counts
//! together, orders connected and popular providers first, and never exposes provider
//! configuration, environment values or keys.
//!
//! Each row crosses the bridge as an `EngineProvider`: id, label, the engine's
//! connected fact, source class and model counts. Nothing else leaves the core.

use std::collections::HashSet;

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

/// One provider row from the engine's catalog.
///
/// The label is safe to display. `source` is the engine's own source class (`api`,
/// `env`, `config`, `custom`), never a configuration value. Counts describe how many
/// models the engine reports and how many of them cost nothing to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEntry {
    /// Engine-native provider id.
    pub id: String,
    /// Display label reported by the engine, or the id when it has no name.
    pub label: String,
    /// True when the engine lists this provider as connected, or when this is the
    /// configured-provider document itself.
    pub connected: bool,
    /// Engine source class (`api`, `env`, `config`, `custom`), when reported.
    pub source: Option<String>,
    /// Number of models the engine reports for this provider.
    pub model_count: usize,
    /// Number of those models whose input cost is zero.
    pub free_model_count: usize,
}

/// A deduplicated, grouped provider catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderCatalog {
    /// Providers in display order: connected first, then the popular order, then label.
    pub providers: Vec<ProviderEntry>,
}

impl ProviderCatalog {
    /// Builds a grouped catalog from parsed provider rows.
    #[must_use]
    pub fn new(mut providers: Vec<ProviderEntry>) -> Self {
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
    pub fn connected(&self) -> Vec<&ProviderEntry> {
        self.providers
            .iter()
            .filter(|provider| provider.connected)
            .collect()
    }

    /// Providers that are in the catalog but not connected yet, in display order.
    #[must_use]
    pub fn available(&self) -> Vec<&ProviderEntry> {
        self.providers
            .iter()
            .filter(|provider| !provider.connected)
            .collect()
    }

    /// The bundled free provider, when the engine reports it.
    #[must_use]
    pub fn opencode(&self) -> Option<&ProviderEntry> {
        self.providers
            .iter()
            .find(|provider| provider.id == "opencode")
    }

    /// Case-insensitive search across the provider id and display label.
    ///
    /// An empty or whitespace-only query returns every provider in display order.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&ProviderEntry> {
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

    /// The display-ordered rows the bridge's `EngineProvider` carries.
    #[must_use]
    pub fn engine_providers(&self) -> Vec<EngineProvider> {
        self.providers
            .iter()
            .map(|provider| EngineProvider {
                id: provider.id.clone(),
                label: provider.label.clone(),
                connected: provider.connected,
                source: provider.source.clone(),
                model_count: provider.model_count,
                free_model_count: provider.free_model_count,
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
) -> Option<ProviderEntry> {
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
    Some(ProviderEntry {
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
        let bridged = catalog.engine_providers();
        assert_eq!(bridged[0].id, "opencode");
        assert_eq!(bridged[0].label, "OpenCode");
        assert!(bridged[0].connected);
        assert_eq!(bridged[0].model_count, 2);
        assert_eq!(bridged[0].free_model_count, 1);
        assert!(!bridged.iter().find(|p| p.id == "openai").unwrap().connected);
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

    #[test]
    fn an_unconnected_duplicate_never_replaces_a_connected_row() {
        let body = br#"{
          "connected": ["deepseek"],
          "all": [
            {"id": "deepseek", "name": "DeepSeek API"},
            {"id": "deepseek", "name": "DeepSeek Copy"}
          ]
        }"#;
        let catalog = parse_provider_catalog(body);
        assert_eq!(catalog.len(), 1);
        assert!(catalog.providers[0].connected);
    }

    #[test]
    fn malformed_documents_yield_an_empty_catalog() {
        assert!(parse_provider_catalog(b"not json").is_empty());
        assert!(parse_provider_catalog(br#"{"all": "nope"}"#).is_empty());
    }
}
