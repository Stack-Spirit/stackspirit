//! App-owned ed25519 SSH keys created locally.
//!
//! One call creates an OpenSSH key pair in an app-private directory, encrypts the
//! private key with a generated 32-byte passphrase, and stores that passphrase only
//! in the local OS keychain under `ssh-key:<name>`. No passphrase enters argv, the
//! process environment, a config file, or any type that implements `Debug`,
//! `Display` or `Serialize`.
//!
//! OpenSSH encoding and bcrypt-PBKDF encryption are delegated to the `ssh-key`
//! crate. StackSpirit never hand-rolls key encoding or cryptography.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ssh_key::rand_core::{OsRng, RngCore};
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};
use zeroize::Zeroizing;

use crate::engine::keychain::{Keychain, KeychainError, SecretString};
use crate::error::{SpiritError, SpiritErrorCode};

/// Prefix for app SSH-key passphrases in the local keychain.
pub const SSH_KEYCHAIN_PREFIX: &str = "ssh-key:";

/// Number of OS-random bytes used for one generated passphrase.
const PASSPHRASE_BYTES: usize = 32;

/// Longest private-key file [`unlock_app_key`] will read.
const MAX_PRIVATE_KEY_BYTES: u64 = 64 * 1024;

/// Longest accepted app key name.
const MAX_NAME_BYTES: usize = 128;

/// Options for [`create_app_key`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppKeyOptions {
    /// Replace an existing key pair and its keychain entry.
    pub force: bool,
}

impl AppKeyOptions {
    /// Builds options that refuse to overwrite an existing key.
    #[must_use]
    pub const fn new() -> Self {
        Self { force: false }
    }

    /// Sets whether an existing key pair may be replaced.
    #[must_use]
    pub const fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

/// A created app-owned SSH key pair. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppKey {
    /// Key name; also the private-key file name.
    pub name: String,
    /// App-private private-key path.
    pub private_key_path: PathBuf,
    /// Public-key path, `<private_key_path>.pub`.
    pub public_key_path: PathBuf,
    /// One-line OpenSSH public key, without a trailing newline.
    pub public_key: String,
    /// `SHA256:...` fingerprint of the public key.
    pub fingerprint: String,
    /// Keychain account that holds the passphrase.
    pub keychain_account: String,
}

/// An app-owned key unlocked with its keychain passphrase. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlockedAppKey {
    /// Key name; also the private-key file name.
    pub name: String,
    /// App-private private-key path.
    pub private_key_path: PathBuf,
    /// One-line OpenSSH public key, without a trailing newline.
    pub public_key: String,
    /// `SHA256:...` fingerprint of the public key.
    pub fingerprint: String,
}

/// Creates one ed25519 app key pair and stores its passphrase in the keychain.
///
/// The directory is created with `0700` permissions on Unix (and tightened if it
/// already exists). The private key is written to `<dir>/<name>` as an encrypted
/// OpenSSH private key with `0600` permissions; `<dir>/<name>.pub` is written with
/// `0600` permissions. A random 32-byte passphrase is generated from the OS random
/// source and stored under `ssh-key:<name>`.
///
/// An existing key pair is refused unless [`AppKeyOptions::force`] is set. With
/// `force`, the old keychain entry is replaced by the new passphrase.
pub fn create_app_key(
    dir: impl AsRef<Path>,
    name: &str,
    keychain: &dyn Keychain,
    options: &AppKeyOptions,
) -> Result<AppKey, SpiritError> {
    validate_name(name)?;
    let dir = dir.as_ref();
    ensure_private_dir(dir)?;

    let private_key_path = dir.join(name);
    let public_key_path = dir.join(format!("{name}.pub"));
    if !options.force && (entry_exists(&private_key_path)? || entry_exists(&public_key_path)?) {
        return Err(SpiritError::new(
            SpiritErrorCode::Conflict,
            format!("SSH app key '{name}' already exists"),
        )
        .with_hint("Choose another name, or create it again with force to replace it."));
    }

    let mut rng = OsRng;
    let mut passphrase_bytes = Zeroizing::new([0_u8; PASSPHRASE_BYTES]);
    rng.fill_bytes(&mut passphrase_bytes[..]);
    let passphrase = Zeroizing::new(hex_encode(&passphrase_bytes[..]));

    let mut private =
        PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(|_| generation_failure())?;
    private.set_comment(format!("StackSpirit {name}"));
    let mut encrypted = private
        .encrypt(&mut rng, passphrase.as_bytes())
        .map_err(|_| generation_failure())?;
    drop(private);
    // `encrypt` keeps the comment inside the encrypted payload but drops it from
    // the public half. Restore it so the returned `.pub` line matches the private
    // key exactly.
    encrypted.set_comment(format!("StackSpirit {name}"));
    let encoded = encrypted
        .to_openssh(LineEnding::LF)
        .map_err(|_| generation_failure())?;
    let public_key = encrypted
        .public_key()
        .to_openssh()
        .map_err(|_| generation_failure())?;
    let fingerprint = encrypted.fingerprint(HashAlg::Sha256).to_string();
    let keychain_account = keychain_account(name);

    // Stage both files beside their final paths, then publish them together. A
    // failure before the rename leaves neither half of the pair behind.
    let staged = StagedPair::create(
        dir,
        &private_key_path,
        &public_key_path,
        encoded.as_bytes(),
        format!("{public_key}\n").as_bytes(),
    )?;

    let previous = keychain
        .get(&keychain_account)
        .map_err(|error| keychain_failure(&error))?;
    keychain
        .set(&keychain_account, &SecretString::new(passphrase.as_str()))
        .map_err(|error| keychain_failure(&error))?;

    if let Err(error) = staged.commit(&private_key_path, &public_key_path, options.force) {
        restore_keychain(keychain, &keychain_account, previous);
        return Err(error);
    }

    Ok(AppKey {
        name: name.to_owned(),
        private_key_path,
        public_key_path,
        public_key,
        fingerprint,
        keychain_account,
    })
}

