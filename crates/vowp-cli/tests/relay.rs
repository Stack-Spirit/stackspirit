//! `stackspirit relay enroll|status|run|rotate-key` against a fake loopback API.
//! Spec: `docs/design/relay-and-packaging.md` §6.2. Hermetic: temporary home, loopback only.
//!
//! This file is compiled as a `vowp-cli` library test module through `src/cmd/relay.rs`: the
//! package root is the workspace root, so Cargo does not auto-discover `crates/vowp-cli/tests/`.
//! It still works as an integration target if the root manifest ever declares one.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::{Value, json};
use tempfile::TempDir;
use vowp_protocol::{Node, NodeName, Project, VowpConfig};
use vowp_store::{NodePaths, Store};

/// A temporary node home and project, with no inherited tmux or enrollment-token environment.
struct TestEnv {
    home: TempDir,
    project: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let project = tempfile::tempdir().expect("temp project");
        let config = VowpConfig::with_defaults_for_tests();
        let text = toml::to_string_pretty(&config).expect("config serialises");
        std::fs::write(project.path().join("vowp.toml"), text).expect("write vowp.toml");
        std::fs::create_dir_all(project.path().join(".vowp/jobs")).expect("jobs dir");
        std::fs::create_dir_all(home.path().join("tmux")).expect("private tmux dir");
        let paths = NodePaths::new(home.path());
        let store = Store::open(&paths).expect("store opens");
        let now = store.clock().now();
        store
            .upsert_node(&Node {
                name: NodeName::parse("local").expect("local slug"),
                ssh_target: None,
                is_local: true,
                capabilities: serde_json::json!({}),
                last_seen_at: Some(now),
                created_at: now,
            })
            .expect("node row");
        store
            .upsert_project(&Project {
                name: config.project.name.clone(),
                path: project.path().display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::to_value(&config).expect("config json"),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        Self { home, project }
    }

    fn home_path(&self) -> &Path {
        self.home.path()
    }

    /// Builds a child command with the isolated home and tmux environment.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", self.home.path())
            .env("TMUX_TMPDIR", self.home.path().join("tmux"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("VOWP_TMUX_SOCKET")
            .env_remove("STACKSPIRIT_ENROLL_TOKEN");
        command
    }

    fn global_args(&self) -> Vec<String> {
        vec![
            "--home".to_owned(),
            self.home.path().display().to_string(),
            "--project".to_owned(),
            self.project.path().display().to_string(),
        ]
    }

    /// Runs one command in a child process and returns `(code, stdout, stderr)`.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        self.command(binary("vowp"))
            .args(self.global_args())
            .args(args)
            .output()
            .map(|output| {
                (
                    output.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                )
            })
            .expect("run vowp")
    }

    /// Runs one command with `stdin`, for `--token-stdin`.
    fn run_with_stdin(&self, args: &[&str], input: &str) -> (i32, String, String) {
        let mut child = self
            .command(binary("vowp"))
            .args(self.global_args())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn vowp");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait vowp");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

/// Returns the `vowp` or `stackspirit` binary Cargo built for this package.
///
/// Integration targets get `CARGO_BIN_EXE_<name>`. This file is also a library test module,
/// where Cargo does not set it, so the target directory is checked next and the binaries are
/// built once when neither exists.
fn binary(name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(format!("CARGO_BIN_EXE_{name}")).map(PathBuf::from)
        && path.is_file()
    {
        return path;
    }
    let target = target_dir();
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut candidates: Vec<PathBuf> = ["debug", "release"]
        .iter()
        .map(|profile| target.join(profile).join(&file))
        .collect();
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent().and_then(Path::parent)
    {
        candidates.push(dir.join(&file));
    }
    if let Some(found) = candidates.iter().find(|path| path.is_file()) {
        return found.clone();
    }
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        // `cargo test --lib` does not rebuild the binaries the tests run. Build them once so
        // the tests always exercise the code compiled from this checkout.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let _ = Command::new(cargo)
            .args(["build", "--bin", "vowp", "--bin", "stackspirit"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status();
    });
    if let Some(found) = candidates.iter().find(|path| path.is_file()) {
        return found.clone();
    }
    panic!("{name} binary not found; run `cargo build --bin vowp --bin stackspirit` first");
}

/// The target directory Cargo used for this checkout.
fn target_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() { dir } else { manifest.join(dir) }
        }
        None => manifest.join("target"),
    }
}

