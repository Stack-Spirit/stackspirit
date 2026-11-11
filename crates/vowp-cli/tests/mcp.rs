// `vowp mcp` integration tests.
//
// Every test uses a temporary node home and project, a scripted stdio client and never
// touches the real `~/.vowp`, the default tmux server, the network or a real harness.
//
// The crate root lists every test target explicitly and F6 does not own `Cargo.toml`, so
// `src/mcp/tests.rs` includes this same file as a lib unit-test module. The fixture below
// is deliberately self-contained so both compilation modes see the same tests.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;
use vowp_cli::BufferIo;
use vowp_protocol::{
    Job, JobId, JobKind, Lease, NewJob, Node, NodeName, Project, Role, SessionId, Surface, SurfaceId, SurfaceKind,
    Timestamp, VowpConfig,
};
use vowp_store::{LedgerFilter, NodePaths, Store};

/// One temporary node home and project.
struct TestEnv {
    home: TempDir,
    project: TempDir,
    config: VowpConfig,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("temp home");
        let project = tempfile::tempdir().expect("temp project");
        let config = VowpConfig::with_defaults_for_tests();
        let text = toml::to_string_pretty(&config).expect("config serialises");
        std::fs::write(project.path().join("vowp.toml"), text).expect("write vowp.toml");
        std::fs::create_dir_all(project.path().join(".vowp/jobs")).expect("jobs dir");

        let paths = NodePaths::new(home.path());
        let store = Store::open(&paths).expect("store opens");
        let now = store.clock().now();
        store
            .upsert_node(&Node {
                name: NodeName::parse("local").expect("local slug"),
                ssh_target: None,
                is_local: true,
                capabilities: json!({}),
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
        Self { home, project, config }
    }

    fn home_path(&self) -> &Path {
        self.home.path()
    }

    fn project_path(&self) -> &Path {
        self.project.path()
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut full = vec![
            "vowp".to_owned(),
            "--home".to_owned(),
            self.home.path().display().to_string(),
            "--project".to_owned(),
            self.project.path().display().to_string(),
        ];
        full.extend(args.iter().map(|value| (*value).to_owned()));
        let mut io = BufferIo::new("");
        let code = vowp_cli::run(full, &mut io);
        (code, io.stdout(), io.stderr())
    }

    fn store(&self) -> Store {
        Store::open(&NodePaths::new(self.home.path())).expect("store opens")
    }

    fn insert_job(&self, id: &str, title: &str, kind: JobKind, needs: &[&str]) -> Job {
        let store = self.store();
        let new = NewJob {
            id: JobId::parse(id).expect("job id"),
            title: title.to_owned(),
            kind,
            role: Role::Worker,
            profile: None,
            needs: needs.iter().map(|need| JobId::parse(need).expect("need id")).collect(),
            owns: Vec::new(),
            reads: Vec::new(),
            gates: None,
            priority: None,
            loop_: None,
            brief_text: None,
        };
        store
            .insert_job(&self.config.project.name, &new, "test")
            .expect("insert job")
    }
}

/// Runs the MCP server in-process with scripted stdin and returns `(code, stdout, stderr)`.
fn run_mcp(
    env: &TestEnv,
    session: &str,
    surface: &str,
    token_file: &Path,
    input: &str,
) -> (i32, String, String) {
    let args = vec![
        "vowp".to_owned(),
        "--home".to_owned(),
        env.home_path().display().to_string(),
        "--project".to_owned(),
        env.project_path().display().to_string(),
        "mcp".to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "--surface".to_owned(),
        surface.to_owned(),
        "--token-file".to_owned(),
        token_file.display().to_string(),
    ];
    let mut io = BufferIo::new(input);
    let code = vowp_cli::run(args, &mut io);
    (code, io.stdout(), io.stderr())
}

/// One JSON-RPC request line.
fn request(id: u64, method: &str, params: Value) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .expect("request JSON")
}

