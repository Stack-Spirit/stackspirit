//! The cloud session file: `<home>/cloud-session.json` at mode 0600.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.4 and §6.7. The file holds the access and
//! refresh tokens and the API base they belong to. Nothing here prints a token: the custom
//! [`std::fmt::Debug`] implementation redacts both values, and every read or write error names
//! the path, never the contents.

use std::fmt;
use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CliError;

/// File name inside the node home, beside `relay.json` and the databases.
pub const SESSION_FILE: &str = "cloud-session.json";

/// One signed-in cloud session.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    /// API base URL this session belongs to, without a trailing slash.
    pub api_url: String,
    /// Current access token. Never printed.
    pub access_token: String,
    /// Refresh token, when the API issued one. Never printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// RFC 3339 expiry of the access token, when the API reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Account id (`acct_...`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Account email, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("api_url", &self.api_url)
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .field("expires_at", &self.expires_at)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Returns the session file path inside `home`.
pub fn session_path(home: &Path) -> PathBuf {
    home.join(SESSION_FILE)
}

impl Session {
    /// Loads the stored session, or `None` when no login has happened in this home.
    pub fn load(home: &Path) -> Result<Option<Self>, CliError> {
        let path = session_path(home);
        let mode = match std::fs::metadata(&path) {
            Ok(metadata) => metadata.permissions().mode() & 0o777,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CliError::Io(error)),
        };
        if mode & 0o077 != 0 {
            return Err(CliError::Auth(format!(
                "the cloud session file {} is mode {mode:03o}; it must be 0600. \
                 Run `{} login` again to rewrite it",
                path.display(),
                crate::cli::binary_brand()
            )));
        }
        let text = std::fs::read_to_string(&path).map_err(CliError::Io)?;
        let session: Session = serde_json::from_str(&text).map_err(|_| {
            CliError::Invalid(format!(
                "the cloud session file {} is not valid JSON; run `{} login` again",
                path.display(),
                crate::cli::binary_brand()
            ))
        })?;
        if session.access_token.trim().is_empty() {
            return Err(CliError::Invalid(format!(
                "the cloud session file {} has no access token; run `{} login` again",
                path.display(),
                crate::cli::binary_brand()
            )));
        }
        Ok(Some(session))
    }

    /// Writes the session atomically with mode 0600.
    ///
    /// The temporary file is created with `create_new(true)` and mode 0600, so a pre-existing
    /// symlink or file at the temporary path is refused rather than followed.
    pub fn save(&self, home: &Path) -> Result<PathBuf, CliError> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(home)
            .map_err(CliError::Io)?;
        let path = session_path(home);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let temp = home.join(format!(".cloud-session.{}.{nanos}.tmp", std::process::id()));
        let write = || -> Result<(), CliError> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            let mut file = options.open(&temp).map_err(CliError::Io)?;
            let text = serde_json::to_string_pretty(self)
                .map_err(|error| CliError::Invalid(format!("cannot serialise the cloud session: {error}")))?;
            file.write_all(text.as_bytes()).map_err(CliError::Io)?;
            file.write_all(b"\n").map_err(CliError::Io)?;
            file.sync_all().map_err(CliError::Io)?;
            std::fs::rename(&temp, &path).map_err(CliError::Io)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(CliError::Io)?;
            Ok(())
        };
        match write() {
            Ok(()) => Ok(path),
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                Err(error)
            }
        }
    }

    /// Removes the stored session. Returns whether a file was removed.
    pub fn remove(home: &Path) -> Result<bool, CliError> {
        match std::fs::remove_file(session_path(home)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(CliError::Io(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            api_url: "https://api.example".to_owned(),
            access_token: "access-secret".to_owned(),
            refresh_token: Some("refresh-secret".to_owned()),
            expires_at: None,
            account_id: Some("acct_1".to_owned()),
            email: None,
        }
    }

    #[test]
    fn save_is_0600_and_round_trips() {
        let home = tempfile::tempdir().expect("home");
        let path = session().save(home.path()).expect("save");
        assert_eq!(path, session_path(home.path()));
        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded = Session::load(home.path()).expect("load").expect("some");
        assert_eq!(loaded.access_token, "access-secret");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-secret"));
    }

    #[test]
    fn debug_redacts_tokens() {
        let text = format!("{:?}", session());
        assert!(!text.contains("access-secret"), "{text}");
        assert!(!text.contains("refresh-secret"), "{text}");
        assert!(text.contains("<redacted>"), "{text}");
    }

    #[test]
    fn load_refuses_a_group_or_world_readable_session() {
        let home = tempfile::tempdir().expect("home");
        let path = session().save(home.path()).expect("save");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let error = Session::load(home.path()).expect_err("loose mode must be refused");
        assert!(matches!(error, CliError::Auth(_)), "{error}");
        assert!(!error.to_string().contains("access-secret"));
    }

    #[test]
    fn remove_is_idempotent() {
        let home = tempfile::tempdir().expect("home");
        assert!(!Session::remove(home.path()).expect("remove"));
        session().save(home.path()).expect("save");
        assert!(Session::remove(home.path()).expect("remove"));
        assert!(Session::load(home.path()).expect("load").is_none());
    }
}
