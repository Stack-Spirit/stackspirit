//! Persisted default harness for app-owned embedded chat.
//!
//! This is the Phase 1a selection half of `docs/HARNESS-CHAT.md` §3: one global default
//! for new app-owned local conversations, stored in a small versioned JSON file. The file
//! never carries a key, token, endpoint, port, binary path, native session id or
//! workspace. A session captures `{harness_id, protocol}` in memory; this module adds no
//! durable binding file.
//!
//! Load rules are deliberately non-destructive: a missing file or an absent/null id is
//! the documented legacy default `opencode`; a corrupt file or a future schema version is
//! a typed [`ChatSettingsError`] and is never rewritten behind the owner's back. An
//! unknown id is returned to the caller for the chat gate to classify; it is never
//! replaced silently.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::{DEFAULT_HARNESS_ID, HarnessProtocol};

/// Current `chat.json` schema version.
pub const CHAT_PREFERENCES_VERSION: u32 = 1;

/// The persisted chat selection.
///
/// The schema has exactly two fields. It has no key, token, endpoint, port, binary path,
/// native session id or workspace, and unknown fields are rejected on load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatPreferences {
    /// File schema version.
    pub version: u32,
    /// Registry id of the default harness, or `None` for the documented OpenCode default.
    #[serde(default)]
    pub default_harness_id: Option<String>,
}

impl Default for ChatPreferences {
    fn default() -> Self {
        Self {
            version: CHAT_PREFERENCES_VERSION,
            default_harness_id: None,
        }
    }
}

impl ChatPreferences {
    /// Returns the selected registry id, falling back only to the documented legacy
    /// default. An unknown id is returned exactly as stored; it is not replaced.
    #[must_use]
    pub fn selected_harness_id(&self) -> &str {
        self.default_harness_id
            .as_deref()
            .unwrap_or(DEFAULT_HARNESS_ID)
    }
}

/// Why `chat.json` cannot be used.
///
/// The type carries no raw parser text, path or file contents. It serializes to a stable
/// safe reason token for the gate/UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ChatSettingsError {
    /// The file exists but could not be read, decoded or validated.
    Unreadable,
    /// The file's schema version is not supported by this build.
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Version this build supports.
        supported: u32,
    },
}

impl ChatSettingsError {
    /// Returns the stable safe reason token.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Unreadable => "chat_settings_unreadable",
            Self::UnsupportedVersion { .. } => "chat_settings_unsupported_version",
        }
    }

    /// Returns the unsupported version, when the file was readable enough to name one.
    #[must_use]
    pub const fn found_version(self) -> Option<u32> {
        match self {
            Self::Unreadable => None,
            Self::UnsupportedVersion { found, .. } => Some(found),
        }
    }

    /// Maps the problem to a safe error card.
    ///
    /// The message carries no path, raw JSON, token or file contents.
    #[must_use]
    pub fn spirit(self) -> SpiritError {
        match self {
            Self::Unreadable => SpiritError::new(
                SpiritErrorCode::Parse,
                "chat_settings_unreadable: the chat settings file could not be read",
            )
            .with_hint("Fix or remove chat.json, then choose a chat transport explicitly."),
            Self::UnsupportedVersion { found, supported } => SpiritError::new(
                SpiritErrorCode::Gate,
                format!(
                    "chat_settings_unreadable: chat settings version {found} is not supported \
                     by this build (version {supported})"
                ),
            )
            .with_hint(
                "Use a StackSpirit build that understands this settings version; the file was \
                 not changed.",
            ),
        }
    }
}

impl fmt::Display for ChatSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str(self.reason()),
            Self::UnsupportedVersion { found, supported } => {
                write!(f, "{}: {found} (supported {supported})", self.reason())
            }
        }
    }
}

impl std::error::Error for ChatSettingsError {}

/// Resolves the persisted chat selection for the gate.
///
/// Implemented by an already-loaded [`ChatPreferences`] and by a [`JsonChatPreferences`]
/// store, so `preflight` can be called with either without changing its return type.
pub trait ChatPreferencesSource {
    /// Resolves the selection. A missing file is the documented default, never an error.
    fn resolve(&self) -> Result<ChatPreferences, ChatSettingsError>;
}