/// One `tools/call` request line.
fn tool_call(id: u64, name: &str, arguments: Value) -> String {
    request(id, "tools/call", json!({"name": name, "arguments": arguments}))
}

/// Parses the server's newline-delimited output.
fn responses(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("bad JSON line {line:?}: {error}")))
        .collect()
}

/// Returns the response with `id`.
fn response(values: &[Value], id: u64) -> &Value {
    values
        .iter()
        .find(|value| value.get("id").and_then(Value::as_u64) == Some(id))
        .unwrap_or_else(|| panic!("no response id {id} in {values:?}"))
}

/// Returns the parsed tool result text.
fn tool_text(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no tool text in {response}"));
    serde_json::from_str(text).unwrap_or_else(|error| panic!("tool text is not JSON ({error}): {text:?}"))
}

/// Creates a session and returns its id.
fn create_session(env: &TestEnv) -> String {
    let (code, stdout, stderr) = env.run(&[
        "session",
        "new",
        "--title",
        "mcp test",
        "--executor",
        "opencode:deepseek/deepseek-flash#max",
    ]);
    assert_eq!(code, 0, "session new failed: {stderr}");
    let id = stdout.trim().to_owned();
    SessionId::parse(&id).expect("session id");
    id
}

/// Takes a lease with the given token and holder.
fn take_lease(env: &TestEnv, session: &str, token: &str, holder: &str) {
    let id = SessionId::parse(session).expect("session id");
    let store = env.store();
    let now = store.clock().now();
    let surface_id = SurfaceId("surf-test".to_owned());
    store
        .upsert_surface(&Surface {
            id: surface_id.clone(),
            kind: SurfaceKind::Mcp,
            node: NodeName::parse("local").expect("node"),
            holder: holder.to_owned(),
            app: Some("vowp".to_owned()),
            cwd: None,
            metadata: json!({}),
            created_at: now,
            last_seen_at: now,
        })
        .expect("surface row");
    store
        .take_lease(&Lease {
            session_id: id,
            surface_id: Some(surface_id),
            surface_kind: SurfaceKind::Mcp,
            node: NodeName::parse("local").expect("node"),
            holder: holder.to_owned(),
            token: token.to_owned(),
            acquired_at: now,
            heartbeat_at: now,
        })
        .expect("take lease");
}

/// Returns a lease token with the schema's required 52 characters.
fn lease_token(label: &str) -> String {
    let mut token = label.to_owned();
    while token.len() < 52 {
        token.push('x');
    }
    token.truncate(52);
    token
}

/// Writes the lease token file.
fn token_file(env: &TestEnv, token: &str) -> PathBuf {
    let path = env.home_path().join("lease.token");
    std::fs::write(&path, format!("{token}\n")).expect("token file");
    path
}

#[test]
fn mcp_initialize_and_tools_list() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let token = token_file(&env, "unused");
    let input = format!(
        "{}\n{}\n",
        request(
            1,
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {}})
        ),
        request(2, "tools/list", json!({})),
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let init = response(&values, 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "vowp");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    let tools = response(&values, 2)["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    for name in [
        "fleet_status",
        "fleet_queue",
        "inbox_list",
        "capacity_report",
        "session_tail",
        "inbox_answer",
        "job_add",
        "job_pause",
        "job_retry",
        "capacity_set",
        "tunnel_open",
        "tunnel_close",
        "secret_request",
        "session_handoff",
        "session_resume",
    ] {
        assert!(tools.contains(&name), "missing tool {name}: {tools:?}");
    }
}

#[test]
fn mcp_server_discover_answered() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let token = token_file(&env, "unused");
    let input = format!("{}\n", request(1, "server/discover", json!({})));
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let discover = response(&values, 1);
    assert!(discover.get("error").is_none(), "{discover}");
    assert_eq!(discover["result"]["serverInfo"]["name"], "vowp");
}

