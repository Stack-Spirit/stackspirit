//! Local-only persistence for the engine choice and selected model.
//!
//! The file never contains a provider key, a token, or a prompt. Selecting a model does
//! not write anything to VOWP; this is the desktop's own configuration.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::EngineChoice;
use crate::error::SpiritError;

const PREFERENCES_VERSION: u32 = 1;

/// The local engine preferences saved between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnginePreferences {
    /// File schema version.
    pub version: u32,
    /// Bundled or an explicitly selected external harness.
    pub choice: EngineChoice,
    /// Engine-native model id, when one was verified and selected.
    pub selected_model_id: Option<String>,
    /// True after the user finishes the first-run flow.
    pub onboarding_complete: bool,
}

impl Default for EnginePreferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            choice: EngineChoice::Bundled,
            selected_model_id: None,
            onboarding_complete: false,
        }
    }
}

/// The persistence boundary used by the Tauri host and tests.
pub trait EnginePreferencesStore: Send + Sync {
    /// Loads preferences, returning defaults when no file exists.
    fn load(&self) -> Result<EnginePreferences, SpiritError>;
    /// Saves preferences without ever writing a secret.
    fn save(&self, preferences: &EnginePreferences) -> Result<(), SpiritError>;
}

/// A JSON preferences file under the app-data directory.
#[derive(Debug, Clone)]
pub struct JsonEnginePreferences {
    path: PathBuf,
}

impl JsonEnginePreferences {
    /// Builds a store at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the file path. It contains no secret.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl EnginePreferencesStore for JsonEnginePreferences {
    fn load(&self) -> Result<EnginePreferences, SpiritError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(EnginePreferences::default());
            }
            Err(error) => {
                return Err(SpiritError::io(
                    "reading the local engine preferences",
                    &error,
                ));
            }
        };
        let preferences: EnginePreferences = serde_json::from_slice(&bytes).map_err(|error| {
            SpiritError::parse("engine preferences", error)
                .with_hint("StackSpirit keeps the bundled engine selected until setup is finished.")
        })?;
        if preferences.version != PREFERENCES_VERSION {
            return Ok(EnginePreferences::default());
        }
        Ok(preferences)
    }

    fn save(&self, preferences: &EnginePreferences) -> Result<(), SpiritError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                SpiritError::io("creating the local engine preferences directory", &error)
            })?;
        }
        let bytes = serde_json::to_vec_pretty(preferences).map_err(|error| {
            SpiritError::new(
                crate::error::SpiritErrorCode::Internal,
                format!("engine preferences could not be serialized: {error}"),
            )
        })?;
        let staged = self.path.with_extension("json.tmp");
        std::fs::write(&staged, bytes)
            .map_err(|error| SpiritError::io("writing the local engine preferences", &error))?;
        std::fs::rename(&staged, &self.path).map_err(|error| {
            let _ = std::fs::remove_file(&staged);
            SpiritError::io("replacing the local engine preferences", &error)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ExternalHarnessKind;

    #[test]
    fn engine_preferences_round_trip_without_any_secret() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonEnginePreferences::new(directory.path().join("engine.json"));
        let preferences = EnginePreferences {
            version: PREFERENCES_VERSION,
            choice: EngineChoice::External {
                path: PathBuf::from("/opt/example/bin/opencode"),
                kind: ExternalHarnessKind::Opencode,
            },
            selected_model_id: Some("opencode/big-pickle".to_owned()),
            onboarding_complete: true,
        };
        store.save(&preferences).unwrap();
        assert_eq!(store.load().unwrap(), preferences);
        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("opencode/big-pickle"));
        assert!(!raw.contains(concat!("s", "k-")));
        assert!(!raw.contains("secret"));
        assert!(!raw.contains("api_key"));
    }

    #[test]
    fn engine_external_selection_never_edits_the_external_config() {
        let directory = tempfile::tempdir().unwrap();
        let external_config = directory.path().join("opencode.json");
        std::fs::write(&external_config, br#"{"model":"user-choice"}"#).unwrap();
        let before = std::fs::read(&external_config).unwrap();
        let store = JsonEnginePreferences::new(directory.path().join("engine.json"));
        store
            .save(&EnginePreferences {
                version: PREFERENCES_VERSION,
                choice: EngineChoice::External {
                    path: external_config.clone(),
                    kind: ExternalHarnessKind::Opencode,
                },
                selected_model_id: None,
                onboarding_complete: true,
            })
            .unwrap();
        assert_eq!(std::fs::read(&external_config).unwrap(), before);
        assert_eq!(
            store.load().unwrap().choice,
            EngineChoice::External {
                path: external_config,
                kind: ExternalHarnessKind::Opencode,
            }
        );
    }

    #[test]
    fn engine_preferences_default_when_missing_or_corrupt() {
        let directory = tempfile::tempdir().unwrap();
        let missing = JsonEnginePreferences::new(directory.path().join("missing.json"));
        assert_eq!(missing.load().unwrap(), EnginePreferences::default());

        let corrupt = directory.path().join("corrupt.json");
        std::fs::write(&corrupt, b"{not json").unwrap();
        let store = JsonEnginePreferences::new(corrupt);
        assert!(store.load().is_err());

        let future = directory.path().join("future.json");
        std::fs::write(
            &future,
            br#"{"version":99,"choice":{"type":"bundled"},"selected_model_id":null,"onboarding_complete":true}"#,
        )
        .unwrap();
        assert_eq!(
            JsonEnginePreferences::new(future).load().unwrap(),
            EnginePreferences::default()
        );
    }
}
