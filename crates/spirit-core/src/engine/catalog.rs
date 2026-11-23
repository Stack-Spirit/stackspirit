//! The bundled included-model catalog and its runtime verification.

use serde::{Deserialize, Serialize};

use crate::engine::{ModelAccess, ModelChoice, RuntimeModel};
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
    /// Models present at runtime. Only free runtime models are marked `Included`.
    pub models: Vec<ModelChoice>,
    /// True when at least one runtime model is still free.
    pub included_models_available: bool,
}

impl ModelCatalog {
    /// Builds a catalog from entries.
    #[must_use]
    pub fn new(entries: Vec<CatalogEntry>) -> Self {
        Self { entries }
    }

    /// Verifies the catalog against raw runtime model ids.
    ///
    /// Id-only callers cannot know whether a model still needs a key, so this method is
    /// retained only for fixture compatibility. Runtime adapters must call
    /// [`verify_runtime`](Self::verify_runtime), which honours the engine's free flag.
    #[must_use]
    pub fn verify(&self, runtime_ids: &[String], verified_at: Timestamp) -> CatalogVerification {
        let models = runtime_ids
            .iter()
            .map(|id| RuntimeModel {
                id: id.clone(),
                provider: String::new(),
                label: String::new(),
                context_tokens: None,
                free: true,
                variants: Vec::new(),
            })
            .collect::<Vec<_>>();
        self.verify_runtime(&models, verified_at)
    }

    /// Verifies the catalog against the engine's runtime models.
    ///
    /// A model is shown only when the runtime reports it. It is marked `Included` only
    /// when the runtime also reports that it needs no user key; a catalog entry the
    /// engine now reports as paid becomes `Byok` and never claims `no key`.
    #[must_use]
    pub fn verify_runtime(
        &self,
        runtime_models: &[RuntimeModel],
        verified_at: Timestamp,
    ) -> CatalogVerification {
        let models: Vec<ModelChoice> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let runtime = runtime_models.iter().find(|model| model.id == entry.id)?;
                Some(ModelChoice {
                    id: entry.id.clone(),
                    label: entry.label.clone(),
                    provider: entry.provider.clone(),
                    access: if runtime.free {
                        ModelAccess::Included
                    } else {
                        ModelAccess::Byok
                    },
                    context_tokens: entry.context_tokens,
                    notes: entry.notes.clone(),
                    verified_at: Some(verified_at),
                })
            })
            .collect();
        CatalogVerification {
            included_models_available: models
                .iter()
                .any(|model| model.access == ModelAccess::Included),
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

    fn runtime_model(entry: &CatalogEntry, free: bool) -> RuntimeModel {
        RuntimeModel {
            id: entry.id.clone(),
            provider: entry.provider.clone(),
            label: entry.label.clone(),
            context_tokens: entry.context_tokens,
            free,
            variants: Vec::new(),
        }
    }

    #[test]
    fn engine_bundled_catalog_is_unverified_until_the_runtime_reports_it() {
        let catalog = bundled_catalog();
        assert!(!catalog.entries.is_empty());
        let before = catalog.unverified_choices();
        assert!(before.iter().all(|model| model.verified_at.is_none()));
        let verification = catalog.verify_runtime(&[], Timestamp::now());
        assert!(!verification.included_models_available);
        assert!(verification.models.is_empty());

        let first = &catalog.entries[0];
        let verification = catalog.verify_runtime(&[runtime_model(first, true)], Timestamp::now());
        assert!(verification.included_models_available);
        assert_eq!(verification.models.len(), 1);
        assert_eq!(verification.models[0].id, first.id);
        assert_eq!(verification.models[0].access, ModelAccess::Included);
        assert!(verification.models[0].verified_at.is_some());
    }

    #[test]
    fn engine_bundled_catalog_marks_a_now_paid_model_as_byok() {
        let catalog = bundled_catalog();
        let first = &catalog.entries[0];
        let verification = catalog.verify_runtime(&[runtime_model(first, false)], Timestamp::now());
        assert!(!verification.included_models_available);
        assert_eq!(verification.models.len(), 1);
        assert_eq!(verification.models[0].id, first.id);
        assert_eq!(verification.models[0].access, ModelAccess::Byok);
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
