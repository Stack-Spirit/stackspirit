//! Host profiles: the local record of one VPS. Profiles never contain key material.

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::SpiritError;
use crate::ssh::SshTarget;
use crate::ssh::fingerprint::ScannedHostKey;
use crate::time::Timestamp;

/// Local host profile id. Created as a ULID; stored as an opaque string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostId(pub String);

impl HostId {
    /// Creates a fresh ULID-backed id.
    #[must_use]
    pub fn new() -> Self {
        Self(ulid::Ulid::generate().to_string())
    }

    /// Borrows the id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for HostId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for HostId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The confirmed host-key pin. It stores fingerprints, never a private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyPin {
    /// SSH algorithm, for example `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:...` exactly as `ssh-keygen -lf` prints it.
    pub sha256: String,
    /// When the user confirmed the fingerprint.
    pub pinned_at: Timestamp,
}

impl HostKeyPin {
    /// Validates the pin shape without touching the network.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if !self.algorithm.starts_with("ssh-") && !self.algorithm.starts_with("ecdsa-") {
            return Err(SpiritError::invalid_input(
                "host_key.algorithm",
                "must be an SSH public-key algorithm",
            ));
        }
        if !self.sha256.starts_with("SHA256:") || self.sha256.len() < 20 {
            return Err(SpiritError::invalid_input(
                "host_key.sha256",
                "must be a SHA256 fingerprint from ssh-keygen",
            ));
        }
        Ok(())
    }

    /// Builds a pin from a scanned key that the user confirmed.
    #[must_use]
    pub fn from_scanned(key: &ScannedHostKey, pinned_at: Timestamp) -> Self {
        Self {
            algorithm: key.algorithm.clone(),
            sha256: key.sha256.clone(),
            pinned_at,
        }
    }
}

/// The pre-save form before a host key is confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDraft {
    /// Display label, 1-64 characters.
    pub label: String,
    /// SSH destination.
    pub ssh: SshTarget,
    /// Identity file path. Path only; StackSpirit never reads the key.
    pub identity_file: Option<PathBuf>,
    /// Remote `--home` path, when the VOWP home is not the default.
    pub vowp_home: Option<String>,
    /// Default project for project-scoped actions.
    pub default_project: Option<String>,
}

impl HostDraft {
    /// Validates every user-editable field.
    pub fn validate(&self) -> Result<(), SpiritError> {
        let label = self.label.trim();
        if label.is_empty() || label.chars().count() > 64 {
            return Err(SpiritError::invalid_input(
                "label",
                "must be 1-64 characters",
            ));
        }
        self.ssh.validate()?;
        if let Some(home) = &self.vowp_home {
            if !home.is_empty() && !home.starts_with('/') {
                return Err(SpiritError::invalid_input(
                    "vowp_home",
                    "must be an absolute path or empty",
                ));
            }
            if home.contains('\u{0}') || home.contains('\n') {
                return Err(SpiritError::invalid_input(
                    "vowp_home",
                    "must not contain control characters",
                ));
            }
        }
        if let Some(project) = &self.default_project {
            if !project.is_empty() && crate::wire::ids::ProjectName::parse(project).is_err() {
                return Err(SpiritError::invalid_input(
                    "default_project",
                    "must be a project slug or empty",
                ));
            }
        }
        Ok(())
    }

    /// Returns the trimmed label.
    #[must_use]
    pub fn trimmed_label(&self) -> String {
        self.label.trim().to_owned()
    }
}

/// The first-save form: a draft plus the fingerprint the user confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewHostProfile {
    /// User-editable fields.
    #[serde(flatten)]
    pub draft: HostDraft,
    /// Confirmed pin.
    pub host_key: HostKeyPin,
}

