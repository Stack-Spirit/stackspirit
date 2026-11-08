//! Shared hermetic fixtures for the `vowp-cli` integration tests.
//!
//! Every test gets its own temporary node home and project directory. The helper never reads
//! the real home directory, starts no harness, and never touches the network.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use vowp_cli::{BufferIo, Global};
use vowp_protocol::{Job, JobId, JobKind, NewJob, Node, NodeName, Project, Role, VowpConfig};
use vowp_store::{NodePaths, Store};

/// One temporary node home and project.
pub struct TestEnv {
    /// Node home (`--home`).
    pub home: TempDir,
    /// Project checkout containing `vowp.toml`.
    pub project: TempDir,
    /// The config written into the project.
    pub config: VowpConfig,
}

impl TestEnv {
    /// Creates the temp home, project config and registered node/project rows.
    pub fn new() -> Self {
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

    /// Returns the node home path.
    pub fn home_path(&self) -> &Path {
        self.home.path()
    }

    /// Returns the project path.
    pub fn project_path(&self) -> &Path {
        self.project.path()
    }

    /// Builds the global flags this environment needs.
    pub fn global(&self, json: bool) -> Global {
        Global {
            json,
            home: Some(self.home.path().to_path_buf()),
            project: Some(self.project.path().display().to_string()),
            ..Global::default()
        }
    }

    /// Runs one command in-process and returns `(code, stdout, stderr)`.
    pub fn run(&self, args: &[&str]) -> (i32, String, String) {
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

    /// Runs one command with `--json` and parses its stdout document.
    pub fn run_json(&self, args: &[&str]) -> (i32, serde_json::Value, String) {
        let mut full = vec!["--json"];
        full.extend(args.iter().copied());
        let (code, stdout, stderr) = self.run(&full);
        let value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {stdout:?}; stderr: {stderr:?}"));
        (code, value, stderr)
    }

    /// Opens a fresh store connection to the temporary home.
    pub fn store(&self) -> Store {
        Store::open(&NodePaths::new(self.home.path())).expect("store opens")
    }

    /// Inserts a pending job directly.
    pub fn insert_job(&self, id: &str, title: &str, kind: JobKind, needs: &[&str]) -> Job {
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

    /// Returns the path of one job's brief.
    pub fn brief_path(&self, id: &str) -> PathBuf {
        self.project.path().join(".vowp/jobs").join(id).join("brief.md")
    }
}
