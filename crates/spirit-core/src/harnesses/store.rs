//! Custom harness persistence: one strict TOML file per harness under
//! `~/.stackspirit/harnesses/<id>.toml`.
//!
//! Writes are atomic and owner-only on Unix. The manifest schema has no credential
//! field, and an unknown field such as `api_key` or `oauth_token` is rejected instead
//! of stored.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{SpiritError, SpiritErrorCode};
use crate::harnesses::manifest::{HarnessManifest, builtin_harnesses};
use crate::harnesses::toml::{manifest_from_toml, manifest_to_toml};

/// Built-ins plus the valid custom entries, and safe warnings for files that were
/// skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessListView {
    /// Built-ins first, then custom entries sorted by id.
    pub harnesses: Vec<HarnessManifest>,
    /// One safe warning per skipped custom file. Never contains file contents.
    pub warnings: Vec<String>,
}

/// Resolves the documented custom manifest directory, `~/.stackspirit/harnesses`.
pub fn default_harness_dir() -> Result<PathBuf, SpiritError> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
        .ok_or_else(|| {
            SpiritError::new(
                SpiritErrorCode::Io,
                "could not resolve the home directory for harness manifests",
            )
        })?;
    Ok(PathBuf::from(home).join(".stackspirit").join("harnesses"))
}

/// The custom harness registry.
#[derive(Debug, Clone)]
pub struct HarnessRegistry {
    dir: PathBuf,
}

impl HarnessRegistry {
    /// Builds a registry at an explicit directory. Tests and headless callers use this;
    /// the desktop app uses [`HarnessRegistry::for_home`].
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Builds the registry at `~/.stackspirit/harnesses`.
    pub fn for_home() -> Result<Self, SpiritError> {
        Ok(Self::new(default_harness_dir()?))
    }

    /// Returns the custom manifest directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Returns the path for one custom manifest. The id is validated first.
    pub fn manifest_path(&self, id: &str) -> Result<PathBuf, SpiritError> {
        HarnessManifest::validate_id(id)?;
        if HarnessManifest::is_builtin_id(id) {
            return Err(SpiritError::invalid_input(
                "id",
                "is reserved for a built-in harness",
            ));
        }
        Ok(self.dir.join(format!("{id}.toml")))
    }

    /// Lists every built-in and valid custom manifest. A broken custom file is skipped
    /// with a warning instead of blocking the rest of the registry.
    pub fn list(&self) -> Result<HarnessListView, SpiritError> {
        let mut warnings = Vec::new();
        let mut customs = Vec::new();
        if self.dir.exists() {
            let entries = fs::read_dir(&self.dir)
                .map_err(|error| SpiritError::io("reading harness manifests", &error))?;
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        warnings.push(format!(
                            "A harness file could not be read: {}",
                            crate::error::redact(&error.to_string())
                        ));
                        continue;
                    }
                };
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "harness.toml".to_owned());
                match load_custom_file(&path) {
                    Ok(manifest) => {
                        if HarnessManifest::is_builtin_id(&manifest.id) {
                            warnings.push(format!(
                                "{name}: id '{}' is reserved for a built-in harness",
                                manifest.id
                            ));
                        } else {
                            customs.push(manifest);
                        }
                    }
                    Err(error) => warnings.push(format!("{name}: {}", error.message)),
                }
            }
        }
        customs.sort_by(|left, right| left.id.cmp(&right.id));

        let mut harnesses = builtin_harnesses();
        harnesses.extend(customs);
        Ok(HarnessListView {
            harnesses,
            warnings,
        })
    }

    /// Returns one built-in or custom manifest, or `None` when the id is not registered.
    pub fn get(&self, id: &str) -> Result<Option<HarnessManifest>, SpiritError> {
        HarnessManifest::validate_id(id)?;
        if let Some(builtin) = builtin_harnesses()
            .into_iter()
            .find(|manifest| manifest.id == id)
        {
            return Ok(Some(builtin));
        }
        if HarnessManifest::is_builtin_id(id) {
            return Ok(None);
        }
        let path = self.dir.join(format!("{id}.toml"));
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(load_custom_file(&path)?))
    }

    /// Saves one custom manifest atomically. A built-in id and `builtin: true` are
    /// rejected, so a custom file can never shadow or impersonate a preset.
    pub fn save_custom(&self, manifest: HarnessManifest) -> Result<HarnessManifest, SpiritError> {
        if manifest.builtin {
            return Err(SpiritError::invalid_input(
                "builtin",
                "a custom harness must have builtin = false",
            ));
        }
        if HarnessManifest::is_builtin_id(&manifest.id) {
            return Err(SpiritError::invalid_input(
                "id",
                "is reserved for a built-in harness",
            ));
        }
        manifest.validate()?;
        fs::create_dir_all(&self.dir)
            .map_err(|error| SpiritError::io("creating the harness directory", &error))?;
        restrict_directory(&self.dir);
        let path = self.dir.join(format!("{}.toml", manifest.id));
        let temporary = self.dir.join(format!("{}.toml.tmp", manifest.id));
        let bytes = manifest_to_toml(&manifest);
        write_atomic(&temporary, &path, bytes.as_bytes())?;
        Ok(manifest)
    }

    /// Deletes one custom manifest. Built-ins are refused and a missing custom file
    /// returns `false` instead of an error.
    pub fn delete_custom(&self, id: &str) -> Result<bool, SpiritError> {
        HarnessManifest::validate_id(id)?;
        if HarnessManifest::is_builtin_id(id) {
            return Err(SpiritError::invalid_input(
                "id",
                "is a built-in harness and cannot be deleted",
            ));
        }
        let path = self.dir.join(format!("{id}.toml"));
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)
            .map_err(|error| SpiritError::io("deleting the harness manifest", &error))?;
        Ok(true)
    }
}