/// A partial profile update. `None` means "leave unchanged"; `Some(None)` clears a
/// nullable field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProfilePatch {
    /// New label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Replacement SSH target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTarget>,
    /// New identity file path, or `Some(None)` to clear it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_field"
    )]
    pub identity_file: Option<Option<PathBuf>>,
    /// New remote home, or `Some(None)` to clear it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_field"
    )]
    pub vowp_home: Option<Option<String>>,
    /// New default project, or `Some(None)` to clear it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_field"
    )]
    pub default_project: Option<Option<String>>,
}

fn deserialize_optional_field<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// One saved VPS profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProfile {
    /// Local id.
    pub id: HostId,
    /// Display label.
    pub label: String,
    /// SSH destination.
    pub ssh: SshTarget,
    /// Identity file path.
    pub identity_file: Option<PathBuf>,
    /// Remote `--home` path.
    pub vowp_home: Option<String>,
    /// Default project for project-scoped actions.
    pub default_project: Option<String>,
    /// Confirmed host-key pin.
    pub host_key: HostKeyPin,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last local update.
    pub updated_at: Timestamp,
    /// Last successful connect.
    pub last_connected_at: Option<Timestamp>,
}

impl HostProfile {
    /// Builds a profile from a validated draft and pin.
    pub fn create(new: NewHostProfile, now: Timestamp) -> Result<Self, SpiritError> {
        new.draft.validate()?;
        new.host_key.validate()?;
        Ok(Self {
            id: HostId::new(),
            label: new.draft.trimmed_label(),
            ssh: new.draft.ssh,
            identity_file: new.draft.identity_file,
            vowp_home: new.draft.vowp_home.filter(|value| !value.is_empty()),
            default_project: new.draft.default_project.filter(|value| !value.is_empty()),
            host_key: new.host_key,
            created_at: now,
            updated_at: now,
            last_connected_at: None,
        })
    }

    /// Validates the stored profile. A profile without a pin cannot connect.
    pub fn validate(&self) -> Result<(), SpiritError> {
        if self.id.0.trim().is_empty() {
            return Err(SpiritError::invalid_input("host.id", "must not be empty"));
        }
        HostDraft {
            label: self.label.clone(),
            ssh: self.ssh.clone(),
            identity_file: self.identity_file.clone(),
            vowp_home: self.vowp_home.clone(),
            default_project: self.default_project.clone(),
        }
        .validate()?;
        self.host_key.validate()
    }

    /// Applies a partial update and returns whether anything changed.
    pub fn apply_patch(
        &mut self,
        patch: HostProfilePatch,
        now: Timestamp,
    ) -> Result<bool, SpiritError> {
        let before = self.clone();
        if let Some(label) = patch.label {
            self.label = label;
        }
        if let Some(ssh) = patch.ssh {
            self.ssh = ssh;
        }
        if let Some(identity_file) = patch.identity_file {
            self.identity_file = identity_file;
        }
        if let Some(vowp_home) = patch.vowp_home {
            self.vowp_home = vowp_home.filter(|value| !value.is_empty());
        }
        if let Some(default_project) = patch.default_project {
            self.default_project = default_project.filter(|value| !value.is_empty());
        }
        self.label = self.label.trim().to_owned();
        self.updated_at = now;
        self.validate()?;
        Ok(*self != before)
    }

    /// Returns a display string for the SSH destination.
    #[must_use]
    pub fn destination(&self) -> String {
        format!("{}@{}:{}", self.ssh.user, self.ssh.host, self.ssh.port)
    }
}

/// Verifies a scanned key against the pinned fingerprint.
///
/// A missing pin or a changed key is a hard block; there is no auto-accept.
pub fn verify_pin_against_scan(
    pin: &HostKeyPin,
    scanned: &[ScannedHostKey],
) -> Result<(), SpiritError> {
    pin.validate()?;
    if scanned.iter().any(|key| key.sha256 == pin.sha256) {
        return Ok(());
    }
    let offered = scanned.first().map_or("no keys", |key| key.sha256.as_str());
    Err(SpiritError::host_key_mismatch(&pin.sha256, offered))
}
