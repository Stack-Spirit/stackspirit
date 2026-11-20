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
    let path = tree.setup_workspace.join("AGENTS.md");
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