impl ChatPreferencesSource for ChatPreferences {
    fn resolve(&self) -> Result<ChatPreferences, ChatSettingsError> {
        Ok(self.clone())
    }
}

/// The persistence boundary used by the desktop host and tests.
pub trait ChatPreferencesStore: Send + Sync {
    /// Loads preferences. A missing file is the OpenCode default; a corrupt or future
    /// file is a typed problem and is not rewritten.
    fn load(&self) -> Result<ChatPreferences, ChatSettingsError>;
    /// Saves preferences atomically without ever writing a secret.
    fn save(&self, preferences: &ChatPreferences) -> Result<(), SpiritError>;
}

/// A JSON chat settings file under the app-data directory.
#[derive(Debug, Clone)]
pub struct JsonChatPreferences {
    path: PathBuf,
}

impl JsonChatPreferences {
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

impl ChatPreferencesStore for JsonChatPreferences {
    fn load(&self) -> Result<ChatPreferences, ChatSettingsError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ChatPreferences::default());
            }
            Err(_) => return Err(ChatSettingsError::Unreadable),
        };
        let preferences: ChatPreferences =
            serde_json::from_slice(&bytes).map_err(|_| ChatSettingsError::Unreadable)?;
        if preferences.version != CHAT_PREFERENCES_VERSION {
            return Err(ChatSettingsError::UnsupportedVersion {
                found: preferences.version,
                supported: CHAT_PREFERENCES_VERSION,
            });
        }
        Ok(preferences)
    }

    fn save(&self, preferences: &ChatPreferences) -> Result<(), SpiritError> {
        if preferences.version != CHAT_PREFERENCES_VERSION {
            return Err(SpiritError::invalid_input(
                "version",
                format!("must be {CHAT_PREFERENCES_VERSION}"),
            ));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| SpiritError::io("creating the chat settings directory", &error))?;
            restrict_directory(parent);
        }
        let bytes = serde_json::to_vec_pretty(preferences).map_err(|error| {
            SpiritError::new(
                SpiritErrorCode::Internal,
                format!("chat settings could not be serialized: {error}"),
            )
        })?;
        let staged = self.path.with_extension("json.tmp");
        write_private(&staged, &bytes)?;
        fs::rename(&staged, &self.path).map_err(|error| {
            let _ = fs::remove_file(&staged);
            SpiritError::io("replacing the chat settings", &error)
        })
    }
}

impl ChatPreferencesSource for JsonChatPreferences {
    fn resolve(&self) -> Result<ChatPreferences, ChatSettingsError> {
        <Self as ChatPreferencesStore>::load(self)
    }
}

/// The immutable in-memory harness binding captured by one chat session.
///
/// The capture carries only the registry id and protocol. It never carries a native
/// session id, endpoint, port, binary path, workspace or credential, and this module
/// does not persist it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHarnessCapture {
    harness_id: String,
    protocol: HarnessProtocol,
}

impl ChatHarnessCapture {
    /// Builds a capture from an already-resolved harness id and protocol.
    #[must_use]
    pub fn new(harness_id: impl Into<String>, protocol: HarnessProtocol) -> Self {
        Self {
            harness_id: harness_id.into(),
            protocol,
        }
    }

    /// Returns the registry id.
    #[must_use]
    pub fn harness_id(&self) -> &str {
        &self.harness_id
    }

