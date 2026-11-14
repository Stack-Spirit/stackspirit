//! Host profile persistence, atomic writes, migration and host-key pinning.

use std::path::PathBuf;

use spirit_core::error::SpiritErrorCode;
use spirit_core::host::{
    HostDraft, HostKeyPin, HostProfile, HostProfilePatch, HostStore, JsonHostStore, NewHostProfile,
    verify_pin_against_scan,
};
use spirit_core::ssh::fingerprint::{parse_fingerprint_output, parse_keyscan_output};
use spirit_core::ssh::known_hosts::{contains_key, write_known_hosts};
use spirit_core::ssh::{ScannedHostKey, SshTarget};
use spirit_core::time::Timestamp;

fn pin() -> HostKeyPin {
    HostKeyPin {
        algorithm: "ssh-ed25519".to_owned(),
        sha256: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789ABCDEFG".to_owned(),
        pinned_at: Timestamp::parse("2026-11-14T12:00:00.000Z").unwrap(),
    }
}

fn target() -> SshTarget {
    SshTarget::new("owner", "192.0.2.10", 2222).unwrap()
}

fn draft(label: &str) -> HostDraft {
    HostDraft {
        label: label.to_owned(),
        ssh: target(),
        identity_file: Some(PathBuf::from("/tmp/opencode/does-not-exist")),
        vowp_home: Some("/srv/vowp-home".to_owned()),
        default_project: Some("vowp".to_owned()),
    }
}

fn new_host(label: &str) -> NewHostProfile {
    NewHostProfile {
        draft: draft(label),
        host_key: pin(),
    }
}

fn scanned(sha256: &str) -> ScannedHostKey {
    ScannedHostKey {
        algorithm: "ssh-ed25519".to_owned(),
        sha256: sha256.to_owned(),
        public_key_base64: "AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyForFixturesOnly".to_owned(),
        preferred: true,
    }
}

#[test]
fn host_store_writes_mode_0600_and_atomic_rename() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hosts.json");
    let store = JsonHostStore::new(&path);
    let profile = store.add(new_host("example-vps")).unwrap();
    assert_eq!(profile.label, "example-vps");
    assert_eq!(profile.ssh.user, "owner");
    assert!(path.exists());
    let temporary = path.with_extension("json.tmp");
    assert!(!temporary.exists(), "the atomic temp file must be renamed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["hosts"].as_array().unwrap().len(), 1);
}

#[test]
fn host_store_migrates_unversioned_profile_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hosts.json");
    let legacy = HostProfile::create(new_host("legacy"), Timestamp::now()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&vec![legacy.clone()]).unwrap()).unwrap();
    let store = JsonHostStore::new(&path);
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].label, "legacy");
    let added = store.add(new_host("second")).unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["hosts"].as_array().unwrap().len(), 2);

    // An unversioned object with a hosts array is also accepted.
    let path2 = directory.path().join("hosts2.json");
    std::fs::write(
        &path2,
        serde_json::to_vec(&serde_json::json!({"hosts": [legacy]})).unwrap(),
    )
    .unwrap();
    assert_eq!(JsonHostStore::new(&path2).list().unwrap().len(), 1);
    assert_eq!(store.get(&added.id).unwrap().unwrap().label, "second");
}

#[test]
fn identity_file_path_is_stored_but_never_read_as_key_material() {
    let directory = tempfile::tempdir().unwrap();
    let store = JsonHostStore::new(directory.path().join("hosts.json"));
    let identity_path = directory.path().join("not-a-key");
    let mut input = new_host("identity-path");
    input.draft.identity_file = Some(identity_path.clone());
    let profile = store.add(input).unwrap();
    assert_eq!(profile.identity_file, Some(identity_path.clone()));
    assert!(!identity_path.exists());
    let text = std::fs::read_to_string(store.path()).unwrap();
    assert!(text.contains("not-a-key"));
    assert!(!text.contains("PRIVATE KEY"));
    assert!(!text.contains("ssh-ed25519 AAAA"));
}

#[test]
fn first_connect_requires_a_pinned_fingerprint() {
    let mut input = new_host("no-pin");
    input.host_key = HostKeyPin {
        algorithm: String::new(),
        sha256: String::new(),
        pinned_at: Timestamp::epoch(),
    };
    let error = HostProfile::create(input, Timestamp::now()).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::InvalidInput);

    let mut profile = HostProfile::create(new_host("valid"), Timestamp::now()).unwrap();
    profile.host_key.sha256 = String::new();
    assert!(profile.validate().is_err());

    let mut input = new_host("bad-label");
    input.draft.label = "   ".to_owned();
    assert!(HostProfile::create(input, Timestamp::now()).is_err());
}