/// Reads the keychain passphrase, decrypts an app key, and returns its public half.
///
/// A missing passphrase is a typed `keychain` error. A passphrase that does not
/// decrypt the file is a typed `ssh_auth` error. Neither error includes the
/// passphrase or any private-key byte.
pub fn unlock_app_key(
    path: impl AsRef<Path>,
    keychain: &dyn Keychain,
) -> Result<UnlockedAppKey, SpiritError> {
    let path = path.as_ref();
    let name = key_name_from_path(path)?;
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SpiritError::not_found("app SSH key")
        } else {
            SpiritError::io("reading the app SSH key", &error)
        }
    })?;
    if !metadata.is_file() {
        return Err(SpiritError::invalid_input(
            "private key path",
            "must name a regular private-key file",
        ));
    }
    if metadata.len() > MAX_PRIVATE_KEY_BYTES {
        return Err(SpiritError::invalid_input(
            "private key path",
            "the private-key file is unexpectedly large",
        ));
    }

    let bytes = Zeroizing::new(
        fs::read(path).map_err(|error| SpiritError::io("reading the app SSH key", &error))?,
    );
    let private = PrivateKey::from_openssh(bytes.as_slice()).map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::Parse,
            format!("app SSH key '{name}' is not a valid OpenSSH private key"),
        )
        .with_hint("Recreate the key with StackSpirit; do not edit the key file.")
    })?;
    if !private.is_encrypted() {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            format!("app SSH key '{name}' is not passphrase-protected"),
        )
        .with_hint("StackSpirit only unlocks app keys it created with a keychain passphrase."));
    }

    let account = keychain_account(name);
    let Some(passphrase) = keychain
        .get(&account)
        .map_err(|error| keychain_failure(&error))?
    else {
        return Err(SpiritError::new(
            SpiritErrorCode::Keychain,
            format!("no passphrase is stored for app SSH key '{name}'"),
        )
        .with_hint("The OS keychain entry is missing; create the key again or restore it."));
    };

    let unlocked = private.decrypt(passphrase.expose()).map_err(|_| {
        SpiritError::new(
            SpiritErrorCode::SshAuth,
            format!("the saved passphrase did not unlock app SSH key '{name}'"),
        )
        .with_hint("The key file may have been replaced. Create it again with force.")
    })?;
    if unlocked.algorithm() != Algorithm::Ed25519 {
        return Err(SpiritError::new(
            SpiritErrorCode::InvalidInput,
            format!("app SSH key '{name}' is not ed25519"),
        )
        .with_hint("StackSpirit creates and unlocks ed25519 app keys only."));
    }

    let public_key = unlocked
        .public_key()
        .to_openssh()
        .map_err(|_| generation_failure())?;
    let fingerprint = unlocked.fingerprint(HashAlg::Sha256).to_string();
    Ok(UnlockedAppKey {
        name: name.to_owned(),
        private_key_path: path.to_path_buf(),
        public_key,
        fingerprint,
    })
}

fn keychain_account(name: &str) -> String {
    format!("{SSH_KEYCHAIN_PREFIX}{name}")
}

