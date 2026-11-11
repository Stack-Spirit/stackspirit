// `[project].path` defaults and relative harness mappings.
//
// `vowp.toml` may omit `[project].path`; the documented default is the directory
// containing the file. These tests cover the config default and the real CLI resolving a
// relative harness `mapping` from that default when the caller stands elsewhere.
//
// The package root is the workspace root, so Cargo does not auto-discover
// `crates/vowp-cli/tests/`. Like the mcp and setup suites, this file is compiled as a
// library unit-test module through `src/ctx.rs`; the shared root `Cargo.toml` is outside
// this job's owned paths. The wrapper aliases the crate as `vowp_cli`, so the same file
// also works unchanged if the package later gains a `[[test]] name = "project_path"`
// entry. Run it with `cargo test -p vowp-cli --lib project_path`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

use tempfile::TempDir;
use vowp_cli::ctx::load_project_config;
use vowp_protocol::{HarnessConfig, Node, NodeName, Project, VowpConfig};
use vowp_store::{NodePaths, Store};

/// Writes a valid config whose `[project]` table omits `path`.
fn write_config_without_path(dir: &Path) -> PathBuf {
    write_config(dir, "")
}

/// Writes a valid config whose `[project]` table carries `extra` after the name.
fn write_config(dir: &Path, extra: &str) -> PathBuf {
    let text = format!(
        r#"[node]
name = "local"

[project]
name = "test"
{extra}
[profiles.planner]
ladder = ["opencode:test/model"]

[profiles.keeper]
ladder = ["opencode:test/model"]

[profiles.worker]
ladder = ["opencode:test/model"]

[profiles.verifier]
ladder = ["opencode:test/model"]
"#
    );
    let path = dir.join("vowp.toml");
    std::fs::write(&path, text).expect("write vowp.toml");
    path
}

#[test]
fn load_project_config_defaults_project_path() {
    let home = tempfile::tempdir().expect("temp home");
    let dir = tempfile::tempdir().expect("temp config dir");
    let paths = NodePaths::new(home.path());

    let file = write_config_without_path(dir.path());
    let config = load_project_config(&file, &paths).expect("load config without path");
    assert_eq!(
        config.project.path,
        Some(dir.path().display().to_string()),
        "an omitted [project].path must default to the directory containing vowp.toml"
    );

    let file = write_config(dir.path(), "path = \"/somewhere\"\n");
    let config = load_project_config(&file, &paths).expect("load config with explicit path");
    assert_eq!(
        config.project.path.as_deref(),
        Some("/somewhere"),
        "an explicit [project].path must be kept verbatim"
    );
}

#[test]
fn cli_project_name_resolves_relative_mapping_outside_checkout() {
    let env = TestEnv::new();
    let mut config = env.config.clone();
    config.project.path = None;
    config.harnesses.insert(
        "fakecli".to_owned(),
        HarnessConfig {
            mapping: Some("fakecli.toml".to_owned()),
            ..HarnessConfig::default()
        },
    );
    std::fs::write(
        env.project_path().join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config serialises"),
    )
    .expect("write vowp.toml");
    std::fs::write(
        env.project_path().join("fakecli.toml"),
        "name = \"fakecli\"\n[command]\nargv = [\"fake\"]\n",
    )
    .expect("write mapping");

    // Stand somewhere unrelated so a cwd-relative mapping cannot be found by accident.
    let elsewhere = tempfile::tempdir().expect("unrelated dir");
    let output = env
        .command()
        .current_dir(elsewhere.path())
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg("test")
        .args(["--json", "job", "ls"])
        .output()
        .expect("run vowp");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "vowp --project test must resolve fakecli.toml under the project directory; exit {:?}; stdout: {stdout}; stderr: {stderr}",
        output.status.code()
    );
    assert!(
        !stderr.contains("No such file or directory"),
        "relative mapping must not be resolved from the caller's cwd; stderr: {stderr}"
    );
    assert!(
        !stdout.contains("No such file or directory"),
        "relative mapping must not be resolved from the caller's cwd; stdout: {stdout}"
    );
    let jobs: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {stdout:?}; stderr: {stderr:?}"));
    assert!(jobs.is_array(), "job ls must print a JSON array: {jobs}");
}

/// One temporary node home and project, copied from `tests/common/mod.rs`.
///
/// The shared helper cannot be included as a library module: it imports the library by
/// its external crate name. Only the pieces this file needs are reproduced here.
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
        std::fs::write(
            project.path().join("vowp.toml"),
            toml::to_string_pretty(&config).expect("config serialises"),
        )
        .expect("write vowp.toml");
        std::fs::create_dir_all(project.path().join(".vowp/jobs")).expect("jobs dir");
        // tmux must never fall back to the user's default server.
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
        Self { home, project, config }
    }

    fn home_path(&self) -> &Path {
        self.home.path()
    }

    fn project_path(&self) -> &Path {
        self.project.path()
    }

    /// Builds a child command with the isolated home and private tmux environment.
    fn command(&self) -> Command {
        let mut command = Command::new(vowp_binary());
        command
            .env("HOME", self.home.path())
            .env("TMUX_TMPDIR", self.home.path().join("tmux"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("VOWP_TMUX_SOCKET");
        command
    }
}

/// Returns the `vowp` binary built from this checkout.
///
/// Integration tests get `CARGO_BIN_EXE_vowp`; this file is also included as a library
/// unit-test module, where Cargo does not set it, so derive and build the sibling binary.
fn vowp_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_vowp") {
        return PathBuf::from(path);
    }
    let candidate = std::env::current_exe()
        .ok()
        .and_then(|current| current.parent().and_then(Path::parent).map(|debug| debug.join("vowp")))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/vowp"));
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        // `cargo test --lib` does not rebuild the binary the test runs. Build it once so
        // the tests always exercise the code compiled from this checkout.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let _ = Command::new(cargo)
            .args(["build", "--bin", "vowp"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status();
    });
    candidate
}