fn load_custom_file(path: &Path) -> Result<HarnessManifest, SpiritError> {
    let bytes =
        fs::read(path).map_err(|error| SpiritError::io("reading a harness manifest", &error))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| SpiritError::parse("harness manifest", "file is not valid UTF-8"))?;
    let manifest = manifest_from_toml(text)?;
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if manifest.id != stem {
        return Err(SpiritError::invalid_input(
            "id",
            "must match the manifest file name",
        ));
    }
    Ok(manifest)
}

fn write_atomic(temporary: &Path, destination: &Path, bytes: &[u8]) -> Result<(), SpiritError> {
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options
            .open(temporary)
            .map_err(|error| SpiritError::io("creating a harness manifest", &error))?;
        handle
            .write_all(bytes)
            .map_err(|error| SpiritError::io("writing a harness manifest", &error))?;
        handle
            .sync_all()
            .map_err(|error| SpiritError::io("syncing a harness manifest", &error))?;
    }
    fs::rename(temporary, destination)
        .map_err(|error| SpiritError::io("replacing the harness manifest", &error))?;
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
    use crate::harnesses::manifest::HarnessAuthMode;

    fn custom() -> HarnessManifest {
        let mut manifest = builtin_harnesses().remove(0);
        manifest.id = "my-harness".to_owned();
        manifest.name = "My Harness".to_owned();
        manifest.auth_modes = vec![HarnessAuthMode::ApiKey];
        manifest.builtin = false;
        manifest
    }

    #[test]
    fn saves_lists_and_deletes_a_custom_manifest_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let registry = HarnessRegistry::new(directory.path());
        let saved = registry.save_custom(custom()).unwrap();
        assert_eq!(saved.id, "my-harness");
        let path = directory.path().join("my-harness.toml");
        assert!(path.exists());
        assert!(!directory.path().join("my-harness.toml.tmp").exists());
        let listed = registry.list().unwrap();
        assert!(listed.harnesses.iter().any(|item| item.id == "my-harness"));
        assert!(listed.harnesses.len() > builtin_harnesses().len());
        assert!(listed.warnings.is_empty());
        let loaded = registry.get("my-harness").unwrap().unwrap();
        assert_eq!(loaded, saved);
        assert!(registry.delete_custom("my-harness").unwrap());
        assert!(!registry.delete_custom("my-harness").unwrap());
    }

    #[test]
    fn refuses_builtin_ids_and_builtin_claims() {
        let directory = tempfile::tempdir().unwrap();
        let registry = HarnessRegistry::new(directory.path());
        let mut manifest = custom();
        manifest.id = "opencode".to_owned();
        assert!(registry.save_custom(manifest).is_err());
        let mut manifest = custom();
        manifest.builtin = true;
        assert!(registry.save_custom(manifest).is_err());
        assert!(registry.delete_custom("opencode").is_err());
    }

    #[test]
    fn skips_a_broken_custom_file_with_a_safe_warning() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("broken.toml"),
            "id = \"broken\"\napi_key = \"do-not-store\"\n",
        )
        .unwrap();
        let listed = HarnessRegistry::new(directory.path()).list().unwrap();
        assert_eq!(listed.harnesses.len(), builtin_harnesses().len());
        assert_eq!(listed.warnings.len(), 1);
        assert!(listed.warnings[0].contains("broken.toml"));
        assert!(!listed.warnings[0].contains("do-not-store"));
    }

    #[cfg(unix)]
    #[test]
    fn writes_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let registry = HarnessRegistry::new(directory.path());
        registry.save_custom(custom()).unwrap();
        let mode = fs::metadata(directory.path().join("my-harness.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