#[test]
fn host_key_mismatch_blocks_connect() {
    let pin = pin();
    let matching = scanned(&pin.sha256);
    verify_pin_against_scan(&pin, &[matching]).unwrap();
    let changed = scanned("SHA256:ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ");
    let error = verify_pin_against_scan(&pin, &[changed]).unwrap_err();
    assert_eq!(error.code, SpiritErrorCode::HostKeyMismatch);
    assert!(error.message.contains(&pin.sha256));
    assert!(error.message.contains("SHA256:ZZZZ"));
}

#[test]
fn host_store_update_remove_and_repin() {
    let directory = tempfile::tempdir().unwrap();
    let store = JsonHostStore::new(directory.path().join("hosts.json"));
    let profile = store.add(new_host("before")).unwrap();
    let patch = HostProfilePatch {
        label: Some("after".to_owned()),
        default_project: Some(None),
        ..HostProfilePatch::default()
    };
    let updated = store.update(&profile.id, patch).unwrap();
    assert_eq!(updated.label, "after");
    assert_eq!(updated.default_project, None);
    assert_eq!(store.get(&profile.id).unwrap().unwrap().label, "after");

    let new_pin = HostKeyPin {
        sha256: "SHA256:NEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEWNEW".to_owned(),
        ..pin()
    };
    let repinned = store.repin(&profile.id, new_pin.clone()).unwrap();
    assert_eq!(repinned.host_key.sha256, new_pin.sha256);
    store.remove(&profile.id).unwrap();
    assert!(store.list().unwrap().is_empty());
    assert_eq!(
        store.remove(&profile.id).unwrap_err().code,
        SpiritErrorCode::NotFound
    );
}

#[test]
fn known_hosts_is_written_with_mode_0600_and_pin_detection_works() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("known_hosts");
    let key = scanned(&pin().sha256);
    let line = spirit_core::ssh::known_hosts::known_hosts_line(&target(), &key);
    write_known_hosts(&path, &[line]).unwrap();
    assert!(contains_key(&path, &target(), &key).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("[192.0.2.10]:2222 ssh-ed25519 "));
}

#[test]
fn fingerprint_helpers_parse_real_shapes() {
    let keyscan = "# 192.0.2.10:2222 SSH-2.0-OpenSSH_9.6\n\
                   192.0.2.10 ssh-ed25519 AAAAC3NzaFake\n\
                   192.0.2.10 rsa-sha2-512 AAAAB3NzaFake\n";
    let keys = parse_keyscan_output(keyscan);
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].0, "ssh-ed25519");
    let (sha, algorithm) =
        parse_fingerprint_output("256 SHA256:AbCdEf owner@host (ED25519)").unwrap();
    assert_eq!(sha, "SHA256:AbCdEf");
    assert_eq!(algorithm, "ssh-ed25519");
    assert!(parse_fingerprint_output("no fingerprint here").is_none());
}

#[tokio::test]
async fn host_probe_reports_every_stage_and_blocks_without_a_pin() {
    use spirit_core::host::{HostProbeInput, probe_host};
    use spirit_core::ssh::command::CommandOutput;
    use spirit_core::ssh::{FakeCommandRunner, SshOptions};
    use std::sync::Arc;

    let fresh = include_str!("../fixtures/vowp/fleet-status-fresh.json");
    let runner = FakeCommandRunner::new(vec![
        CommandOutput::ok("vowp 0.4.0\n"),
        CommandOutput::ok(fresh),
    ]);
    let input = HostProbeInput {
        target: target(),
        options: SshOptions::new("/tmp/opencode/known_hosts"),
        vowp_home: Some("/srv/vowp-home".to_owned()),
        pin: Some(pin()),
    };
    let probe = probe_host(Arc::new(runner), &input).await;
    assert!(probe.reachable);
    assert!(probe.auth_ok);
    assert!(probe.host_key_ok);
    assert_eq!(probe.vowp_version.as_deref(), Some("0.4.0"));
    assert_eq!(probe.stages.len(), 5);
    assert!(probe.save_allowed());
    assert!(probe.stage("supervisor").unwrap().ok);

    let runner = FakeCommandRunner::new(vec![CommandOutput::ok(fresh)]);
    let input = HostProbeInput { pin: None, ..input };
    let probe = probe_host(Arc::new(runner), &input).await;
    assert!(!probe.host_key_ok);
    assert!(!probe.save_allowed());
    assert!(!probe.stage("host_key").unwrap().ok);
}