#[test]
fn mcp_read_tools_return_snapshot_data() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let token = token_file(&env, "unused");
    env.insert_job("job-a", "first mcp job", JobKind::Build, &[]);

    let input = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        tool_call(1, "fleet_status", json!({})),
        tool_call(2, "fleet_queue", json!({})),
        tool_call(3, "inbox_list", json!({})),
        tool_call(4, "capacity_report", json!({})),
        tool_call(5, "session_tail", json!({"session": session})),
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);

    let status = tool_text(response(&values, 1));
    assert_eq!(status["source"], "local");
    assert_eq!(
        status["snapshot"]["projects"][0]["name"],
        env.config.project.name.as_str()
    );

    let queue = tool_text(response(&values, 2));
    assert!(
        queue
            .as_array()
            .expect("queue array")
            .iter()
            .any(|row| row["job"] == "job-a"),
        "{queue}"
    );

    let inbox = tool_text(response(&values, 3));
    assert_eq!(inbox.as_array().map(Vec::len), Some(0), "{inbox}");

    let capacity = tool_text(response(&values, 4));
    assert!(capacity["target"]["target_workers"].is_number(), "{capacity}");
    assert!(capacity["brakes"].is_object(), "{capacity}");

    let tail = tool_text(response(&values, 5));
    assert!(
        tail.as_array().is_some_and(|events| !events.is_empty()),
        "session tail must contain session.created: {tail}"
    );
}

#[test]
fn mcp_write_tool_checks_lease_and_records_actor() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let right = lease_token("right");
    take_lease(&env, &session, &right, "laptop");
    let token = token_file(&env, &right);
    let input = format!(
        "{}\n",
        tool_call(
            1,
            "capacity_set",
            json!({"target": 2, "mode": "fixed", "override": true, "reason": "test target"}),
        )
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let result = response(&values, 1);
    assert_eq!(result["result"]["isError"], false, "{result}");
    let target = tool_text(result);
    assert_eq!(target["target_workers"], 2, "{target}");
    assert_eq!(
        target["set_by"],
        format!("session:{session}/surface:surf-test"),
        "{target}"
    );

    let store = env.store();
    let rows = store
        .ledger_since(&LedgerFilter::default())
        .expect("ledger rows");
    let row = rows
        .iter()
        .find(|entry| entry.summary == "mcp set capacity target 2")
        .unwrap_or_else(|| panic!("no MCP ledger row in {rows:?}"));
    assert_eq!(
        row.detail["actor"],
        format!("session:{session}/surface:surf-test"),
        "{}",
        row.detail
    );
    assert_eq!(row.detail["tool"], "capacity_set");
}