const TOKEN: &str = "ssm_cli-test-only-token-0123456789";
const MACHINE_ID: &str = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";

fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// An unsigned, JWT-shaped credential: the machine only reads `exp` from it locally.
fn fake_credential(tag: &str) -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        + 86_400;
    let claims = json!({
        "iss": "http://127.0.0.1", "aud": "127.0.0.1", "sub": format!("machine:{MACHINE_ID}"), "acct": "acct_x",
        "key_id": "ed25519:x", "mkey": "x", "scopes": ["machine.connect"], "iat": exp - 86_400, "exp": exp, "jti": tag,
    });
    format!(
        "{}.{}.{}",
        b64url(br#"{"alg":"ES256","kid":"k"}"#),
        b64url(claims.to_string().as_bytes()),
        b64url(tag.as_bytes())
    )
}

struct FakeApi {
    url: String,
    pubkey: Arc<Mutex<String>>,
    log: Arc<Mutex<Vec<String>>>,
}

fn fake_api() -> FakeApi {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let url = format!("http://127.0.0.1:{port}");
    let pubkey = Arc::new(Mutex::new(String::new()));
    let log = Arc::new(Mutex::new(Vec::new()));
    let (state_key, state_log, own_url) = (Arc::clone(&pubkey), Arc::clone(&log), url.clone());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (state_key, state_log, own_url) = (Arc::clone(&state_key), Arc::clone(&state_log), own_url.clone());
            std::thread::spawn(move || handle(&mut stream, &state_key, &state_log, &own_url));
        }
    });
    FakeApi { url, pubkey, log }
}

fn handle(stream: &mut TcpStream, pubkey: &Mutex<String>, log: &Mutex<Vec<String>>, own_url: &str) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split_whitespace();
    let (method, path) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    let length: usize = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    log.lock().expect("lock").push(format!("{method} {path}"));
    let (status, reply) = match (method, path) {
        ("POST", "/v1/machines/enroll") if body["token"] == TOKEN => {
            *pubkey.lock().expect("lock") = body["public_key"].as_str().unwrap_or_default().to_owned();
            (
                200,
                json!({
                    "machine_id": MACHINE_ID, "account_id": "acct_01JQ9B7M5X6P8S1T3W4Y5Z6A7B",
                    "machine_credential": fake_credential("enroll"),
                    "relay_url": "ws://127.0.0.1:1/v1/machine", "api_url": own_url,
                    "expires_at": "2099-01-01T00:00:00.000Z",
                }),
            )
        }
        ("POST", "/v1/machines/enroll") => (401, json!({"error": {"code": "invalid_token"}})),
        ("POST", p) if p == format!("/v1/machines/{MACHINE_ID}/credential") => (
            200,
            json!({"machine_credential": fake_credential("refresh"), "expires_at": "2099-01-01T00:00:00.000Z"}),
        ),
        ("POST", p) if p == format!("/v1/machines/{MACHINE_ID}/keys") => {
            let old = pubkey.lock().expect("lock").clone();
            let new = body["new_public_key"].as_str().unwrap_or_default().to_owned();
            let canonical = [
                "STACKSPIRIT-ROTATE-V1",
                MACHINE_ID,
                body["key_id"].as_str().unwrap_or_default(),
                body["new_key_id"].as_str().unwrap_or_default(),
                &new,
                body["signed_at"].as_str().unwrap_or_default(),
            ]
            .join("\n");
            let verified = |public: &str, signature: &str| -> bool {
                let Some(key) = unhex(public).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else {
                    return false;
                };
                let Some(sig) = unhex(signature).ok().and_then(|b| <[u8; 64]>::try_from(b).ok()) else {
                    return false;
                };
                VerifyingKey::from_bytes(&key)
                    .is_ok_and(|key| key.verify(canonical.as_bytes(), &Signature::from_bytes(&sig)).is_ok())
            };
            if verified(&old, body["signature"].as_str().unwrap_or_default())
                && verified(&new, body["new_signature"].as_str().unwrap_or_default())
            {
                *pubkey.lock().expect("lock") = new;
                (
                    200,
                    json!({"machine_credential": fake_credential("rotated"), "expires_at": "2099-01-01T00:00:00.000Z"}),
                )
            } else {
                (401, json!({"error": {"code": "bad_signature"}}))
            }
        }
        _ => (404, json!({})),
    };
    let text = reply.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",
        text.len()
    );
}

