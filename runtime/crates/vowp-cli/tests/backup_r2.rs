//! Hermetic round-trip test for `stackspirit backup push|list|restore`.
//!
//! A loopback mock API stands in for `api.stackspirit.io` and the Worker's per-machine R2
//! backup routes. The child process gets a temporary `$HOME`, a temporary node home and
//! project, a fake `relay.json` enrollment and a generated age identity; it never reaches
//! the real network or the real home. The test verifies that only an age ciphertext reaches
//! the mock, that the plaintext session log, cloud token, relay record and `secrets/` never
//! appear in the upload, and that the restored home passes the core manifest and row-count
//! checks.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use age::secrecy::ExposeSecret;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use vowp_protocol::{JobId, JobKind, NewJob, Project, ProjectName, Role, SessionId, Timestamp, VowpConfig};
use vowp_store::{NodePaths, Store};

/// The fake access token; it is deliberately distinctive so the test can prove it is never
/// uploaded. It is not a real secret.
const TOKEN: &str = "ss-test-token-not-a-secret-8f4c2a";
/// A syntactically valid `mch_` id for the fake enrollment.
const MACHINE_ID: &str = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
/// A marker written into a session log; it must not be visible in the ciphertext.
const PLAINTEXT_MARKER: &str = "session-log-plaintext-marker";
/// A marker written under `secrets/`; the core backup excludes that directory.
const SECRET_MARKER: &str = "must-never-leave-the-machine";

#[test]
fn push_uploads_ciphertext_and_restore_rebuilds_the_home() {
    let server = MockServer::new();
    let fixture = Fixture::new(&server.url());
    let recipient = fixture.identity.to_public().to_string();

    // Push: one authenticated PUT to the machine's backup route.
    let pushed = fixture.run(&["--json", "backup", "push", "--recipient", &recipient]);
    assert!(
        pushed.status.success(),
        "push failed: {}",
        String::from_utf8_lossy(&pushed.stderr)
    );
    let pushed_json = parse_json(&pushed);
    let name = pushed_json["backup"]["name"]
        .as_str()
        .expect("push returns a backup name")
        .to_owned();
    assert!(name.ends_with(".tar.age"), "server object name: {name}");
    assert_eq!(pushed_json["backup"]["encrypted"], true);
    assert_eq!(pushed_json["machine"], MACHINE_ID);

    // The mock saw one object and only age ciphertext.
    {
        let state = server.state.lock().expect("mock state");
        let stored = state.backups.get(&name).expect("stored backup");
        assert_eq!(stored.machine, MACHINE_ID);
        assert!(
            stored.ciphertext.starts_with(b"age-encryption.org/v1\n"),
            "the uploaded object must be an age file"
        );
        assert!(
            !contains(&stored.ciphertext, PLAINTEXT_MARKER.as_bytes()),
            "the session log must never reach the server"
        );
        assert!(
            !contains(&stored.ciphertext, TOKEN.as_bytes()),
            "the cloud token must never reach the backup object"
        );
        assert!(
            !contains(&stored.ciphertext, SECRET_MARKER.as_bytes()),
            "secrets/ must never reach the backup object"
        );
        assert!(
            !contains(&stored.ciphertext, b"acct_test"),
            "the relay enrollment record must never reach the backup object"
        );
    }

    // List sees the object; `--machine alpha` resolves the name through the account list.
    let listed = fixture.run(&["--json", "backup", "list", "--machine", "alpha"]);
    assert!(
        listed.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let list_json = parse_json(&listed);
    assert_eq!(list_json["backups"][0]["name"], name);
    assert_eq!(list_json["machine"], MACHINE_ID);

    // Restore into a fresh home and verify the database, project, job and session log.
    let restored = fixture.root.path().join("restored-home");
    let restore = fixture.run(&[
        "--json",
        "backup",
        "restore",
        "--id",
        &name,
        "--identity",
        fixture.identity_path.to_str().expect("identity path"),
        "--to-home",
        restored.to_str().expect("restore path"),
    ]);
    assert!(
        restore.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&restore.stderr)
    );
    let restored_paths = NodePaths::new(&restored);
    let global = Store::open_global(&restored_paths).expect("restored global store");
    assert!(
        global
            .list_projects()
            .expect("projects")
            .iter()
            .any(|project| project.name == fixture.project_name),
        "the project row must be restored"
    );
    let project = Store::open_with_project(&restored_paths, &fixture.project_name).expect("restored project");
    let jobs = project.list_jobs(&Default::default()).expect("jobs");
    assert_eq!(jobs.len(), 1, "the job must be restored");
    assert_eq!(jobs[0].id, JobId::parse("r2-round-trip").expect("job id"));

    let log = std::fs::read_to_string(restored_paths.session_dir(&fixture.session_id).join("log.jsonl"))
        .expect("restored session log");
    assert!(log.contains(PLAINTEXT_MARKER), "the session content must round-trip");
    assert!(
        !restored.join("secrets").exists(),
        "secrets/ is deliberately absent from every backup"
    );

    // The restored home passes a normal store integrity check.
    global.integrity_check().expect("restored global integrity");
    project.integrity_check().expect("restored project integrity");
}

