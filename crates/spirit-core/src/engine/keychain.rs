//! Provider-key storage. Keys live only in the local OS keychain.
//!
//! [`SecretString`] deliberately has no `Debug`, `Display` or `Serialize` implementation,
//! so a key cannot be logged or sent to the VPS through a derived wire type.

use std::collections::BTreeMap;
use std::sync::Mutex;

#[cfg(feature = "keychain")]
use std::collections::BTreeSet;
#[cfg(feature = "keychain")]
use std::path::{Path, PathBuf};
#[cfg(feature = "keychain")]
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::engine::runtime::{EngineRuntime, KeyVerification};
use crate::time::Timestamp;

/// A zeroizing secret with an explicit accessor. No formatting traits are implemented.
#[derive(Clone, Default)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Wraps a secret value; the input is moved into zeroizing storage.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Borrows the secret for the one call that needs it (a keychain write or a provider
    /// request). Callers must not log the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    /// Returns true for an empty value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Why a keychain operation failed. Never carries a secret value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeychainErrorClass {
    /// No platform keychain is available.
    Unavailable,
    /// The user denied access.
    Denied,
    /// The provider entry does not exist.
    NotFound,
    /// A platform error occurred.
    Platform,
    /// The input was invalid.
    Invalid,
}

/// A keychain error naming the provider and the error class only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainError {
    /// Provider name.
    pub provider: String,
    /// Error class.
    pub class: KeychainErrorClass,
}

impl KeychainError {
    /// Builds an error.
    #[must_use]
    pub fn new(provider: impl Into<String>, class: KeychainErrorClass) -> Self {
        Self {
            provider: provider.into(),
            class,
        }
    }
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "keychain {} failed ({:?}); no key value is included",
            self.provider, self.class
        )
    }
}

impl std::error::Error for KeychainError {}

/// Alias used by the D4 brief for the secret-storage boundary.
pub trait SecretStore: Keychain {}
impl<T: Keychain + ?Sized> SecretStore for T {}

/// A provider-key state safe to show in the UI and the IPC event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKeyState {
    /// No key is stored for the provider.
    Missing,
    /// A key is stored and has not been checked.
    Saved,
    /// The engine rejected the stored key.
    Invalid,
    /// The engine could not prove or disprove the stored key.
    Unverified,
}

/// The result of saving or checking one provider key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderKeyStatus {
    /// Provider name. Never a value.
    pub provider: String,
    /// Key state.
    pub state: ProviderKeyState,
    /// When the state was checked.
    pub last_checked_at: Option<Timestamp>,
}

impl ProviderKeyStatus {
    /// Builds a status without a key value.
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        state: ProviderKeyState,
        last_checked_at: Option<Timestamp>,
    ) -> Self {
        Self {
            provider: provider.into(),
            state,
            last_checked_at,
        }
    }
}

/// Reads a stored key and asks the local engine to check it.
///
/// The key never leaves the keychain except for the one engine call. A missing engine
/// leaves the key `Unverified`; a missing key is `Missing`. The key is never deleted on
/// failure, and no error carries its value.
pub async fn verify_provider_key(
    keychain: &dyn Keychain,
    runtime: &dyn EngineRuntime,
    port: Option<u16>,
    provider: &str,
    checked_at: Timestamp,
) -> Result<ProviderKeyStatus, KeychainError> {
    let Some(secret) = keychain.get(provider)? else {
        return Ok(ProviderKeyStatus::new(
            provider,
            ProviderKeyState::Missing,
            Some(checked_at),
        ));
    };
    let state = match port {
        Some(port) => match runtime.verify_key(port, provider, &secret).await {
            KeyVerification::Verified => ProviderKeyState::Saved,
            KeyVerification::Invalid => ProviderKeyState::Invalid,
            KeyVerification::Unverified => ProviderKeyState::Unverified,
        },
        None => ProviderKeyState::Unverified,
    };
    Ok(ProviderKeyStatus::new(provider, state, Some(checked_at)))
}

/// The local keychain boundary.
pub trait Keychain: Send + Sync {
    /// Stores a provider key.
    fn set(&self, provider: &str, secret: &SecretString) -> Result<(), KeychainError>;
    /// Reads a provider key.
    fn get(&self, provider: &str) -> Result<Option<SecretString>, KeychainError>;
    /// Deletes a provider key.
    fn delete(&self, provider: &str) -> Result<(), KeychainError>;
    /// Lists provider names only.
    fn list(&self) -> Result<Vec<String>, KeychainError>;
}

