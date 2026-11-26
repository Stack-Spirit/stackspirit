//! The relay identity and the machine files: `relay.key`, `relay.pub`, `relay.json` and the
//! status file.
//!
//! The relay key is a **new** Ed25519 key, separate from `node.key`, so rotating it never
//! disturbs the v0.4 home-agent enrollment. Modes: `relay.key` 0600, `relay.pub` 0644,
//! `relay.json` 0600. Nothing here prints a private key, a credential or a token.

use std::fmt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use super::RelayError;

/// Default private key file name.
pub const RELAY_KEY_FILE: &str = "relay.key";
/// Public key file name.
pub const RELAY_PUB_FILE: &str = "relay.pub";
/// Default credential file name.
pub const RELAY_JSON_FILE: &str = "relay.json";
/// Connector status file name (no secrets).
pub const RELAY_STATUS_FILE: &str = "relay-status.json";
/// Cached API JWKS file name (public, 24 hours at most).
pub const RELAY_JWKS_FILE: &str = "relay-jwks.json";
/// The `key_id` display prefix and domain separator.
pub const KEY_ID_PREFIX: &str = "ed25519:";

/// The machine's long-lived Ed25519 relay identity.
#[derive(Clone)]
pub struct RelayKey {
    signing: SigningKey,
}

impl fmt::Debug for RelayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayKey")
            .field("key_id", &self.key_id())
            .finish_non_exhaustive()
    }
}

impl RelayKey {
    /// Generates a key in memory; nothing is written.
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    /// Builds a key from a fixed secret. Used by the committed test vectors only.
    pub fn from_secret(secret: [u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(&secret),
        }
    }

    /// Writes `relay.key` (0600) and `relay.pub` (0644) atomically into `home`.
    ///
    /// `key_file` is the configured private file name; `relay.pub` always sits beside it.
    pub fn write(&self, home: &Path, key_file: &str) -> Result<(), RelayError> {
        write_atomic(
            &home.join(key_file),
            format!("{}\n", hex::encode(self.signing.to_bytes())).as_bytes(),
            0o600,
        )?;
        write_atomic(
            &home.join(RELAY_PUB_FILE),
            format!("{}\n", self.public_key_hex()).as_bytes(),
            0o644,
        )
    }

    /// Loads the private key; refuses a file that is group or world accessible.
    pub fn load(home: &Path, key_file: &str) -> Result<Self, RelayError> {
        let path = home.join(key_file);
        let mode = fs::metadata(&path)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => RelayError::NotEnrolled(format!("{key_file} is missing")),
                _ => RelayError::Io(error),
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(RelayError::Key(format!(
                "{key_file} is mode {mode:03o}; the relay key must be 0600"
            )));
        }
        let text = fs::read_to_string(&path)?;
        let bytes = hex::decode(text.trim()).map_err(|_| RelayError::Key(format!("{key_file} is not hex")))?;
        let secret: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| RelayError::Key(format!("{key_file} must hold 32 bytes")))?;
        Ok(Self::from_secret(secret))
    }

    /// The public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// The 32-byte public key as lowercase hex (`mkey` in tickets).
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key().as_bytes())
    }

    /// `ed25519:<64 lowercase hex>`.
    pub fn key_id(&self) -> String {
        format!("{KEY_ID_PREFIX}{}", self.public_key_hex())
    }

    /// Signs `message` and returns the lowercase hex signature.
    pub fn sign_hex(&self, message: &[u8]) -> String {
        hex::encode(self.signing.sign(message).to_bytes())
    }
}

/// Verifies a lowercase-hex Ed25519 `signature` over `message` with a lowercase-hex public key.
pub fn verify_hex(public_key_hex: &str, message: &[u8], signature_hex: &str) -> Result<(), RelayError> {
    let public: [u8; 32] = hex::decode(public_key_hex)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| RelayError::Auth("the public key is not 32 hex bytes".to_owned()))?;
    let signature: [u8; 64] = hex::decode(signature_hex)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| RelayError::Auth("the signature is not 64 hex bytes".to_owned()))?;
    let verifying =
        VerifyingKey::from_bytes(&public).map_err(|_| RelayError::Auth("the public key is invalid".to_owned()))?;
    verifying
        .verify(message, &Signature::from_bytes(&signature))
        .map_err(|_| RelayError::Auth("bad signature".to_owned()))
}

/// Writes `bytes` to `path` through a same-directory temporary file and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), RelayError> {
    let parent = path
        .parent()
        .ok_or_else(|| RelayError::Config(format!("{} has no parent directory", path.display())))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(RelayError::Io)
}

