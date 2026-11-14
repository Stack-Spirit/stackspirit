//! Provider-key storage. Keys live only in the local OS keychain.
//!
//! [`SecretString`] deliberately has no `Debug`, `Display` or `Serialize` implementation,
//! so a key cannot be logged or sent to the VPS through a derived wire type.

use std::collections::BTreeMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

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

/// The real OS keychain, enabled by the `keychain` feature.
#[cfg(feature = "keychain")]
#[derive(Debug, Clone)]
pub struct OsKeychain {
    service: String,
}

#[cfg(feature = "keychain")]
impl OsKeychain {
    /// Builds a keychain namespace. Use a stable service name such as `dev.stackspirit`.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
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
            .map_err(|_| KeychainError::new(provider, KeychainErrorClass::Platform))
    }

    fn get(&self, provider: &str) -> Result<Option<SecretString>, KeychainError> {
        match self.entry(provider)?.get_password() {
            Ok(value) => Ok(Some(SecretString::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(KeychainError::new(provider, KeychainErrorClass::Platform)),
        }
    }

    fn delete(&self, provider: &str) -> Result<(), KeychainError> {
        self.entry(provider)?
            .delete_credential()
            .map_err(|_| KeychainError::new(provider, KeychainErrorClass::Platform))
    }

    fn list(&self) -> Result<Vec<String>, KeychainError> {
        Err(KeychainError::new("all", KeychainErrorClass::Unavailable))
    }
}