/// An in-memory keychain for tests and preview. It never touches the filesystem.
#[derive(Default)]
pub struct FakeKeychain {
    entries: Mutex<BTreeMap<String, Zeroizing<String>>>,
}

impl std::fmt::Debug for FakeKeychain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeKeychain")
            .field("providers", &self.len())
            .finish_non_exhaustive()
    }
}

impl FakeKeychain {
    /// Builds an empty keychain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fake storage is memory-only; no file is ever created.
    #[must_use]
    pub const fn writes_files(&self) -> bool {
        false
    }

    /// Returns the number of stored providers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Returns true when no provider key is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Keychain for FakeKeychain {
    fn set(&self, provider: &str, secret: &SecretString) -> Result<(), KeychainError> {
        if provider.trim().is_empty() || secret.is_empty() {
            return Err(KeychainError::new(provider, KeychainErrorClass::Invalid));
        }
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                provider.to_owned(),
                Zeroizing::new(secret.expose().to_owned()),
            );
        Ok(())
    }

    fn get(&self, provider: &str) -> Result<Option<SecretString>, KeychainError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(provider)
            .map(|value| SecretString::new(value.as_str())))
    }

    fn delete(&self, provider: &str) -> Result<(), KeychainError> {
        let removed = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(provider);
        if removed.is_some() {
            Ok(())
        } else {
            Err(KeychainError::new(provider, KeychainErrorClass::NotFound))
        }
    }

    fn list(&self) -> Result<Vec<String>, KeychainError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect())
    }
}

#[cfg(feature = "keychain")]
fn read_index(path: &Path) -> Result<BTreeSet<String>, KeychainError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(_) => return Err(KeychainError::new("all", KeychainErrorClass::Platform)),
    };
    let providers: Vec<String> = serde_json::from_slice(&bytes)
        .map_err(|_| KeychainError::new("all", KeychainErrorClass::Platform))?;
    Ok(providers.into_iter().collect())
}

#[cfg(feature = "keychain")]
fn write_index(path: &Path, providers: &BTreeSet<String>) -> Result<(), KeychainError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| KeychainError::new("all", KeychainErrorClass::Platform))?;
    }
    let bytes = serde_json::to_vec_pretty(&providers.iter().collect::<Vec<_>>())
        .map_err(|_| KeychainError::new("all", KeychainErrorClass::Platform))?;
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, bytes)
        .map_err(|_| KeychainError::new("all", KeychainErrorClass::Platform))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&staged, path).map_err(|_| {
        let _ = std::fs::remove_file(&staged);
        KeychainError::new("all", KeychainErrorClass::Platform)
    })
}

#[cfg(feature = "keychain")]
fn add_to_index(path: &Path, provider: &str) -> Result<(), KeychainError> {
    let mut providers = read_index(path)?;
    providers.insert(provider.to_owned());
    write_index(path, &providers)
}

#[cfg(feature = "keychain")]
fn remove_from_index(path: &Path, provider: &str) -> Result<(), KeychainError> {
    let mut providers = read_index(path)?;
    if providers.remove(provider) {
        write_index(path, &providers)?;
    }
    Ok(())
}

/// The real OS keychain, enabled by the `keychain` feature.
#[cfg(feature = "keychain")]
#[derive(Debug, Clone)]
pub struct OsKeychain {
    service: String,
    index_path: Arc<Mutex<Option<PathBuf>>>,
}

#[cfg(feature = "keychain")]
impl OsKeychain {
    /// Builds a keychain namespace. Use a stable service name such as `dev.stackspirit`.
    ///
    /// `list` is unavailable until an index path is supplied with
    /// [`with_index`](Self::with_index): the OS APIs do not enumerate credentials, and
    /// StackSpirit never writes a key outside the keychain.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            index_path: Arc::new(Mutex::new(None)),
        }
    }

    /// Enables provider listing through a local, non-secret provider-name index.
    ///
    /// The index file contains provider ids only. Key values remain in the OS keychain.
    #[must_use]
    pub fn with_index(self, path: impl Into<PathBuf>) -> Self {
        *self
            .index_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.into());
        self
    }

    fn index(&self) -> Option<PathBuf> {
        self.index_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn entry(&self, provider: &str) -> Result<keyring::Entry, KeychainError> {
        keyring::Entry::new(&self.service, provider)
            .map_err(|_| KeychainError::new(provider, KeychainErrorClass::Unavailable))
    }
}