#[test]
fn restore_refuses_a_tampered_ciphertext() {
    let server = MockServer::new();
    let fixture = Fixture::new(&server.url());
    let recipient = fixture.identity.to_public().to_string();
    let pushed = fixture.run(&["--json", "backup", "push", "--recipient", &recipient]);
    assert!(
        pushed.status.success(),
        "push failed: {}",
        String::from_utf8_lossy(&pushed.stderr)
    );
    let name = parse_json(&pushed)["backup"]["name"]
        .as_str()
        .expect("backup name")
        .to_owned();

    {
        let mut state = server.state.lock().expect("mock state");
        let stored = state.backups.get_mut(&name).expect("stored backup");
        stored.ciphertext[30] ^= 0xff;
    }

    let target = fixture.root.path().join("tampered-restore");
    let restore = fixture.run(&[
        "--json",
        "backup",
        "restore",
        "--id",
        &name,
        "--identity",
        fixture.identity_path.to_str().expect("identity path"),
        "--to-home",
        target.to_str().expect("target"),
    ]);
    assert!(!restore.status.success(), "a tampered object must fail");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&restore.stdout),
        String::from_utf8_lossy(&restore.stderr)
    );
    assert!(
        text.contains("SHA-256") || text.contains("age"),
        "the failure must name integrity: {text}"
    );
    assert!(!target.exists(), "a failed restore must not create the target");
}