    /// Returns the protocol captured with the id.
    #[must_use]
    pub const fn protocol(&self) -> HarnessProtocol {
        self.protocol
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SpiritError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options
        .open(path)
        .map_err(|error| SpiritError::io("creating the chat settings", &error))?;
    handle
        .write_all(bytes)
        .map_err(|error| SpiritError::io("writing the chat settings", &error))?;
    handle
        .sync_all()
        .map_err(|error| SpiritError::io("syncing the chat settings", &error))?;
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences(id: Option<&str>) -> ChatPreferences {
        ChatPreferences {
            version: CHAT_PREFERENCES_VERSION,
            default_harness_id: id.map(str::to_owned),
        }
    }

    #[test]
    fn preferences_round_trip_without_any_secret() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonChatPreferences::new(directory.path().join("chat.json"));
        let saved = preferences(Some("goose"));
        store.save(&saved).unwrap();

        assert_eq!(store.load().unwrap(), saved);
        assert_eq!(store.load().unwrap().selected_harness_id(), "goose");
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("\"default_harness_id\": \"goose\""));
        assert!(!raw.contains("api_key"));
        assert!(!raw.contains("token"));
        assert!(!raw.contains("endpoint"));
        assert!(!raw.contains("workspace"));
    }

    #[test]
    fn unknown_id_is_returned_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonChatPreferences::new(directory.path().join("chat.json"));
        store.save(&preferences(Some("future-harness"))).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.default_harness_id.as_deref(),
            Some("future-harness"),
            "the store must not silently replace an unknown id"
        );
        assert_eq!(loaded.selected_harness_id(), "future-harness");
    }

    #[test]
    fn missing_absent_and_null_ids_select_opencode() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonChatPreferences::new(directory.path().join("chat.json"));

        assert_eq!(store.load().unwrap(), ChatPreferences::default());
        assert_eq!(store.load().unwrap().selected_harness_id(), "opencode");

        fs::write(store.path(), br#"{"version":1,"default_harness_id":null}"#).unwrap();
        assert_eq!(store.load().unwrap().selected_harness_id(), "opencode");

        fs::write(store.path(), br#"{"version":1}"#).unwrap();
        assert_eq!(store.load().unwrap().selected_harness_id(), "opencode");

        store.save(&preferences(Some("opencode"))).unwrap();
        assert_eq!(store.load().unwrap().selected_harness_id(), "opencode");
    }

    #[test]
    fn corrupt_and_future_settings_are_typed_and_never_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonChatPreferences::new(directory.path().join("chat.json"));

        for corrupt in [
            br#"{not json"#.as_slice(),
            br#"{"version":1,"default_harness_id":null,"api_key":"not-stored"}"#.as_slice(),
        ] {
            fs::write(store.path(), corrupt).unwrap();
            assert_eq!(store.load(), Err(ChatSettingsError::Unreadable));
            assert_eq!(fs::read(store.path()).unwrap(), corrupt);
        }

        let future = br#"{"version":99,"default_harness_id":"opencode"}"#;
        fs::write(store.path(), future).unwrap();
        let expected = ChatSettingsError::UnsupportedVersion {
            found: 99,
            supported: CHAT_PREFERENCES_VERSION,
        };
        assert_eq!(store.load(), Err(expected));
        assert_eq!(expected.found_version(), Some(99));
        assert_eq!(fs::read(store.path()).unwrap(), future);
    }

    #[test]
    fn save_replaces_atomically_and_removes_the_staged_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chat.json");
        let store = JsonChatPreferences::new(&path);

        store.save(&preferences(Some("opencode"))).unwrap();
        store.save(&preferences(Some("goose"))).unwrap();

        assert_eq!(store.load().unwrap(), preferences(Some("goose")));
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let entries: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "only chat.json remains: {entries:?}");
    }

    #[test]
    fn save_refuses_a_foreign_schema_version() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonChatPreferences::new(directory.path().join("chat.json"));
        let mut future = preferences(None);
        future.version = CHAT_PREFERENCES_VERSION + 1;

        let error = store.save(&future).unwrap_err();
        assert_eq!(error.code, SpiritErrorCode::InvalidInput);
        assert!(!store.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn save_creates_owner_only_file_and_directory() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("app-data");
        let store = JsonChatPreferences::new(nested.join("chat.json"));
        store.save(&preferences(Some("opencode"))).unwrap();

        let file_mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
        let directory_mode = fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(directory_mode, 0o700);
    }
}