fn validate_name(name: &str) -> Result<(), SpiritError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(SpiritError::invalid_input(
            "key name",
            "use 1-128 ASCII letters, digits, '.', '_' or '-'",
        ));
    }
    if name.starts_with('.') || name.starts_with('-') || name.ends_with(".pub") {
        return Err(SpiritError::invalid_input(
            "key name",
            "must not start with '.' or '-', or end with '.pub'",
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(SpiritError::invalid_input(
            "key name",
            "use ASCII letters, digits, '.', '_' or '-' only",
        ));
    }
    Ok(())
}

fn key_name_from_path(path: &Path) -> Result<&str, SpiritError> {
    let name = path.file_name().and_then(OsStr::to_str).ok_or_else(|| {
        SpiritError::invalid_input("private key path", "must end in a UTF-8 key name")
    })?;
    validate_name(name)?;
    Ok(name)
}

fn ensure_private_dir(dir: &Path) -> Result<(), SpiritError> {
    if dir.as_os_str().is_empty() {
        return Err(SpiritError::invalid_input(
            "key directory",
            "must not be empty",
        ));
    }
    match fs::metadata(dir) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(SpiritError::invalid_input(
                "key directory",
                "must be a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_private_dir(dir)?,
        Err(error) => {
            return Err(SpiritError::io("reading the app key directory", &error));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| SpiritError::io("securing the app key directory", &error))?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<(), SpiritError> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(dir)
        .map_err(|error| SpiritError::io("creating the app key directory", &error))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<(), SpiritError> {
    fs::create_dir_all(dir)
        .map_err(|error| SpiritError::io("creating the app key directory", &error))
}

fn entry_exists(path: &Path) -> Result<bool, SpiritError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SpiritError::io(
            "checking for an existing app SSH key",
            &error,
        )),
    }
}

struct StagedPair {
    private: PathBuf,
    public: PathBuf,
}

impl StagedPair {
    fn create(
        dir: &Path,
        private_final: &Path,
        public_final: &Path,
        private_bytes: &[u8],
        public_bytes: &[u8],
    ) -> Result<Self, SpiritError> {
        let private = write_staged_file(
            dir,
            private_final,
            private_bytes,
            "writing the app SSH private key",
        )?;
        let public = match write_staged_file(
            dir,
            public_final,
            public_bytes,
            "writing the app SSH public key",
        ) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_file(&private);
                return Err(error);
            }
        };
        Ok(Self { private, public })
    }

    fn commit(
        self,
        private_final: &Path,
        public_final: &Path,
        force: bool,
    ) -> Result<(), SpiritError> {
        replace_file(&self.private, private_final, force)
            .map_err(|error| SpiritError::io("installing the app SSH private key", &error))?;
        if let Err(error) = replace_file(&self.public, public_final, force) {
            let _ = fs::remove_file(private_final);
            return Err(SpiritError::io("installing the app SSH public key", &error));
        }
        Ok(())
    }
}

impl Drop for StagedPair {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.private);
        let _ = fs::remove_file(&self.public);
    }
}

fn write_staged_file(
    dir: &Path,
    final_path: &Path,
    bytes: &[u8],
    context: &str,
) -> Result<PathBuf, SpiritError> {
    let stem = final_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("key");
    for _ in 0..8 {
        let staged = dir.join(format!(".{stem}.{}.tmp", ulid::Ulid::generate()));
        let mut file = match create_private_file(&staged) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(SpiritError::io(context, &error)),
        };
        let result = file.write_all(bytes).and_then(|()| file.sync_all());
        if let Err(error) = result {
            drop(file);
            let _ = fs::remove_file(&staged);
            return Err(SpiritError::io(context, &error));
        }
        return Ok(staged);
    }
    Err(SpiritError::new(
        SpiritErrorCode::Internal,
        "could not stage the app SSH key file",
    ))
}

fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error);
        }
    }
    Ok(file)
}

fn replace_file(staged: &Path, final_path: &Path, force: bool) -> std::io::Result<()> {
    let _ = force;
    // `rename` replaces an existing file on Unix. Windows needs the old entry
    // removed first; a no-force create still lets the rename fail on a race.
    #[cfg(windows)]
    {
        if force {
            match fs::remove_file(final_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    fs::rename(staged, final_path)
}

fn restore_keychain(keychain: &dyn Keychain, account: &str, previous: Option<SecretString>) {
    match previous {
        Some(secret) => {
            let _ = keychain.set(account, &secret);
        }
        None => {
            let _ = keychain.delete(account);
        }
    }
}

fn generation_failure() -> SpiritError {
    SpiritError::new(
        SpiritErrorCode::Internal,
        "could not create the ed25519 app SSH key",
    )
    .with_hint("Retry; if it keeps failing, check the OS random source.")
}

fn keychain_failure(error: &KeychainError) -> SpiritError {
    SpiritError::new(SpiritErrorCode::Keychain, error.to_string())
        .with_hint("Unlock the OS keychain, then retry.")
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
