//! Finder file operations: path validation, argv-only quoting, classification, listing
//! parsing and the masked-by-default preview. Hermetic: a `FakeCommandRunner` stands in
//! for SSH, and the secret-like fixtures are assembled at runtime.

use std::sync::Arc;

use spirit_core::error::SpiritErrorCode;
use spirit_core::ssh::command::CommandOutput;
use spirit_core::ssh::{FakeCommandRunner, SshOptions, SshRemote, SshTarget};
use spirit_core::vowp::files::{
    base64_encode, build_preview, is_protected_name, is_protected_path, is_secret_like,
    parse_listing, validate_entry_name, validate_remote_path,
};
use spirit_core::vowp::{RemoteEntryKind, RemoteFiles, RemotePreviewKind};

fn files(responses: Vec<CommandOutput>) -> (Arc<FakeCommandRunner>, RemoteFiles) {
    let runner = Arc::new(FakeCommandRunner::new(responses));
    let target = SshTarget::new("owner", "vps.example.test", 22).expect("target");
    let options = SshOptions::new("/tmp/known_hosts");
    let remote = SshRemote::new(runner.clone(), target, options);
    (runner, RemoteFiles::new(remote))
}

fn listing_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"/home/owner\0/home/owner/.vowp\0");
    for record in [
        "f\t4096\t600\t1763000000.5\tvowp.db",
        "f\t120\t644\t1763000100.0\tvowp.toml",
        "d\t4096\t755\t1763000200.0\tlogs",
        "d\t4096\t755\t1763000300.0\tworktrees",
        "l\t11\t777\t1763000400.0\tcurrent",
        "f\t30\t600\t1763000500.0\t.env",
        "f\t7\t644\t1763000600.0\tname with\ttab and 'quote",
    ] {
        out.extend_from_slice(record.as_bytes());
        out.push(0);
    }
    out
}

#[test]
fn validates_remote_paths() {
    assert!(validate_remote_path("p", "/").is_ok());
    assert!(validate_remote_path("p", "~").is_ok());
    assert!(validate_remote_path("p", "~/.vowp/logs").is_ok());
    assert!(validate_remote_path("p", "").is_err());
    assert!(validate_remote_path("p", "relative/path").is_err());
    assert!(validate_remote_path("p", "/a\0b").is_err());
    assert!(validate_remote_path("p", "/a\nb").is_err());
    assert!(validate_remote_path("p", "/a\rb").is_err());
    assert!(validate_remote_path("p", &format!("/{}", "x".repeat(5000))).is_err());
    assert!(validate_entry_name("n", "ok name.txt").is_ok());
    for bad in ["", ".", "..", "a/b", "a\u{7}b"] {
        assert!(validate_entry_name("n", bad).is_err(), "{bad:?}");
    }
}

#[test]
fn classifies_secret_like_names() {
    let key = ["id_", "ed25519"].concat();
    for name in [
        ".env",
        ".env.local",
        key.as_str(),
        "server.pem",
        "tls.KEY",
        "auth.json",
        "credentials",
        "credentials.json",
    ] {
        assert!(is_secret_like(name), "{name}");
    }
    for name in [
        "README.md",
        "id_ed25519.pub",
        "environment.md",
        "keyboard.txt",
        "vowp.toml",
    ] {
        assert!(!is_secret_like(name), "{name}");
    }
}

#[test]
fn classifies_protected_live_paths() {
    for name in [
        "vowp.db",
        "vowp.db-wal",
        "vowp.db-shm",
        "vowp.toml",
        "stackspirit.toml",
        ".ssh",
    ] {
        assert!(is_protected_name(name), "{name}");
    }
    assert!(!is_protected_name("vowp.toml.bak"));
    assert!(!is_protected_name("stackspirit.toml.bak"));
    assert!(is_protected_path("~/.vowp/stackspirit.toml"));
    assert!(is_protected_path("~/.ssh"));
    assert!(is_protected_path("/home/o/.ssh/authorized_keys"));
    assert!(is_protected_path("~/.vowp/vowp.db"));
    assert!(is_protected_path("/home/o/.vowp/worktrees/job-1"));
    assert!(is_protected_path("~/.vowp/worktrees/job-1/src/main.rs"));
    // The worktrees folder itself and ordinary paths are not protected.
    assert!(!is_protected_path("~/.vowp/worktrees"));
    assert!(!is_protected_path("~/.vowp/logs"));
    assert!(!is_protected_path("/home/o/stackspirit"));
}

