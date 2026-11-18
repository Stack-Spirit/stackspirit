//! App-owned SSH key creation: OpenSSH parsing, public/private matching, file
//! permissions, keychain passphrase storage, unlock behavior and force semantics.

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Mutex, MutexGuard};

use ssh_key::{Algorithm, HashAlg, PrivateKey};
use tempfile::TempDir;

use spirit_core::engine::keychain::{Keychain, KeychainError, KeychainErrorClass, SecretString};
use spirit_core::error::SpiritErrorCode;
use spirit_core::ssh::{
    AppKey, AppKeyOptions, SSH_KEYCHAIN_PREFIX, create_app_key, unlock_app_key,
};

const NAME: &str = "id_ed25519";

/// A fake keychain that records every `set` call, so tests can prove the
/// passphrase was written exactly once per create.
#[derive(Default)]
struct RecordingKeychain {
    entries: Mutex<BTreeMap<String, String>>,
    sets: Mutex<Vec<String>>,
}

impl RecordingKeychain {
    fn set_calls(&self) -> Vec<String> {
        lock(&self.sets).clone()
    }

    fn stored(&self, account: &str) -> Option<String> {
        lock(&self.entries).get(account).cloned()
    }

    fn overwrite(&self, account: &str, secret: &str) {
        lock(&self.entries).insert(account.to_owned(), secret.to_owned());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Keychain for RecordingKeychain {
    fn set(&self, account: &str, secret: &SecretString) -> Result<(), KeychainError> {
        lock(&self.sets).push(account.to_owned());
        lock(&self.entries).insert(account.to_owned(), secret.expose().to_owned());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<SecretString>, KeychainError> {
        Ok(lock(&self.entries)
            .get(account)
            .cloned()
            .map(SecretString::new))
    }

    fn delete(&self, account: &str) -> Result<(), KeychainError> {
        let removed = lock(&self.entries).remove(account);
        if removed.is_some() {
            Ok(())
        } else {
            Err(KeychainError::new(account, KeychainErrorClass::NotFound))
        }
    }

    fn list(&self) -> Result<Vec<String>, KeychainError> {
        Ok(lock(&self.entries).keys().cloned().collect())
    }
}

fn stored_passphrase(keychain: &RecordingKeychain, key: &AppKey) -> String {
    keychain
        .stored(&key.keychain_account)
        .expect("the keychain has the generated passphrase")
}

fn create(work: &TempDir, keychain: &RecordingKeychain, options: &AppKeyOptions) -> AppKey {
    create_app_key(work.path(), NAME, keychain, options).unwrap()
}

#[test]
fn created_key_parses_as_ed25519_and_the_public_line_matches_the_private_key() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let key = create(&work, &keychain, &AppKeyOptions::default());

    let passphrase = stored_passphrase(&keychain, &key);
    let private_text = fs::read_to_string(&key.private_key_path).unwrap();
    assert!(private_text.contains("BEGIN OPENSSH PRIVATE KEY"));

    let private = PrivateKey::from_openssh(private_text.as_bytes()).unwrap();
    assert!(private.is_encrypted(), "the private key must be encrypted");
    let decrypted = private.decrypt(passphrase.as_bytes()).unwrap();
    assert_eq!(decrypted.algorithm(), Algorithm::Ed25519);
    assert_eq!(
        decrypted.public_key().to_openssh().unwrap(),
        key.public_key,
        "the returned public line must come from the private key"
    );
    assert_eq!(
        decrypted.fingerprint(HashAlg::Sha256).to_string(),
        key.fingerprint
    );
    assert!(key.fingerprint.starts_with("SHA256:"));

    assert_eq!(key.private_key_path, work.path().join(NAME));
    assert_eq!(key.public_key_path, work.path().join(format!("{NAME}.pub")));
    let public_file = fs::read_to_string(&key.public_key_path).unwrap();
    assert_eq!(public_file.trim_end(), key.public_key);
    assert!(public_file.ends_with('\n'));
}

#[cfg(unix)]
#[test]
fn created_key_files_are_0600_and_the_directory_is_0700() {
    use std::os::unix::fs::PermissionsExt;

    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let dir = work.path().join("ssh");
    let key = create_app_key(&dir, NAME, &keychain, &AppKeyOptions::default()).unwrap();

    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&key.private_key_path), 0o600);
    assert_eq!(mode(&key.public_key_path), 0o600);
}

#[test]
fn the_keychain_gets_one_passphrase_and_the_file_never_contains_it() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let key = create(&work, &keychain, &AppKeyOptions::default());

    assert_eq!(
        key.keychain_account,
        format!("{SSH_KEYCHAIN_PREFIX}{NAME}"),
        "the passphrase lives in the namespaced app-key account"
    );
    assert_eq!(
        keychain.set_calls(),
        vec![key.keychain_account.clone()],
        "one create writes the passphrase to the keychain exactly once"
    );
    let passphrase = stored_passphrase(&keychain, &key);
    assert!(
        passphrase.len() >= 32,
        "the passphrase must carry at least 32 bytes of randomness"
    );

    let private_text = fs::read_to_string(&key.private_key_path).unwrap();
    assert!(!private_text.contains(&passphrase));
    assert!(!format!("{key:?}").contains(&passphrase));

    let unlocked = unlock_app_key(&key.private_key_path, &keychain).unwrap();
    assert_eq!(unlocked.public_key, key.public_key);
    assert_eq!(unlocked.fingerprint, key.fingerprint);
    assert!(!format!("{unlocked:?}").contains(&passphrase));
}

