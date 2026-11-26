//! The private cloud session file.
//!
//! Spec: `runtime/docs/design/relay-and-packaging.md` §6.4. An optional account
//! session is stored in the app-private data directory as one JSON file at mode
//! `0600`; there is no Keychain prompt and no second credential store. The file
//! holds the access and refresh tokens, the API base they belong to and safe
//! account metadata. Nothing here prints a token: [`CloudSession`] has a
//! redacting [`std::fmt::Debug`] implementation, and every read or write error
//! names the path, never the file contents.

#[cfg(unix)]
use std::fs::DirBuilder;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use zeroize::Zeroizing;

use super::error::{CloudError, CloudErrorCode};
use crate::time::Timestamp;

/// The schema version written into the session file.
pub const CLOUD_SESSION_SCHEMA_VERSION: u32 = 1;

/// The file name inside the app's private data directory.
pub const CLOUD_SESSION_FILE: &str = "cloud-session.json";

/// One optional signed-in cloud session.
///
/// `access_token` and `refresh_token` are wrapped in [`Zeroizing`], so they are
/// cleared when the session is dropped. The fields are public so a host may
/// implement [`CloudSessionStore`] on its own storage; they must never be
/// displayed, logged or serialized into a view model.
pub struct CloudSession {
    /// The API base URL this session belongs to, without a trailing slash.
    pub api_url: String,
    /// Short-lived account access token.
    pub access_token: Zeroizing<String>,
    /// Refresh token, when the API issued one.
    pub refresh_token: Option<Zeroizing<String>>,
    /// Access-token expiry, when the API reported one.
    pub expires_at: Option<Timestamp>,
    /// Account id (`acct_...`), when known.
    pub account_id: Option<String>,
    /// Account e-mail, when known.
    pub email: Option<String>,
}

impl CloudSession {
    /// Builds a session from a base URL and access token.
    #[must_use]
    pub fn new(api_url: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            access_token: Zeroizing::new(access_token.into()),
            refresh_token: None,
            expires_at: None,
            account_id: None,
            email: None,
        }
    }

    /// Adds or replaces the refresh token.
    #[must_use]
    pub fn with_refresh_token(mut self, token: impl Into<String>) -> Self {
        self.refresh_token = Some(Zeroizing::new(token.into()));
        self
    }

    /// Records the access-token expiry.
    #[must_use]
    pub fn with_expires_at(mut self, expires_at: Timestamp) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Adds safe account metadata.
    #[must_use]
    pub fn with_account(mut self, id: impl Into<String>, email: impl Into<String>) -> Self {
        self.account_id = Some(id.into());
        self.email = Some(email.into());
        self
    }

    /// Returns true when `now` is at or past the recorded access-token expiry.
    #[must_use]
    pub fn is_expired_at(&self, now: Timestamp) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

impl std::fmt::Debug for CloudSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudSession")
            .field("api_url", &self.api_url)
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .finish()
    }
}

impl Serialize for CloudSession {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CloudSession", 7)?;
        state.serialize_field("schema_version", &CLOUD_SESSION_SCHEMA_VERSION)?;
        state.serialize_field("api_url", &self.api_url)?;
        state.serialize_field("access_token", self.access_token.as_str())?;
        state.serialize_field(
            "refresh_token",
            &self.refresh_token.as_ref().map(|token| token.as_str()),
        )?;
        state.serialize_field("expires_at", &self.expires_at)?;
        state.serialize_field("account_id", &self.account_id)?;
        state.serialize_field("email", &self.email)?;
        state.end()
    }
}

#[derive(serde::Deserialize)]
struct RawCloudSession {
    schema_version: u32,
    api_url: String,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<Timestamp>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

impl<'de> Deserialize<'de> for CloudSession {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawCloudSession::deserialize(deserializer)?;
        if raw.schema_version != CLOUD_SESSION_SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "cloud session schema version {} is not supported",
                raw.schema_version
            )));
        }
        Ok(Self {
            api_url: raw.api_url,
            access_token: Zeroizing::new(raw.access_token),
            refresh_token: raw.refresh_token.map(Zeroizing::new),
            expires_at: raw.expires_at,
            account_id: raw.account_id,
            email: raw.email,
        })
    }
}

