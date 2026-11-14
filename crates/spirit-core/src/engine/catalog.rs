//! The bundled included-model catalog and its runtime verification.

use serde::{Deserialize, Serialize};

use crate::engine::{ModelAccess, ModelChoice};
use crate::time::Timestamp;

const BUNDLED_CATALOG: &str = include_str!("../../../../engine/models.json");

/// One build-time catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Engine-native model id.
    pub id: String,
    /// Friendly label.
    pub label: String,
    /// Provider name.
    pub provider: String,
    /// Context window, when known.
    pub context_tokens: Option<u64>,
    /// Extra notes.
    pub notes: String,
}

/// The bundled catalog generated from the engine build.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalog {
    /// Catalog entries.
    pub entries: Vec<CatalogEntry>,
}

/// The result of checking the catalog against a running engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogVerification {
    /// Models present at runtime, marked `Included`.
    pub models: Vec<ModelChoice>,
    /// True when at least one included model is available.
    pub included_models_available: bool,
}

impl ModelCatalog {
    /// Builds a catalog from entries.
    #[must_use]
    pub fn new(entries: Vec<CatalogEntry>) -> Self {
        Self { entries }
    }

    /// Verifies the catalog against the engine's runtime model ids.
    ///
    /// A model is marked `Included` only when the runtime reports it. The catalog never
    /// claims a model is available from the build list alone.
    #[must_use]
    pub fn verify(&self, runtime_ids: &[String], verified_at: Timestamp) -> CatalogVerification {
        let models: Vec<ModelChoice> = self
            .entries
            .iter()
            .filter(|entry| runtime_ids.iter().any(|id| id == &entry.id))
            .map(|entry| ModelChoice {
                id: entry.id.clone(),
                label: entry.label.clone(),
                provider: entry.provider.clone(),
                access: ModelAccess::Included,
                context_tokens: entry.context_tokens,
                notes: entry.notes.clone(),
                verified_at: Some(verified_at),
            })
            .collect();
        CatalogVerification {
            included_models_available: !models.is_empty(),
            models,
        }
    }

    /// Returns the catalog entries as unverified `Included` choices.
    ///
    /// Callers must only display these after [`verify`](Self::verify) succeeds.
    #[must_use]
    pub fn unverified_choices(&self) -> Vec<ModelChoice> {
        self.entries
            .iter()
            .map(|entry| ModelChoice {
                id: entry.id.clone(),
                label: entry.label.clone(),
                provider: entry.provider.clone(),
                access: ModelAccess::Included,
                context_tokens: entry.context_tokens,
                notes: entry.notes.clone(),
                verified_at: None,
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct BundledCatalogDocument {
    entries: Vec<CatalogEntry>,
}

/// Loads the included-model catalog generated for the pinned engine build.
///
/// The file lists only models the engine reports as free. Every entry is still
/// unverified until the running engine reports its id.
#[must_use]
pub fn bundled_catalog() -> ModelCatalog {
    match serde_json::from_str::<BundledCatalogDocument>(BUNDLED_CATALOG) {
        Ok(document) => ModelCatalog::new(document.entries),
        Err(_) => ModelCatalog::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_bundled_catalog_is_unverified_until_the_runtime_reports_it() {
        let catalog = bundled_catalog();
        assert!(!catalog.entries.is_empty());
        let before = catalog.unverified_choices();
        assert!(before.iter().all(|model| model.verified_at.is_none()));
        let verification = catalog.verify(&[], Timestamp::now());
        assert!(!verification.included_models_available);
        assert!(verification.models.is_empty());

        let first = catalog.entries[0].id.clone();
        let verification = catalog.verify(std::slice::from_ref(&first), Timestamp::now());
        assert!(verification.included_models_available);
        assert_eq!(verification.models.len(), 1);
        assert_eq!(verification.models[0].id, first);
        assert!(verification.models[0].verified_at.is_some());
    }

    #[test]
    fn engine_bundled_catalog_keeps_implementation_branding_out_of_model_copy() {
        let catalog = bundled_catalog();
        assert!(
            catalog.entries.iter().all(|entry| {
                !entry.label.to_ascii_lowercase().contains("opencode")
                    && !entry.notes.to_ascii_lowercase().contains("opencode")
            }),
            "the included-model list must not show engine implementation branding"
        );
    }

    #[test]
    fn engine_bundled_catalog_contains_no_secret_values() {
        let source = BUNDLED_CATALOG;
        assert!(!source.contains(concat!("s", "k-")));
        assert!(!source.contains(concat!("g", "hp_")));
        assert!(!source.contains("api_key"));
        let catalog = bundled_catalog();
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| entry.provider == "opencode")
        );
    }
}