#[test]
fn local_backup_spelling_is_unchanged() {
    let server = MockServer::new();
    let fixture = Fixture::new(&server.url());
    let target = fixture.root.path().join("legacy.db");

    let output = fixture.run(&["backup", "--to", target.to_str().expect("target")]);
    assert!(
        output.status.success(),
        "local backup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target.is_file(), "the legacy single-DB backup must be written");
    let store = Store::open_path(&target).expect("legacy backup opens");
    store.integrity_check().expect("legacy backup integrity");
}

/// One seeded temporary machine.
struct Fixture {
    root: TempDir,
    home: PathBuf,
    project: PathBuf,
    project_name: ProjectName,
    session_id: SessionId,
    identity: age::x25519::Identity,
    identity_path: PathBuf,
}

impl Fixture {
    fn new(api_url: &str) -> Self {
        let root = tempfile::tempdir().expect("temp root");
        let home = root.path().join(".stackspirit");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("project dir");
        let config = VowpConfig::with_defaults_for_tests();
        std::fs::write(
            project.join("vowp.toml"),
            toml::to_string_pretty(&config).expect("config serialises"),
        )
        .expect("write vowp.toml");

        let paths = NodePaths::new(&home);
        let store = Store::open(&paths).expect("store opens");
        let now = Timestamp::now();
        let project_name = config.project.name.clone();
        store
            .upsert_project(&Project {
                name: project_name.clone(),
                path: project.display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::to_value(&config).expect("config json"),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        Store::create_project_db(&paths, &project_name).expect("project db");
        let project_store = Store::open_with_project(&paths, &project_name).expect("project store");
        project_store
            .insert_job(
                &project_name,
                &NewJob {
                    id: JobId::parse("r2-round-trip").expect("job id"),
                    title: "R2 round trip".to_owned(),
                    kind: JobKind::Build,
                    role: Role::Worker,
                    profile: None,
                    needs: Vec::new(),
                    owns: Vec::new(),
                    reads: Vec::new(),
                    gates: None,
                    priority: None,
                    loop_: None,
                    brief_text: None,
                },
                "test",
            )
            .expect("insert job");

        let session_id = SessionId::new();
        let session_dir = paths.session_dir(&session_id);
        std::fs::create_dir_all(&session_dir).expect("session dir");
        std::fs::write(
            session_dir.join("log.jsonl"),
            format!("{{\"kind\":\"note\",\"text\":\"{PLAINTEXT_MARKER}\"}}\n"),
        )
        .expect("session log");
        std::fs::create_dir_all(home.join("secrets")).expect("secrets dir");
        std::fs::write(home.join("secrets/marker.txt"), SECRET_MARKER).expect("secret marker");

        let session_path = home.join("cloud-session.json");
        std::fs::write(
            &session_path,
            serde_json::to_vec_pretty(&json!({
                "api_url": api_url,
                "access_token": TOKEN,
            }))
            .expect("session json"),
        )
        .expect("session file");
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&session_path, std::fs::Permissions::from_mode(0o600)).expect("session mode");

        // The real enrollment record holds more fields; the backup command only reads the
        // machine id and enforces the private mode.
        let relay_path = home.join("relay.json");
        std::fs::write(
            &relay_path,
            serde_json::to_vec_pretty(&json!({
                "machine_id": MACHINE_ID,
                "account_id": "acct_test",
            }))
            .expect("relay json"),
        )
        .expect("relay record");
        std::fs::set_permissions(&relay_path, std::fs::Permissions::from_mode(0o600)).expect("relay mode");

        let identity = age::x25519::Identity::generate();
        let identity_path = root.path().join("backup-identity.txt");
        std::fs::write(&identity_path, identity.to_string().expose_secret().as_bytes()).expect("identity file");
        std::fs::set_permissions(&identity_path, std::fs::Permissions::from_mode(0o600)).expect("identity mode");

        Self {
            root,
            home,
            project,
            project_name,
            session_id,
            identity,
            identity_path,
        }
    }

    /// Runs the shipped `stackspirit` binary with the isolated environment.
    fn run(&self, args: &[&str]) -> Output {
        let tmux = self.root.path().join("tmux");
        std::fs::create_dir_all(&tmux).expect("private tmux dir");
        let mut command = Command::new(env!("CARGO_BIN_EXE_stackspirit"));
        command
            .args([
                "--home",
                self.home.to_str().expect("home"),
                "--project",
                self.project.to_str().expect("project"),
            ])
            .args(args)
            .current_dir(&self.project)
            .env("HOME", self.root.path())
            .env("TMUX_TMPDIR", &tmux)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("STACKSPIRIT_HOME")
            .env_remove("VOWP_HOME")
            .env_remove("AGE_IDENTITY")
            .env_remove("STACKSPIRIT_BACKUP_RECIPIENT")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy");
        command.output().expect("run stackspirit")
    }
}

/// A minimal loopback API and R2 stand-in.
struct MockServer {
    address: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockServer {
    fn new() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let address = listener.local_addr().expect("address");
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            for connection in listener.incoming() {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = connection else {
                    continue;
                };
                let _ = handle_connection(&mut stream, &thread_state);
            }
        });
        Self {
            address,
            state,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// State shared with the accept loop.
#[derive(Default)]
struct State {
    next_id: u64,
    backups: BTreeMap<String, StoredBackup>,
}

struct StoredBackup {
    machine: String,
    name: String,
    created_at: String,
    size: u64,
    sha256: String,
    ciphertext: Vec<u8>,
}

impl StoredBackup {
    fn row(&self) -> Value {
        json!({
            "name": self.name,
            "size": self.size,
            "sha256": self.sha256,
            "created_at": self.created_at,
        })
    }
}

/// One parsed HTTP request.
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

/// Handles one connection.
fn handle_connection(stream: &mut TcpStream, state: &Arc<Mutex<State>>) -> std::io::Result<()> {
    let Some(request) = read_request(stream)? else {
        return Ok(());
    };
    let path = request.path.split('?').next().unwrap_or("").to_owned();

    let expected = format!("Bearer {TOKEN}");
    if request.headers.get("authorization") != Some(&expected) {
        return respond_json(
            stream,
            401,
            &json!({"error": {"code": "unauthorized", "message": "missing test token"}}),
        );
    }

    // GET /v1/machines (name resolution)
    if request.method == "GET" && path == "/v1/machines" {
        return respond_json(stream, 200, &json!({"machines": [{"id": MACHINE_ID, "name": "alpha"}]}));
    }

    // PUT /v1/machines/:id/backups
    if request.method == "PUT" && path == format!("/v1/machines/{MACHINE_ID}/backups") {
        let claimed = request
            .headers
            .get("x-content-sha256")
            .map(String::as_str)
            .unwrap_or_default();
        if claimed.len() != 64 || !claimed.chars().all(|character| character.is_ascii_hexdigit()) {
            return respond_json(
                stream,
                400,
                &json!({"error": {"code": "invalid_hash", "message": "bad x-content-sha256"}}),
            );
        }
        if request.headers.get("content-type").map(String::as_str) != Some("application/octet-stream") {
            return respond_json(
                stream,
                415,
                &json!({"error": {"code": "invalid_content_type", "message": "bad content type"}}),
            );
        }
        if !request.body.starts_with(b"age-encryption.org/v1\n") {
            return respond_json(
                stream,
                422,
                &json!({"error": {"code": "not_encrypted", "message": "not age"}}),
            );
        }
        let actual = hash_hex(&request.body);
        if !actual.eq_ignore_ascii_case(claimed) {
            return respond_json(
                stream,
                422,
                &json!({"error": {"code": "hash_mismatch", "message": "sha256 differs"}}),
            );
        }

        let mut state = state.lock().expect("mock state");
        state.next_id += 1;
        let name = format!("20261126T22000{}Z.tar.age", state.next_id);
        let stored = StoredBackup {
            machine: MACHINE_ID.to_owned(),
            name: name.clone(),
            created_at: "2026-11-26T22:00:00Z".to_owned(),
            size: u64::try_from(request.body.len()).unwrap_or_default(),
            sha256: actual,
            ciphertext: request.body.clone(),
        };
        let row = stored.row();
        state.backups.insert(name, stored);
        return respond_json(
            stream,
            201,
            &json!({
                "name": row["name"],
                "size": row["size"],
                "sha256": row["sha256"],
                "created_at": row["created_at"],
                "pruned": 0,
            }),
        );
    }

    // GET /v1/machines/:id/backups
    if request.method == "GET" && path == format!("/v1/machines/{MACHINE_ID}/backups") {
        let state = state.lock().expect("mock state");
        let mut rows: Vec<Value> = state.backups.values().map(StoredBackup::row).collect();
        rows.sort_by(|left, right| {
            right["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(left["name"].as_str().unwrap_or_default())
        });
        return respond_json(stream, 200, &json!({"backups": rows}));
    }

    // GET /v1/machines/:id/backups/:name
    if request.method == "GET" {
        if let Some(name) = path
            .strip_prefix(&format!("/v1/machines/{MACHINE_ID}/backups/"))
            .filter(|name| !name.is_empty())
        {
            let state = state.lock().expect("mock state");
            let Some(backup) = state.backups.get(name) else {
                return respond_json(
                    stream,
                    404,
                    &json!({"error": {"code": "not_found", "message": "unknown backup"}}),
                );
            };
            return respond(stream, 200, "application/octet-stream", &backup.ciphertext);
        }
    }

    respond_json(
        stream,
        404,
        &json!({"error": {"code": "not_found", "message": "unknown route"}}),
    )
}

/// Reads one HTTP/1.1 request with a Content-Length body.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    if headers
        .get("expect")
        .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        stream.flush()?;
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

/// Writes one raw response.
fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

/// Writes one JSON response.
fn respond_json(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    respond(stream, status, "application/json", &bytes)
}

/// Parses one JSON stdout document.
fn parse_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {:?}; stderr: {:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Returns the lowercase hexadecimal SHA-256 of a byte slice.
fn hash_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Returns whether `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
