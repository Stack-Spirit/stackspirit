//! The bundled included-model catalog and its runtime verification.

use serde::{Deserialize, Serialize};

use crate::engine::{ModelAccess, ModelChoice};
use crate::time::Timestamp;

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
