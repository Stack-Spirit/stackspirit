//! The bundled setup assistant is sandboxed and carries real StackSpirit instructions.

use std::os::unix::fs::PermissionsExt;

use serde_json::Value;
use spirit_core::engine::PrivateEngineTree;

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn setup_engine_config_denies_every_host_tool_and_never_asks() {
    let temp = tempfile::tempdir().unwrap();
    let tree = PrivateEngineTree::prepare(temp.path()).unwrap();
    let path = tree.opencode_config_dir().join("opencode.json");
    assert_eq!(mode(&path), 0o600);
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("\"ask\""));
    let config: Value = serde_json::from_str(&raw).unwrap();
    let permission = config["permission"].as_object().unwrap();
    for tool in [
        "bash",
        "edit",
        "write",
        "read",
        "external_directory",
        "webfetch",
    ] {
        assert_eq!(permission[tool], "deny", "{tool} must be denied");
    }
    assert!(permission.values().all(|value| value == "deny"));
    // Re-preparing (engine ensure) rewrites it atomically and keeps the mode.
    std::fs::write(&path, b"{}").unwrap();
    PrivateEngineTree::prepare(temp.path()).unwrap();
    assert_eq!(mode(&path), 0o600);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"bash\": \"deny\"")
    );
    assert!(!path.with_extension("tmp").exists());
}

#[test]
fn setup_agents_md_describes_real_flows_without_secrets_or_paths() {
    let temp = tempfile::tempdir().unwrap();
    let tree = PrivateEngineTree::prepare(temp.path()).unwrap();
    let path = tree.opencode_config_dir().join("AGENTS.md");
    assert_eq!(mode(&path), 0o600);
    let text = std::fs::read_to_string(&path).unwrap();
    for needle in [
        "+ Connect a new server",
        "Add server",
        "Form a cluster",
        "Never ask the user to paste a password",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
    let root = temp.path().to_string_lossy();
    assert!(!text.contains(root.as_ref()));
    for forbidden in ["/Users/", "/home/", "sk-", "BEGIN ", ".env"] {
        assert!(!text.contains(forbidden), "unexpected {forbidden}");
    }
}

#[test]
fn isolated_env_points_only_at_the_private_tree() {
    let temp = tempfile::tempdir().unwrap();
    let tree = PrivateEngineTree::prepare(temp.path()).unwrap();
    let env: std::collections::HashMap<_, _> = tree.isolated_env().into_iter().collect();
    let own = tree.opencode_config_dir();
    assert_eq!(env["OPENCODE_CONFIG"], own.join("opencode.json"));
    assert_eq!(env["OPENCODE_CONFIG_DIR"], own);
    assert_eq!(env["XDG_CONFIG_HOME"], tree.config);
    assert_eq!(env["OPENCODE_DISABLE_PROJECT_CONFIG"].to_str(), Some("1"));
    for (name, value) in &env {
        if *name != "OPENCODE_DISABLE_PROJECT_CONFIG" {
            assert!(value.starts_with(temp.path()), "{name} escapes the tree");
        }
    }
    assert!(PrivateEngineTree::INHERITED_CONFIG_ENV.contains(&"OPENCODE_CONFIG_CONTENT"));
}

/// Runs the bundled engine against a loopback mock provider and returns the request bodies it sent.
/// Returns `None` when the sidecar is not present (fresh worktree without `fetch-engine`).
fn captured_model_requests(temp: &std::path::Path) -> Option<String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let triple = spirit_core::engine::current_target_triple()?;
    let binary = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/spirit-desktop/src-tauri/binaries")
        .join(spirit_core::engine::sidecar_file_name(triple));
    if !binary.exists() {
        eprintln!("skipping: engine sidecar not fetched");
        return None;
    }
    let tree = PrivateEngineTree::prepare(temp).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Same sandbox config as production, plus a loopback provider so a turn can be captured.
    let path = tree.opencode_config_dir().join("opencode.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["provider"] = serde_json::json!({"openai": {"options": {
        "baseURL": format!("http://127.0.0.1:{port}/v1"), "apiKey": "mock"}}});
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();

    let captured = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&captured);
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };
            stream.set_nonblocking(false).ok();
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .ok();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stream.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            sink.lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf));
            let _ = stream.write_all(
                b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            );
            if sink.lock().unwrap().contains("StackSpirit") {
                break;
            }
        }
    });

    let mut command = Command::new(&binary);
    command
        .args(["run", "--model", "openai/gpt-4o", "hello"])
        .current_dir(&tree.setup_workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in PrivateEngineTree::INHERITED_CONFIG_ENV {
        command.env_remove(name);
    }
    for (name, value) in tree.isolated_env() {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline && !server.is_finished() {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = server.join();
    let text = captured.lock().unwrap().clone();
    Some(text)
}

#[test]
fn live_engine_sends_the_setup_prompt_to_the_model() {
    let temp = tempfile::tempdir().unwrap();
    let Some(requests) = captured_model_requests(temp.path()) else {
        return;
    };
    for needle in ["StackSpirit", "+ Connect a new server", "Form a cluster"] {
        assert!(
            requests.contains(needle),
            "the engine did not deliver the setup AGENTS.md ({needle} missing from the model request)"
        );
    }
}