#[test]
fn a_missing_passphrase_is_a_typed_keychain_error_without_secret_text() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let key = create(&work, &keychain, &AppKeyOptions::default());
    let passphrase = stored_passphrase(&keychain, &key);
    keychain.delete(&key.keychain_account).unwrap();

    let error = unlock_app_key(&key.private_key_path, &keychain).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Keychain);
    assert!(!error.message.contains(&passphrase));
    assert!(!format!("{error:?}").contains(&passphrase));
}

#[test]
fn a_wrong_passphrase_is_a_typed_ssh_auth_error_without_secret_text() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let key = create(&work, &keychain, &AppKeyOptions::default());
    let wrong = "wrong-passphrase-0123456789abcdef";
    keychain.overwrite(&key.keychain_account, wrong);

    let error = unlock_app_key(&key.private_key_path, &keychain).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::SshAuth);
    assert!(!error.message.contains(wrong));
    assert!(!error.hint.as_deref().unwrap_or_default().contains(wrong));
    assert!(!format!("{error:?}").contains(wrong));
}

#[test]
fn a_second_create_without_force_is_refused_and_leaves_the_first_key_untouched() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let first = create(&work, &keychain, &AppKeyOptions::default());
    let first_private = fs::read(&first.private_key_path).unwrap();
    let first_public = fs::read(&first.public_key_path).unwrap();
    let first_passphrase = stored_passphrase(&keychain, &first);

    let error =
        create_app_key(work.path(), NAME, &keychain, &AppKeyOptions::default()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::Conflict);
    assert_eq!(fs::read(&first.private_key_path).unwrap(), first_private);
    assert_eq!(fs::read(&first.public_key_path).unwrap(), first_public);
    assert_eq!(stored_passphrase(&keychain, &first), first_passphrase);
    assert_eq!(keychain.set_calls().len(), 1);

    let second = create(&work, &keychain, &AppKeyOptions::default().with_force(true));
    assert_eq!(second.keychain_account, first.keychain_account);
    assert_eq!(
        keychain.set_calls(),
        vec![
            first.keychain_account.clone(),
            second.keychain_account.clone()
        ],
        "force writes a new keychain entry"
    );
    let second_passphrase = stored_passphrase(&keychain, &second);
    assert_ne!(second_passphrase, first_passphrase);
    assert_ne!(fs::read(&second.private_key_path).unwrap(), first_private);
    assert_ne!(fs::read(&second.public_key_path).unwrap(), first_public);
    assert_ne!(second.fingerprint, first.fingerprint);

    let old_private = PrivateKey::from_openssh(first_private.as_slice()).unwrap();
    assert!(
        old_private.decrypt(second_passphrase.as_bytes()).is_err(),
        "the replaced key cannot be unlocked by the new passphrase"
    );
    let unlocked = unlock_app_key(&second.private_key_path, &keychain).unwrap();
    assert_eq!(unlocked.fingerprint, second.fingerprint);
}

#[test]
fn fingerprints_are_stable_for_one_key_and_change_for_a_new_key() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();
    let first = create(&work, &keychain, &AppKeyOptions::default());

    let first_unlock_a = unlock_app_key(&first.private_key_path, &keychain).unwrap();
    let first_unlock_b = unlock_app_key(&first.private_key_path, &keychain).unwrap();
    assert_eq!(first_unlock_a.fingerprint, first.fingerprint);
    assert_eq!(first_unlock_b.fingerprint, first.fingerprint);

    let second = create(&work, &keychain, &AppKeyOptions::default().with_force(true));
    let second_unlock = unlock_app_key(&second.private_key_path, &keychain).unwrap();
    assert_eq!(second_unlock.fingerprint, second.fingerprint);
    assert_ne!(second_unlock.fingerprint, first.fingerprint);
}

#[test]
fn unsafe_key_names_are_refused_without_touching_disk_or_keychain() {
    let work = tempfile::tempdir().unwrap();
    let keychain = RecordingKeychain::default();

    for name in [
        "",
        ".hidden",
        "-flag",
        "../escape",
        "a/b",
        "id key",
        "id.pub",
    ] {
        let error =
            create_app_key(work.path(), name, &keychain, &AppKeyOptions::default()).unwrap_err();
        assert_eq!(
            error.code,
            SpiritErrorCode::InvalidInput,
            "{name:?} must be refused before any write"
        );
    }

    assert!(keychain.set_calls().is_empty());
    assert_eq!(
        fs::read_dir(work.path()).unwrap().count(),
        0,
        "an invalid name must not create a directory or file"
    );
}