/// The host-supplied private session store boundary.
///
/// Production uses [`JsonCloudSessionStore`]. Tests may supply an in-memory
/// store; a host with a different private directory may provide its own. No
/// implementation may put a token in an error message or a log.
pub trait CloudSessionStore: Send + Sync {
    /// Loads the stored session, or `None` when the owner has never signed in.
    fn load(&self) -> Result<Option<CloudSession>, CloudError>;
    /// Writes the session atomically with owner-only permissions.
    fn save(&self, session: &CloudSession) -> Result<(), CloudError>;
    /// Removes the stored session. Returns whether a file was removed.
    fn remove(&self) -> Result<bool, CloudError>;
}

/// A JSON session store backed by one `0600` file.
#[derive(Debug, Clone)]
pub struct JsonCloudSessionStore {
    path: PathBuf,
}

impl JsonCloudSessionStore {
    /// Builds a store at `path`; the file is created on the first save.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Builds a store at `<directory>/cloud-session.json`.
    #[must_use]
    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self::new(directory.as_ref().join(CLOUD_SESSION_FILE))
    }

    /// Returns the backing path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl CloudSessionStore for JsonCloudSessionStore {
    fn load(&self) -> Result<Option<CloudSession>, CloudError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CloudError::io("reading the cloud session", &error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(CloudError::new(
                CloudErrorCode::Auth,
                "The cloud session file is a symbolic link; StackSpirit refuses to read it.",
            )
            .with_hint("Remove it, then sign in again."));
        }
        if !metadata.is_file() {
            return Err(CloudError::new(
                CloudErrorCode::Io,
                "The cloud session path is not a regular file.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(CloudError::new(
                    CloudErrorCode::Auth,
                    format!("The cloud session file is mode {mode:03o}; it must be 0600."),
                )
                .with_hint("Sign in again to rewrite it with owner-only permissions."));
            }
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| CloudError::io("reading the cloud session", &error))?;
        if bytes.is_empty() {
            return Err(CloudError::parse("cloud session file", "the file is empty"));
        }
        let session: CloudSession = serde_json::from_slice(&bytes)
            .map_err(|error| CloudError::parse("cloud session file", error))?;
        if session.api_url.trim().is_empty() {
            return Err(CloudError::parse(
                "cloud session file",
                "the API base is empty",
            ));
        }
        if session.access_token.trim().is_empty() {
            return Err(CloudError::parse(
                "cloud session file",
                "the access token is empty",
            ));
        }
        Ok(Some(session))
    }

    fn save(&self, session: &CloudSession) -> Result<(), CloudError> {
        if let Some(parent) = self.path.parent() {
            create_private_directory(parent)?;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let temporary = parent.join(format!(".cloud-session.{}.{nanos}.tmp", std::process::id()));
        let write = || -> Result<(), CloudError> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| CloudError::io("creating the cloud session", &error))?;
            let mut bytes = serde_json::to_vec_pretty(session)
                .map_err(|error| CloudError::parse("cloud session file", error))?;
            bytes.push(b'\n');
            let bytes = Zeroizing::new(bytes);
            file.write_all(&bytes)
                .map_err(|error| CloudError::io("writing the cloud session", &error))?;
            file.sync_all()
                .map_err(|error| CloudError::io("syncing the cloud session", &error))?;
            drop(file);
            fs::rename(&temporary, &self.path)
                .map_err(|error| CloudError::io("replacing the cloud session", &error))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
                    .map_err(|error| CloudError::io("securing the cloud session", &error))?;
            }
            Ok(())
        };
        match write() {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        }
    }

    fn remove(&self) -> Result<bool, CloudError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(CloudError::io("removing the cloud session", &error)),
        }
    }
}

fn create_private_directory(path: &Path) -> Result<(), CloudError> {
    if path.as_os_str().is_empty() || path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(path)
            .map_err(|error| CloudError::io("creating the app data directory", &error))?;
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
            .map_err(|error| CloudError::io("creating the app data directory", &error))?;
    }
    Ok(())
}