fn unhex(text: &str) -> Result<Vec<u8>, ()> {
    if text.len() % 2 != 0 {
        return Err(());
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn write_token(env: &TestEnv) -> std::path::PathBuf {
    let path = env.home_path().join("token.txt");
    std::fs::write(&path, format!("{TOKEN}\n")).expect("token file");
    path
}

fn all_text_under(dir: &Path) -> String {
    let mut text = String::new();
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if path.file_name().is_some_and(|name| name == "token.txt") {
                    continue;
                }
                text.push_str(&content);
            }
        }
    }
    text
}

fn enroll_via_cli(env: &TestEnv, api: &FakeApi) -> (String, String) {
    let token_file = write_token(env);
    let (code, stdout, stderr) = env.run(&[
        "relay",
        "enroll",
        "--token-file",
        &token_file.display().to_string(),
        "--api-url",
        &api.url,
        "--name",
        "cli-box",
        "--serve-project",
        "vowp",
    ]);
    assert_eq!(code, 0, "enroll failed: {stdout}\n{stderr}");
    (stdout, stderr)
}

#[test]
fn relay_enroll_status_and_rotate_through_the_cli() {
    let env = TestEnv::new();
    let api = fake_api();
    // A user setting that enrollment must preserve.
    std::fs::write(
        env.home_path().join("config.toml"),
        "[ingress.wss]\nmax_connections = 7\n",
    )
    .expect("config");

    let (code, stdout, _) = env.run(&["--json", "relay", "status"]);
    assert_eq!(code, 0);
    let before: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(before["enrolled"], false);
    assert_eq!(before["enabled"], false);

    let (stdout, stderr) = enroll_via_cli(&env, &api);
    assert!(stdout.contains(MACHINE_ID) && stdout.contains("cli-box"), "{stdout}");
    assert!(
        !stdout.contains(TOKEN) && !stderr.contains(TOKEN),
        "the token is never printed"
    );

    let home = env.home_path();
    assert_eq!(mode(&home.join("relay.key")), 0o600);
    assert_eq!(mode(&home.join("relay.pub")), 0o644);
    assert_eq!(mode(&home.join("relay.json")), 0o600);
    assert!(
        !home.join("node.key").exists(),
        "the relay key is separate from node.key"
    );
    let config = std::fs::read_to_string(home.join("config.toml")).expect("config");
    assert!(
        config.contains("max_connections = 7"),
        "other settings are preserved:\n{config}"
    );
    assert!(
        config.contains("[relay]") && config.contains("enabled = true"),
        "{config}"
    );
    assert!(config.contains(&format!("api_url = \"{}\"", api.url)));
    assert!(config.contains("project = \"vowp\""));
    assert!(
        !config.contains("credential") && !config.contains(TOKEN),
        "no secret in config.toml"
    );
    assert!(!all_text_under(home).contains(TOKEN), "the token is written nowhere");

    // status --json: no credential, token or private key anywhere.
    let private = std::fs::read_to_string(home.join("relay.key"))
        .expect("key")
        .trim()
        .to_owned();
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("relay.json")).expect("json")).expect("record");
    let credential = record["credential"].as_str().expect("credential").to_owned();
    let (code, stdout, _) = env.run(&["--json", "relay", "status"]);
    assert_eq!(code, 0);
    for secret in [credential.as_str(), private.as_str(), TOKEN] {
        assert!(!stdout.contains(secret), "status --json leaked a secret: {stdout}");
    }
    let status: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(status["enrolled"], true);
    assert_eq!(status["enabled"], true);
    assert_eq!(status["machine_id"], MACHINE_ID);
    assert_eq!(status["name"], "cli-box");
    assert_eq!(status["policy"]["http_read"], true);
    assert_eq!(status["policy"]["http_write"], false);
    assert_eq!(status["policy"]["ssh"], false);
    assert!(status.get("credential").is_none() && status.get("token").is_none());
    let key_id_before = status["key_id"].as_str().expect("key id").to_owned();
    assert!(key_id_before.starts_with("ed25519:"));

    let (code, text, _) = env.run(&["relay", "status"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("enrolled:  true") && text.contains("connector: not started"),
        "{text}"
    );
    assert!(!text.contains(&credential) && !text.contains(&private));

    // A second enrollment is a conflict and does not call the API again.
    let calls = api.log.lock().expect("lock").len();
    let token_file = write_token(&env);
    let (code, _, stderr) = env.run(&[
        "relay",
        "enroll",
        "--token-file",
        &token_file.display().to_string(),
        "--api-url",
        &api.url,
    ]);
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("already enrolled"), "{stderr}");
    assert_eq!(api.log.lock().expect("lock").len(), calls);

    // rotate-key: a new key, 0600, the API saw valid signatures from both keys.
    let (code, stdout, stderr) = env.run(&["--json", "relay", "rotate-key"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let rotated: Value = serde_json::from_str(&stdout).expect("json");
    assert_ne!(rotated["key_id"], key_id_before);
    assert_eq!(mode(&home.join("relay.key")), 0o600);
    assert_eq!(mode(&home.join("relay.json")), 0o600);
    let new_private = std::fs::read_to_string(home.join("relay.key"))
        .expect("key")
        .trim()
        .to_owned();
    assert_ne!(new_private, private);
    assert!(!stdout.contains(&new_private));
    let (_, stdout, _) = env.run(&["--json", "relay", "status"]);
    let after: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(after["key_id"], rotated["key_id"]);
    assert_eq!(
        *api.pubkey.lock().expect("lock"),
        after["key_id"].as_str().expect("id").trim_start_matches("ed25519:")
    );
}

#[test]
fn relay_enroll_refuses_a_token_flag_and_wants_a_token_source() {
    let env = TestEnv::new();
    let api = fake_api();
    let (code, _, stderr) = env.run(&["relay", "enroll", "--token", TOKEN, "--api-url", &api.url]);
    assert_eq!(code, 2, "a token flag must not exist: {stderr}");
    assert!(
        !stderr.contains(TOKEN) || stderr.contains("unexpected argument"),
        "{stderr}"
    );

    // No token anywhere: a clear error and no network call.
    let output = env
        .command(binary("vowp"))
        .args([
            "--home",
            &env.home_path().display().to_string(),
            "relay",
            "enroll",
            "--api-url",
            &api.url,
        ])
        .env_remove("STACKSPIRIT_ENROLL_TOKEN")
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--token-file") && stderr.contains("STACKSPIRIT_ENROLL_TOKEN"),
        "{stderr}"
    );
    assert!(api.log.lock().expect("lock").is_empty());
    assert!(
        !env.home_path().join("relay.key").exists(),
        "nothing is written without a token"
    );

    // The environment variable works and the token still goes nowhere.
    let output = env
        .command(binary("stackspirit"))
        .args([
            "--home",
            &env.home_path().display().to_string(),
            "relay",
            "enroll",
            "--api-url",
            &api.url,
        ])
        .env("STACKSPIRIT_ENROLL_TOKEN", TOKEN)
        .output()
        .expect("run");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
    assert_eq!(mode(&env.home_path().join("relay.key")), 0o600);
}

