//! Host profile persistence. One versioned JSON file, written atomically with mode 0600.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::host::profile::{HostId, HostKeyPin, HostProfile, HostProfilePatch, NewHostProfile};
use crate::time::Timestamp;

/// The schema version of the host store file.
pub const HOST_STORE_SCHEMA_VERSION: u32 = 1;

/// The host profile store boundary.
pub trait HostStore: Send + Sync {
    /// Returns every saved profile, in file order.
    fn list(&self) -> Result<Vec<HostProfile>, SpiritError>;
    /// Returns one profile.
    fn get(&self, id: &HostId) -> Result<Option<HostProfile>, SpiritError>;
    /// Adds a profile. The caller must have supplied a confirmed pin.
    fn add(&self, new: NewHostProfile) -> Result<HostProfile, SpiritError>;
    /// Updates user-editable fields.
    fn update(&self, id: &HostId, patch: HostProfilePatch) -> Result<HostProfile, SpiritError>;
    /// Removes a profile and its pin.
    fn remove(&self, id: &HostId) -> Result<(), SpiritError>;
    /// Replaces the pin after an explicit out-of-band verification.
    fn repin(&self, id: &HostId, pin: HostKeyPin) -> Result<HostProfile, SpiritError>;
    /// Records a successful connection time.
    fn mark_connected(&self, id: &HostId, at: Timestamp) -> Result<HostProfile, SpiritError>;
}

#[derive(Debug, Serialize, Deserialize)]
struct HostFile {
    schema_version: u32,
    hosts: Vec<HostProfile>,
}

/// A JSON host store backed by one versioned file.
#[derive(Debug, Clone)]
pub struct JsonHostStore {
    path: PathBuf,
}

impl JsonHostStore {
    /// Builds a store at `path`. The file is created on the first write.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the backing path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<Vec<HostProfile>, SpiritError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes =
            fs::read(&self.path).map_err(|err| SpiritError::io("reading host store", &err))?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|err| SpiritError::parse("host store", err))?;
        if value.is_array() {
            return serde_json::from_value(value)
                .map_err(|err| SpiritError::parse("host store (unversioned)", err));
        }
        let object = value
            .as_object()
            .ok_or_else(|| SpiritError::parse("host store", "expected an object or array"))?;
        let hosts_value = object
            .get("hosts")
            .cloned()
            .ok_or_else(|| SpiritError::parse("host store", "missing hosts array"))?;
        if let Some(version) = object
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
        {
            if version != u64::from(HOST_STORE_SCHEMA_VERSION) {
                return Err(SpiritError::new(
                    crate::error::SpiritErrorCode::InvalidInput,
                    format!("host store schema version {version} is not supported"),
                )
                .with_hint("This build reads host store schema 1."));
            }
        }
        serde_json::from_value(hosts_value).map_err(|err| SpiritError::parse("host store", err))
    }

    fn save(&self, hosts: &[HostProfile]) -> Result<(), SpiritError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| SpiritError::io("creating app data dir", &err))?;
        }
        let file = HostFile {
            schema_version: HOST_STORE_SCHEMA_VERSION,
            hosts: hosts.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|err| SpiritError::parse("host store", err))?;
        let temporary = self.path.with_extension("json.tmp");
        {
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut handle = options
                .open(&temporary)
                .map_err(|err| SpiritError::io("creating host store", &err))?;
            handle
                .write_all(&bytes)
                .map_err(|err| SpiritError::io("writing host store", &err))?;
            handle
                .sync_all()
                .map_err(|err| SpiritError::io("syncing host store", &err))?;
        }
        fs::rename(&temporary, &self.path)
            .map_err(|err| SpiritError::io("renaming host store", &err))?;
        Ok(())
    }
}

impl HostStore for JsonHostStore {
    fn list(&self) -> Result<Vec<HostProfile>, SpiritError> {
        self.load()
    }

    fn get(&self, id: &HostId) -> Result<Option<HostProfile>, SpiritError> {
        Ok(self.load()?.into_iter().find(|host| &host.id == id))
    }

    fn add(&self, new: NewHostProfile) -> Result<HostProfile, SpiritError> {
        let mut hosts = self.load()?;
        let profile = HostProfile::create(new, Timestamp::now())?;
        hosts.push(profile.clone());
        self.save(&hosts)?;
        Ok(profile)
    }

    fn update(&self, id: &HostId, patch: HostProfilePatch) -> Result<HostProfile, SpiritError> {
        let mut hosts = self.load()?;
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        let mut profile = hosts[index].clone();
        profile.apply_patch(patch, Timestamp::now())?;
        hosts[index] = profile.clone();
        self.save(&hosts)?;
        Ok(profile)
    }

    fn remove(&self, id: &HostId) -> Result<(), SpiritError> {
        let mut hosts = self.load()?;
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts.remove(index);
        self.save(&hosts)?;
        Ok(())
    }

    fn repin(&self, id: &HostId, pin: HostKeyPin) -> Result<HostProfile, SpiritError> {
        pin.validate()?;
        let mut hosts = self.load()?;
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts[index].host_key = pin;
        hosts[index].updated_at = Timestamp::now();
        let profile = hosts[index].clone();
        self.save(&hosts)?;
        Ok(profile)
    }

    fn mark_connected(&self, id: &HostId, at: Timestamp) -> Result<HostProfile, SpiritError> {
        let mut hosts = self.load()?;
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts[index].last_connected_at = Some(at);
        hosts[index].updated_at = at;
        let profile = hosts[index].clone();
        self.save(&hosts)?;
        Ok(profile)
    }
}

/// An in-memory store used by tests and by headless previews.
#[derive(Debug, Default)]
pub struct InMemoryHostStore {
    hosts: Mutex<Vec<HostProfile>>,
}

impl InMemoryHostStore {
    /// Builds an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Vec<HostProfile>> {
        self.hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl HostStore for InMemoryHostStore {
    fn list(&self) -> Result<Vec<HostProfile>, SpiritError> {
        Ok(self.guard().clone())
    }

    fn get(&self, id: &HostId) -> Result<Option<HostProfile>, SpiritError> {
        Ok(self.guard().iter().find(|host| &host.id == id).cloned())
    }

    fn add(&self, new: NewHostProfile) -> Result<HostProfile, SpiritError> {
        let profile = HostProfile::create(new, Timestamp::now())?;
        self.guard().push(profile.clone());
        Ok(profile)
    }

    fn update(&self, id: &HostId, patch: HostProfilePatch) -> Result<HostProfile, SpiritError> {
        let mut hosts = self.guard();
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts[index].apply_patch(patch, Timestamp::now())?;
        Ok(hosts[index].clone())
    }

    fn remove(&self, id: &HostId) -> Result<(), SpiritError> {
        let mut hosts = self.guard();
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts.remove(index);
        Ok(())
    }

    fn repin(&self, id: &HostId, pin: HostKeyPin) -> Result<HostProfile, SpiritError> {
        pin.validate()?;
        let mut hosts = self.guard();
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts[index].host_key = pin;
        hosts[index].updated_at = Timestamp::now();
        Ok(hosts[index].clone())
    }

    fn mark_connected(&self, id: &HostId, at: Timestamp) -> Result<HostProfile, SpiritError> {
        let mut hosts = self.guard();
        let Some(index) = hosts.iter().position(|host| &host.id == id) else {
            return Err(SpiritError::not_found(&format!("host {id}")));
        };
        hosts[index].last_connected_at = Some(at);
        hosts[index].updated_at = at;
        Ok(hosts[index].clone())
    }
}