#[test]
fn mcp_old_app_write_gets_read_only_notice() {
    let env = TestEnv::new();
    let session = create_session(&env);
    take_lease(&env, &session, &lease_token("phone"), "phone");
    let token = token_file(&env, &lease_token("laptop-stale"));
    let input = format!(
        "{}\n",
        tool_call(
            1,
            "capacity_set",
            json!({"target": 2, "mode": "fixed", "override": true}),
        )
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "old-surface", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let result = response(&values, 1);
    assert_eq!(result["result"]["isError"], true, "{result}");
    let text = result["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("Taken over by phone"), "{text}");
    assert!(text.contains("read-only"), "{text}");
    assert!(text.contains("follow mode"), "{text}");
    assert!(
        env.store().latest_capacity_target().expect("target query").is_none(),
        "a refused write must not change the target"
    );
}

#[test]
fn mcp_job_tools_add_pause_and_retry() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let right = lease_token("right");
    take_lease(&env, &session, &right, "laptop");
    let token = token_file(&env, &right);
    let input = format!(
        "{}\n{}\n{}\n",
        tool_call(
            1,
            "job_add",
            json!({
                "id": "job-mcp",
                "title": "job added over MCP",
                "kind": "build",
                "role": "worker",
                "owns": ["src/**"],
                "brief_text": "# job-mcp\n\nDo the work.\n"
            }),
        ),
        tool_call(2, "job_pause", json!({"job": "job-mcp", "reason": "owner test"})),
        tool_call(3, "job_retry", json!({"job": "job-mcp"})),
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);

    let added = tool_text(response(&values, 1));
    assert_eq!(added["id"], "job-mcp");
    assert_eq!(added["state"], "pending");
    assert!(
        env.project_path()
            .join(".vowp/jobs/job-mcp/brief.md")
            .is_file(),
        "job_add must write the brief"
    );

    let paused = tool_text(response(&values, 2));
    assert_eq!(paused["state"], "parked", "{paused}");
    let retried = tool_text(response(&values, 3));
    assert_eq!(retried["state"], "ready", "{retried}");
}

#[test]
fn mcp_session_resume_returns_context_and_keeps_token_private() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let right = lease_token("right");
    take_lease(&env, &session, &right, "laptop");
    let token = token_file(&env, &right);
    let input = format!("{}\n", tool_call(1, "session_resume", json!({})));
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let result = response(&values, 1);
    assert_eq!(result["result"]["isError"], false, "{result}");
    let context = tool_text(result);
    assert_eq!(context["session"]["id"], session);
    assert_eq!(context["lease"]["holder"], "laptop");
    assert!(
        context["tail"]
            .as_array()
            .is_some_and(|events| !events.is_empty()),
        "{context}"
    );
    let text = result["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(!text.contains(&right), "resume must not return the lease token: {text}");
}

#[test]
fn mcp_secret_request_never_returns_value() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let right = lease_token("right");
    take_lease(&env, &session, &right, "laptop");
    let token = token_file(&env, &right);
    let input = format!(
        "{}\n",
        tool_call(1, "secret_request", json!({"name": "DEEPSEEK_API_KEY"}))
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let result = response(&values, 1);
    assert_eq!(result["result"]["isError"], false, "{result}");
    let request = tool_text(result);
    assert_eq!(request["name"], "DEEPSEEK_API_KEY");
    assert!(
        request["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("http://127.0.0.1:")),
        "{request}"
    );
    assert_eq!(request["single_use"], true);
    assert!(request.get("value").is_none(), "{request}");
    let text = result["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(!text.contains("sk-"), "{text}");
}

#[test]
fn mcp_unknown_method_is_jsonrpc_error() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let token = token_file(&env, "unused");
    let input = format!("{}\n", request(9, "does/not/exist", json!({})));
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    let response = response(&values, 9);
    assert_eq!(response["error"]["code"], -32601, "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("does/not/exist")),
        "{response}"
    );
}

#[test]
fn mcp_works_over_scripted_stdio_client() {
    let env = TestEnv::new();
    let session = create_session(&env);
    let right = lease_token("right");
    let token = token_file(&env, &right);
    take_lease(&env, &session, &right, "laptop");
    let input = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n",
        request(
            1,
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {}})
        ),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        request(2, "tools/list", json!({})),
        request(3, "ping", json!({})),
        tool_call(4, "fleet_status", json!({})),
        request(5, "missing/method", json!({})),
    );
    let (code, stdout, stderr) = run_mcp(&env, &session, "surf-test", &token, &input);
    assert_eq!(code, 0, "{stderr}");
    let values = responses(&stdout);
    assert_eq!(
        values
            .iter()
            .map(|value| value["id"].as_u64().unwrap_or(0))
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5],
        "one response per request, none for the notification: {values:?}"
    );
    assert_eq!(response(&values, 3)["result"], json!({}));
    assert_eq!(response(&values, 5)["error"]["code"], -32601);
    let status = tool_text(response(&values, 4));
    assert_eq!(status["snapshot"]["home"], "local");
    assert!(
        status["fetched_at"]
            .as_str()
            .is_some_and(|text| Timestamp::parse(text).is_ok()),
        "{status}"
    );
}