#[cfg(feature = "keychain")]
impl Keychain for OsKeychain {
    fn set(&self, provider: &str, secret: &SecretString) -> Result<(), KeychainError> {
        if provider.trim().is_empty() || secret.is_empty() {
            return Err(KeychainError::new(provider, KeychainErrorClass::Invalid));
        }
        self.entry(provider)?
            .set_password(secret.expose())
            .map_err(|_| KeychainError::new(provider, KeychainErrorClass::Platform))?;
        if let Some(index) = self.index()
            && let Err(error) = add_to_index(&index, provider)
        {
            let _ = self.entry(provider)?.delete_credential();
            return Err(error);
        }
        Ok(())
    }

    fn get(&self, provider: &str) -> Result<Option<SecretString>, KeychainError> {
        match self.entry(provider)?.get_password() {
            Ok(value) => Ok(Some(SecretString::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(KeychainError::new(provider, KeychainErrorClass::Platform)),
        }
    }

    fn delete(&self, provider: &str) -> Result<(), KeychainError> {
        let result = self.entry(provider)?.delete_credential();
        let index = self.index();
        match result {
            Ok(()) => {
                if let Some(index) = index {
                    remove_from_index(&index, provider)?;
                }
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                if let Some(index) = index {
                    remove_from_index(&index, provider)?;
                }
                Err(KeychainError::new(provider, KeychainErrorClass::NotFound))
            }
            Err(_) => Err(KeychainError::new(provider, KeychainErrorClass::Platform)),
        }
    }

    fn list(&self) -> Result<Vec<String>, KeychainError> {
        let Some(index) = self.index() else {
            return Err(KeychainError::new("all", KeychainErrorClass::Unavailable));
        };
        Ok(read_index(&index)?.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::FakeEngineRuntime;

    #[tokio::test]
    async fn engine_byok_verify_failure_keeps_the_key_with_invalid_status() {
        let keychain = FakeKeychain::new();
        let secret = SecretString::new("not-a-real-provider-key");
        keychain.set("deepseek", &secret).unwrap();
        let runtime = FakeEngineRuntime::new().with_key_verification(KeyVerification::Invalid);
        let status = verify_provider_key(
            &keychain,
            &runtime,
            Some(43117),
            "deepseek",
            Timestamp::parse("2026-11-14T12:00:00.000Z").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(status.state, ProviderKeyState::Invalid);
        assert_eq!(
            keychain.get("deepseek").unwrap().unwrap().expose(),
            "not-a-real-provider-key",
            "a failed verification must not delete the key"
        );
        let serialized = serde_json::to_string(&status).unwrap();
        assert!(!serialized.contains("not-a-real-provider-key"));
        assert!(serialized.contains("deepseek"));
    }

    #[tokio::test]
    async fn engine_byok_missing_key_reports_missing_without_reading_a_secret() {
        let keychain = FakeKeychain::new();
        let runtime = FakeEngineRuntime::new();
        let status = verify_provider_key(&keychain, &runtime, None, "openai", Timestamp::now())
            .await
            .unwrap();
        assert_eq!(status.state, ProviderKeyState::Missing);
        assert!(!format!("{status:?}").contains("key"));
    }

    #[cfg(feature = "keychain")]
    #[test]
    fn engine_keychain_index_lists_provider_names_only() {
        let directory = tempfile::tempdir().unwrap();
        let index = directory.path().join("keychain-index.json");
        add_to_index(&index, "deepseek").unwrap();
        add_to_index(&index, "openai").unwrap();
        assert_eq!(
            read_index(&index).unwrap().into_iter().collect::<Vec<_>>(),
            vec!["deepseek".to_owned(), "openai".to_owned()]
        );
        remove_from_index(&index, "deepseek").unwrap();
        assert_eq!(
            read_index(&index).unwrap().into_iter().collect::<Vec<_>>(),
            vec!["openai".to_owned()]
        );
        let raw = std::fs::read_to_string(&index).unwrap();
        assert!(!raw.contains(concat!("s", "k-")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&index).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn engine_byok_unverified_when_the_engine_is_not_running() {
        let keychain = FakeKeychain::new();
        keychain
            .set("openai", &SecretString::new("not-a-real-key"))
            .unwrap();
        let runtime = FakeEngineRuntime::new();
        let status = verify_provider_key(&keychain, &runtime, None, "openai", Timestamp::now())
            .await
            .unwrap();
        assert_eq!(status.state, ProviderKeyState::Unverified);
    }
}