#[test]
fn relay_enroll_reads_the_token_from_stdin_and_reports_a_rejected_token() {
    let env = TestEnv::new();
    let api = fake_api();
    // A rejected token: exit 7 (remote), the token is not echoed, no relay.json.
    let (code, stdout, stderr) = env.run_with_stdin(
        &["relay", "enroll", "--token-stdin", "--api-url", &api.url],
        "ssm_wrong-token-0123456789abcdef\n",
    );
    assert_eq!(code, 7, "{stderr}");
    assert!(!stderr.contains("wrong-token") && !stdout.contains("wrong-token"));
    assert!(stderr.contains("invalid_token"), "{stderr}");
    assert!(!env.home_path().join("relay.json").exists());

    let (code, stdout, stderr) = env.run_with_stdin(
        &["--json", "relay", "enroll", "--token-stdin", "--api-url", &api.url],
        &format!("{TOKEN}\n"),
    );
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(report["machine_id"], MACHINE_ID);
    assert!(report.get("credential").is_none() && report.get("token").is_none());
    assert!(!stdout.contains(TOKEN));
}

#[test]
fn relay_run_refuses_when_disabled_or_not_enrolled() {
    let env = TestEnv::new();
    let (code, _, stderr) = env.run(&["relay", "run"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("relay is disabled"), "{stderr}");

    std::fs::write(env.home_path().join("config.toml"), "[relay]\nenabled = true\n").expect("config");
    let (code, _, stderr) = env.run(&["relay", "run"]);
    assert_eq!(code, 3, "{stderr}");
    assert!(
        stderr.contains("not enrolled") && stderr.contains("relay enroll"),
        "{stderr}"
    );
    assert!(
        !env.home_path().join("web-window").exists(),
        "run never starts a daemon"
    );
}

#[test]
fn relay_run_reconnects_in_the_background_and_stops_on_sigterm() {
    let env = TestEnv::new();
    let api = fake_api();
    enroll_via_cli(&env, &api); // the fake API points relay.json at a closed loopback port
    let child = env
        .command(binary("stackspirit"))
        .args(["--home", &env.home_path().display().to_string(), "relay", "run"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn relay run");
    let status_path = env.home_path().join("relay-status.json");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen_backoff = false;
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&status_path) {
            if let Ok(status) = serde_json::from_str::<Value>(&text) {
                if status["state"] == "backoff" {
                    seen_backoff = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        seen_backoff,
        "the connector never reached backoff against a closed port"
    );
    let before = std::fs::read_to_string(&status_path).expect("status");
    let record = std::fs::read_to_string(env.home_path().join("relay.json")).expect("record");
    let credential = serde_json::from_str::<Value>(&record).expect("json")["credential"]
        .as_str()
        .expect("c")
        .to_owned();
    assert!(
        !before.contains(&credential),
        "the status file never holds the credential"
    );
    assert_eq!(mode(&status_path), 0o600);

    // `relay status` sees the live connector.
    let (code, stdout, _) = env.run(&["--json", "relay", "status"]);
    assert_eq!(code, 0);
    let status: Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(status["connector"]["state"], "backoff");
    assert_eq!(status["connector"]["pid"], child.id());
    assert_eq!(status["connector_stale"], false);

    let killed = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill");
    assert!(killed.success());
    let output = child.wait_with_output().expect("wait");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("relay: dropped (network)"), "{stderr}");
    for secret in [credential.as_str(), TOKEN] {
        assert!(!stderr.contains(secret) && !String::from_utf8_lossy(&output.stdout).contains(secret));
    }
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&status_path).expect("status")).expect("json");
    assert_eq!(after["state"], "stopped", "a clean stop is recorded");
    assert!(
        !env.home_path().join("web-window").exists(),
        "relay run never starts a second daemon"
    );
}