#[tokio::test]
async fn host_probe_classifies_a_changed_host_key() {
    use spirit_core::error::SpiritErrorCode;
    use spirit_core::host::{HostProbeInput, probe_host};
    use spirit_core::ssh::command::CommandOutput;
    use spirit_core::ssh::{FakeCommandRunner, SshOptions};
    use std::sync::Arc;

    let runner = FakeCommandRunner::new(vec![CommandOutput::fail(
        255,
        "Host key verification failed.",
    )]);
    let input = HostProbeInput {
        target: target(),
        options: SshOptions::new("/tmp/opencode/known_hosts"),
        vowp_home: None,
        pin: Some(pin()),
    };
    let probe = probe_host(Arc::new(runner), &input).await;
    assert!(!probe.host_key_ok);
    assert!(!probe.save_allowed());
    assert!(!probe.stage("host_key").unwrap().ok || !probe.stage("ssh").unwrap().ok);

    let output =
        spirit_core::ssh::command::CommandOutput::fail(255, "Host key verification failed.");
    assert_eq!(
        spirit_core::host::classify_ssh_failure(&output).code,
        SpiritErrorCode::HostKeyMismatch
    );
}

#[test]
fn nullable_patch_fields_distinguish_absent_from_null() {
    let absent: HostProfilePatch = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.default_project, None);
    assert_eq!(absent.identity_file, None);
    let cleared: HostProfilePatch = serde_json::from_str(r#"{"default_project":null}"#).unwrap();
    assert_eq!(cleared.default_project, Some(None));
    let changed: HostProfilePatch = serde_json::from_str(r#"{"vowp_home":"/srv/other"}"#).unwrap();
    assert_eq!(changed.vowp_home, Some(Some("/srv/other".to_owned())));
    let empty: HostProfilePatch = serde_json::from_str(r#"{"vowp_home":""}"#).unwrap();
    let profile = HostProfile::create(new_host("empty-home"), Timestamp::now()).unwrap();
    let mut profile = profile;
    profile
        .apply_patch(empty, Timestamp::parse("2026-11-14T12:00:01.000Z").unwrap())
        .unwrap();
    assert_eq!(profile.vowp_home, None);
}

#[test]
fn patch_serialization_omits_unchanged_nullable_fields() {
    let patch = HostProfilePatch {
        label: Some("new-label".to_owned()),
        ..HostProfilePatch::default()
    };
    let value = serde_json::to_value(&patch).unwrap();
    assert_eq!(value["label"], "new-label");
    assert!(
        value.get("default_project").is_none(),
        "an unchanged nullable field must be omitted, not sent as null: {value}"
    );
    let round_tripped: HostProfilePatch = serde_json::from_value(value).unwrap();
    assert_eq!(round_tripped, patch);

    let cleared = HostProfilePatch {
        default_project: Some(None),
        ..HostProfilePatch::default()
    };
    let value = serde_json::to_value(&cleared).unwrap();
    assert!(value["default_project"].is_null());
    let round_tripped: HostProfilePatch = serde_json::from_value(value).unwrap();
    assert_eq!(round_tripped.default_project, Some(None));
}

#[tokio::test]
async fn host_key_scan_prefers_ed25519_and_fingerprints_every_key() {
    use spirit_core::ssh::command::CommandOutput;
    use spirit_core::ssh::{FakeCommandRunner, scan_host_keys};
    use std::sync::Arc;
    use std::time::Duration;

    let keyscan = "192.0.2.10 ssh-rsa AAAAB3NzaFakeRsa\n\
                   192.0.2.10 ssh-ed25519 AAAAC3NzaFakeEd\n";
    let runner = FakeCommandRunner::new(vec![
        CommandOutput::ok(keyscan),
        CommandOutput::ok("256 SHA256:RRRRRRRR owner@host (RSA)\n"),
        CommandOutput::ok("256 SHA256:EEEEEEEE owner@host (ED25519)\n"),
    ]);
    let runner = Arc::new(runner);
    let scan = scan_host_keys(runner.as_ref(), &target(), Duration::from_secs(8))
        .await
        .unwrap();
    assert_eq!(scan.keys.len(), 2);
    assert_eq!(scan.keys[0].algorithm, "ssh-ed25519");
    assert!(scan.keys[0].preferred);
    assert_eq!(scan.keys[0].sha256, "SHA256:EEEEEEEE");
    assert_eq!(scan.keys[1].algorithm, "ssh-rsa");
    assert_eq!(scan.keys[1].sha256, "SHA256:RRRRRRRR");
    assert_eq!(runner.call_count(), 3);
}

#[test]
fn host_store_records_last_connected_at() {
    let directory = tempfile::tempdir().unwrap();
    let store = JsonHostStore::new(directory.path().join("hosts.json"));
    let profile = store.add(new_host("connect-time")).unwrap();
    assert_eq!(profile.last_connected_at, None);
    let at = Timestamp::parse("2026-11-14T12:34:56.000Z").unwrap();
    let updated = store.mark_connected(&profile.id, at).unwrap();
    assert_eq!(updated.last_connected_at, Some(at));
    assert_eq!(
        store.get(&profile.id).unwrap().unwrap().last_connected_at,
        Some(at)
    );
}