#[test]
fn parses_a_listing_with_hints_and_awkward_names() {
    let listing = parse_listing(&listing_bytes()).expect("parse");
    assert_eq!(listing.home, "/home/owner");
    assert_eq!(listing.path, "/home/owner/.vowp");
    assert_eq!(listing.parent.as_deref(), Some("/home/owner"));
    assert!(!listing.truncated);
    // Directories first, then files by name.
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names[0], "logs");
    assert_eq!(names[1], "worktrees");
    let db = listing
        .entries
        .iter()
        .find(|e| e.name == "vowp.db")
        .unwrap();
    assert_eq!(db.kind, RemoteEntryKind::File);
    assert_eq!(db.size_bytes, 4096);
    assert_eq!(db.mode, "0600");
    assert_eq!(db.path, "/home/owner/.vowp/vowp.db");
    assert!(db.is_protected_hint && !db.is_secret_like);
    assert!(db.modified_at.is_some());
    let env = listing.entries.iter().find(|e| e.name == ".env").unwrap();
    assert!(env.is_secret_like && !env.is_protected_hint);
    let link = listing
        .entries
        .iter()
        .find(|e| e.name == "current")
        .unwrap();
    assert_eq!(link.kind, RemoteEntryKind::Symlink);
    assert!(
        listing
            .entries
            .iter()
            .any(|e| e.name == "name with\ttab and 'quote")
    );
    let worktrees = listing
        .entries
        .iter()
        .find(|e| e.name == "worktrees")
        .unwrap();
    assert!(!worktrees.is_protected_hint);
}

#[test]
fn worktree_children_are_protected_hints() {
    let mut bytes = b"/home/o\0/home/o/.vowp/worktrees\0".to_vec();
    bytes.extend_from_slice(b"d\t4096\t755\t1763000000.0\tjob-1\0");
    let listing = parse_listing(&bytes).expect("parse");
    assert!(listing.entries[0].is_protected_hint);
}

#[test]
fn rejects_a_listing_without_a_header() {
    assert!(parse_listing(b"").is_err());
    assert!(parse_listing(b"/home/o\0not-absolute\0").is_err());
}

#[tokio::test]
async fn list_dir_sends_the_path_as_one_quoted_argv_element() {
    let (runner, files) = files(vec![CommandOutput::ok(listing_bytes())]);
    let hostile = "/tmp/a'; rm -rf / #";
    let listing = files.list_dir(hostile).await.expect("list");
    assert_eq!(listing.entries.len(), 7);
    let call = runner.last_call().expect("call");
    let remote = call.args.last().expect("remote command").clone();
    // The hostile path is wrapped in quotes with the embedded quote escaped, and only
    // appears after the fixed `sh -c <script> sh` prefix.
    assert!(remote.starts_with("'sh' '-c' '"), "{remote}");
    assert!(remote.ends_with("'/tmp/a'\\''; rm -rf / #'"), "{remote}");
}

#[tokio::test]
async fn rejects_unsafe_paths_before_any_ssh_call() {
    let (runner, files) = files(vec![]);
    for bad in ["/a\0b", "/a\nb", "no-slash", ""] {
        let err = files.list_dir(bad).await.expect_err(bad);
        assert_eq!(err.code, SpiritErrorCode::InvalidInput);
    }
    assert!(files.mkdir("~/new/..").await.is_err());
    assert!(files.rename_or_move("/a", "/b/\n", false).await.is_err());
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn secret_like_preview_is_masked_and_never_reads_the_file() {
    let (runner, files) = files(vec![CommandOutput::ok(b"30\0".to_vec())]);
    let preview = files
        .read_preview("/home/o/project/.env", false)
        .await
        .expect("preview");
    assert!(preview.masked);
    assert!(preview.text.is_none() && preview.image_base64.is_none());
    // The read cap passed to the script is 0: no bytes of the file are requested.
    let remote = runner.last_call().unwrap().args.last().unwrap().clone();
    assert!(remote.ends_with("'/home/o/project/.env' '0'"), "{remote}");
}

#[tokio::test]
async fn reveal_reads_a_secret_like_file_only_when_explicit() {
    let body = ["TOKEN=", "not-a-real-value"].concat();
    let mut out = b"22\0".to_vec();
    out.extend_from_slice(body.as_bytes());
    let (runner, files) = files(vec![CommandOutput::ok(out)]);
    let preview = files.read_preview("~/p/.env", true).await.expect("preview");
    assert!(!preview.masked);
    assert_eq!(preview.text.as_deref(), Some(body.as_str()));
    let remote = runner.last_call().unwrap().args.last().unwrap().clone();
    assert!(remote.ends_with("'~/p/.env' '262144'"), "{remote}");
    // Debug output never prints content.
    assert!(!format!("{preview:?}").contains("not-a-real-value"));
}

#[test]
fn builds_text_markdown_image_and_binary_previews() {
    let text = build_preview("/a/notes.txt", b"5\0hello", 1024, false).unwrap();
    assert_eq!(text.kind, RemotePreviewKind::Text);
    assert_eq!(text.text.as_deref(), Some("hello"));
    let md = build_preview("/a/README.md", b"3\0# a", 1024, false).unwrap();
    assert_eq!(md.kind, RemotePreviewKind::Markdown);
    let png = build_preview("/a/pic.PNG", b"3\0abc", 1024, false).unwrap();
    assert_eq!(png.kind, RemotePreviewKind::Image);
    assert_eq!(png.image_mime.as_deref(), Some("image/png"));
    assert_eq!(png.image_base64.as_deref(), Some("YWJj"));
    let bin = build_preview("/a/blob.bin", b"3\0a\0b", 1024, false).unwrap();
    assert_eq!(bin.kind, RemotePreviewKind::Binary);
    assert!(bin.text.is_none() && bin.note.is_some());
    let big = build_preview("/a/big.log", b"9999999\0", 1024, false).unwrap();
    assert!(big.truncated && big.text.is_none());
    let masked = build_preview("/a/.env", b"4\0", 1024, true).unwrap();
    assert!(masked.masked && masked.text.is_none());
}

#[test]
fn base64_matches_rfc_vectors() {
    for (input, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64_encode(input.as_bytes()), expected);
    }
}