/// `relay.json`: the machine's enrollment record. Mode 0600; it holds the credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayJson {
    /// File version, 1.
    pub v: u8,
    /// API base URL.
    pub api_url: String,
    /// Machine WebSocket URL.
    pub relay_url: String,
    /// `mch_` machine id.
    pub machine_id: String,
    /// `acct_` account id.
    pub account_id: String,
    /// `ed25519:<hex>` key id.
    pub key_id: String,
    /// The ES256 machine credential (a bearer authorisation to attempt the challenge).
    pub credential: String,
    /// Credential expiry, RFC 3339.
    pub credential_expires_at: String,
    /// Machine display name.
    #[serde(default)]
    pub name: String,
    /// Served project.
    #[serde(default)]
    pub project: String,
    /// Account used by `stackspirit ssh`.
    #[serde(default)]
    pub ssh_user: String,
    /// Registration time, RFC 3339.
    #[serde(default)]
    pub registered_at: String,
}

impl fmt::Debug for RelayJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayJson")
            .field("machine_id", &self.machine_id)
            .field("account_id", &self.account_id)
            .field("key_id", &self.key_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl RelayJson {
    /// Reads `relay.json`; refuses a file that is group or world accessible.
    pub fn load(path: &Path) -> Result<Self, RelayError> {
        let mode = match fs::metadata(path) {
            Ok(meta) => meta.permissions().mode() & 0o777,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RelayError::NotEnrolled(format!(
                    "{} is missing; run `stackspirit relay enroll`",
                    file_label(path)
                )));
            }
            Err(error) => return Err(RelayError::Io(error)),
        };
        if mode & 0o077 != 0 {
            return Err(RelayError::Key(format!(
                "{} is mode {mode:03o}; it holds the credential and must be 0600",
                file_label(path)
            )));
        }
        let text = fs::read_to_string(path)?;
        let record: Self = serde_json::from_str(&text)
            .map_err(|_| RelayError::Config(format!("{} is not valid", file_label(path))))?;
        if record.v != 1 {
            return Err(RelayError::Config(format!(
                "{} has an unsupported version",
                file_label(path)
            )));
        }
        Ok(record)
    }

    /// Writes the record atomically at mode 0600.
    pub fn save(&self, path: &Path) -> Result<(), RelayError> {
        let text = serde_json::to_string_pretty(self).map_err(|_| RelayError::Config("relay.json".to_owned()))?;
        write_atomic(path, format!("{text}\n").as_bytes(), 0o600)
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "relay.json".to_owned())
}

/// The connector's coarse state, as written to `relay-status.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorState {
    /// `[relay].enabled` is false.
    Disabled,
    /// Opening a connection or authenticating.
    Connecting,
    /// Authenticated and serving.
    Online,
    /// Waiting to reconnect.
    Backoff,
    /// The owner revoked the machine; the connector stopped.
    Revoked,
    /// Another connection superseded this one; the connector stopped.
    Superseded,
    /// Stopped on request.
    Stopped,
}

/// `relay-status.json`: connector progress, with no credential, token or key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayStatusFile {
    /// Coarse state.
    pub state: ConnectorState,
    /// Process id of the connector.
    pub pid: u32,
    /// Machine id.
    pub machine_id: String,
    /// Relay URL.
    pub relay_url: String,
    /// Open channels.
    pub channels: u32,
    /// Connections made since start.
    pub connects: u64,
    /// Last successful authentication, RFC 3339.
    #[serde(default)]
    pub last_connected_at: Option<String>,
    /// Last status write, RFC 3339.
    pub updated_at: String,
    /// A short stable code for the last failure (never a payload, token or credential).
    #[serde(default)]
    pub last_error: Option<String>,
}

impl RelayStatusFile {
    /// Writes the file atomically at mode 0600.
    pub fn save(&self, path: &Path) -> Result<(), RelayError> {
        let text = serde_json::to_string_pretty(self).map_err(|_| RelayError::Config("relay status".to_owned()))?;
        write_atomic(path, format!("{text}\n").as_bytes(), 0o600)
    }

    /// Reads the file, or `None` when it does not exist or is unreadable.
    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
    }
}

/// Resolved file locations for one home and one `[relay]` table.
#[derive(Debug, Clone)]
pub struct RelayPaths {
    /// The node home.
    pub home: PathBuf,
    /// Private key file name.
    pub key_file: String,
    /// Credential file name.
    pub credential_file: String,
}

impl RelayPaths {
    /// Builds the paths from a `[relay]` table.
    pub fn new(home: &Path, config: &vowp_protocol::RelayConfig) -> Self {
        Self {
            home: home.to_owned(),
            key_file: config.key_file.clone(),
            credential_file: config.credential_file.clone(),
        }
    }

    /// `relay.json`.
    pub fn credential(&self) -> PathBuf {
        self.home.join(&self.credential_file)
    }

    /// The status file.
    pub fn status(&self) -> PathBuf {
        self.home.join(RELAY_STATUS_FILE)
    }

    /// The JWKS cache file.
    pub fn jwks(&self) -> PathBuf {
        self.home.join(RELAY_JWKS_FILE)
    }
}