#[tokio::test]
async fn protected_paths_need_confirmation_before_any_ssh_call() {
    let (runner, files) = files(vec![
        CommandOutput::ok(Vec::new()),
        CommandOutput::ok(Vec::new()),
    ]);
    let err = files
        .delete("~/.vowp/vowp.db", false, false)
        .await
        .expect_err("seatbelt");
    assert_eq!(err.code, SpiritErrorCode::Conflict);
    assert!(err.message.starts_with("protected_path"));
    let err = files
        .rename_or_move("~/.vowp/vowp.toml", "~/.vowp/old.toml", false)
        .await
        .expect_err("seatbelt");
    assert_eq!(err.code, SpiritErrorCode::Conflict);
    assert_eq!(runner.call_count(), 0);
    files
        .delete("~/.vowp/vowp.db", false, true)
        .await
        .expect("confirmed");
    assert_eq!(runner.call_count(), 1);
}

#[tokio::test]
async fn root_and_home_cannot_be_deleted() {
    let (runner, files) = files(vec![]);
    for path in ["/", "~", "~/"] {
        let err = files.delete(path, true, true).await.expect_err(path);
        assert_eq!(err.code, SpiritErrorCode::Refused);
    }
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn remote_exit_codes_map_to_typed_errors_without_echoing_paths() {
    let (_runner, files) = files(vec![
        CommandOutput::fail(5, b"mv: /secret/path exists".to_vec()),
        CommandOutput::fail(3, Vec::new()),
    ]);
    let err = files.mkdir("~/exists").await.expect_err("exists");
    assert_eq!(err.code, SpiritErrorCode::Conflict);
    assert!(!err.message.contains("secret"));
    let err = files
        .rename_or_move("~/gone", "~/there", false)
        .await
        .expect_err("missing");
    assert_eq!(err.code, SpiritErrorCode::NotFound);
}

#[tokio::test]
async fn upload_streams_stdin_and_never_overwrites() {
    let dir = tempfile::tempdir().expect("dir");
    let local = dir.path().join("notes.txt");
    std::fs::write(&local, b"hello").expect("write");
    let (runner, files) = files(vec![CommandOutput::ok(Vec::new())]);
    let outcome = files.upload_file(&local, "~/inbox").await.expect("upload");
    assert_eq!(outcome.path, "~/inbox/notes.txt");
    assert_eq!(outcome.size_bytes, 5);
    let call = runner.last_call().unwrap();
    assert_eq!(call.stdin.as_deref(), Some(b"hello".as_slice()));
    assert!(call.args.last().unwrap().ends_with("'~/inbox' 'notes.txt'"));
}

#[tokio::test]
async fn download_writes_a_new_local_file_and_keeps_existing_ones() {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join("a.txt"), b"mine").expect("write");
    let (_runner, files) = files(vec![CommandOutput::ok(b"4\0data".to_vec())]);
    let outcome = files
        .download_file("/srv/a.txt", dir.path())
        .await
        .expect("download");
    assert_eq!(outcome.size_bytes, 4);
    assert!(
        outcome.local_path.ends_with("a (1).txt"),
        "{}",
        outcome.local_path
    );
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"mine");
    assert_eq!(std::fs::read(&outcome.local_path).unwrap(), b"data");
}
