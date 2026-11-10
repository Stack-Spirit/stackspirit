//! `vowp reindex`, `backup`, `schema`, `version`, and the CLI-wide listing and stub tests.

mod common;

use vowp_cli::cli::V0_1_COMMANDS;

#[test]
fn backup_creates_openable_db() {
    let env = common::TestEnv::new();
    let path = env.home_path().join("manual-backup.db");
    let (code, stdout, stderr) = env.run(&["backup", "--to", path.to_str().expect("path")]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains(path.to_str().expect("path")), "{stdout}");
    assert!(path.is_file());
    let store = vowp_store::Store::open_path(&path).expect("backup opens");
    assert_eq!(store.schema_version().expect("schema"), 1);
    assert_eq!(store.list_projects().expect("projects").len(), 1);
    store.integrity_check().expect("integrity");
}

#[test]
fn reindex_after_db_delete_via_cli() {
    let env = common::TestEnv::new();
    let (code, stdout, stderr) = env.run(&["session", "new", "--title", "reindex me"]);
    assert_eq!(code, 0, "{stderr}");
    let id = stdout.trim().to_owned();
    assert!(!id.is_empty());

    // Remove the index database and call the CLI directly. `vowp reindex` must open/create
    // the empty index itself and rebuild it from `sessions/*/log.jsonl`.
    std::fs::remove_file(env.home_path().join("vowp.db")).expect("delete db");
    let (code, value, stderr) = env.run_json(&["reindex"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["sessions"], 1);
    assert!(value["events"].as_u64().unwrap_or(0) >= 1);

    let store = env.store();
    let session = store
        .require_session(&vowp_protocol::SessionId::parse(&id).expect("id"))
        .expect("session restored");
    assert_eq!(session.title, "reindex me");
    assert!(session.last_seq >= 1);
}

#[test]
fn version_prints_binary_git_and_schema() {
    let env = common::TestEnv::new();
    let (code, stdout, stderr) = env.run(&["version"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.starts_with("vowp "), "{stdout}");
    assert!(stdout.contains("schema 1"), "{stdout}");

    let (code, value, stderr) = env.run_json(&["version"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["schema"], 1);
    assert!(value["version"].as_str().unwrap_or_default().starts_with("0.1"));
}

#[test]
fn schema_lists_names_and_prints_one_document() {
    let env = common::TestEnv::new();
    let (code, value, stderr) = env.run_json(&["schema"]);
    assert_eq!(code, 0, "{stderr}");
    let names = value.as_array().expect("names");
    assert!(names.iter().any(|name| name == "Job"), "{value}");
    assert!(names.iter().any(|name| name == "Session"), "{value}");

    let (code, value, stderr) = env.run_json(&["schema", "Job"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(value["properties"].is_object(), "{value}");

    let (code, _, _) = env.run(&["schema", "NoSuchType"]);
    assert_eq!(code, 3);
}

#[test]
fn json_flag_outputs_valid_json_for_every_listing_command() {
    let env = common::TestEnv::new();
    for args in [
        vec!["job", "ls"],
        vec!["session", "ls"],
        vec!["inbox", "ls"],
        vec!["node", "ls"],
        vec!["loops"],
        vec!["schedule", "ls"],
        vec!["releases"],
        vec!["schema"],
    ] {
        let (code, value, stderr) = env.run_json(&args);
        assert_eq!(code, 0, "`{}` failed: {stderr}\nstdout: {value}", args.join(" "));
        assert!(
            value.is_array() || value.is_object(),
            "`{}` printed {value}",
            args.join(" ")
        );
    }

    let (code, value, stderr) = env.run_json(&["cost"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(value["by"], "job");
    assert!(value["rows"].is_array());
    assert!(value["total"].is_object());
}

#[test]
fn no_command_returns_not_implemented() {
    let mut env = common::TestEnv::new();
    // Point every harness probe at a fake script so `doctor` and `plan` never run the real
    // claude/opencode binaries installed on this machine.
    let fake = env.home_path().join("fake-harness.sh");
    std::fs::write(
        &fake,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '2.0.28'; exit 0; fi\nexit 0\n",
    )
    .expect("fake harness");
    let mut permissions = std::fs::metadata(&fake).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt as _;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake, permissions).expect("chmod");
    let mut config = env.config.clone();
    for name in ["opencode", "claude-code"] {
        config.harnesses.insert(
            name.to_owned(),
            vowp_protocol::HarnessConfig {
                binary: Some(fake.display().to_string()),
                ..vowp_protocol::HarnessConfig::default()
            },
        );
    }
    std::fs::write(
        env.project_path().join("vowp.toml"),
        toml::to_string_pretty(&config).expect("config"),
    )
    .expect("write config");
    env.config = config;

    let backup = env.home_path().join("cli-smoke-backup.db");
    let backup_text = backup.display().to_string();
    // `init` stays local and walks the cwd for a git repository; run it in a throwaway
    // repository so the smoke test can never write into this checkout.
    let init_dir = tempfile::tempdir().expect("temp init dir");
    std::fs::create_dir(init_dir.path().join(".git")).expect("git dir");
    let output = env
        .command(env!("CARGO_BIN_EXE_vowp"))
        .current_dir(init_dir.path())
        .arg("--home")
        .arg(env.home_path())
        .args(["init", "--name", "smoke"])
        .output()
        .expect("run vowp init");
    let init_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!init_text.contains("not implemented"), "{init_text}");

    let commands: Vec<Vec<&str>> = vec![
        vec!["doctor"],
        vec!["up", "--once", "--no-tmux"],
        vec!["down"],
        vec!["status", "--check"],
        vec!["board", "--once"],
        vec!["plan", "smoke", "--dry-run"],
        vec!["job", "ls"],
        vec!["graph"],
        vec!["attach"],
        vec!["session", "ls"],
        vec!["chat", "00000000000000000000000000"],
        vec!["inbox", "ls"],
        vec!["cost"],
        vec!["node", "ls"],
        vec!["loops"],
        vec!["schedule", "ls"],
        vec!["promote", "HEAD", "--to", "missing-env"],
        vec!["rollback", "00000000000000000000000000"],
        vec!["releases"],
        vec!["reindex"],
        vec!["backup", "--to", &backup_text],
        vec!["schema"],
        vec!["version"],
        vec!["__window", "board"],
    ];
    assert_eq!(
        commands.len() + 1,
        V0_1_COMMANDS.len(),
        "every v0.1 command needs a dry invocation"
    );
    for args in commands {
        let (code, stdout, stderr) = env.run(&args);
        let text = format!("{stdout}\n{stderr}");
        if text.contains("not implemented") {
            // A command may still be an explicit stub only while its own handler source
            // contains the stub. Once it is implemented, a NotImplemented return fails.
            let source = source_for(args[0]);
            assert!(
                source.contains("Err(CliError::NotImplemented("),
                "`{}` returned NotImplemented (exit {code}) but its handler is implemented: {text}",
                args.join(" ")
            );
        }
    }
}

/// Returns the source file for one top-level command.
fn source_for(command: &str) -> String {
    let module = match command {
        "chat" | "session" => "session",
        "up" | "down" => "up",
        "job" => "job",
        "graph" => "graph",
        "inbox" => "inbox",
        "cost" => "cost",
        "node" => "node",
        "loops" => "loops",
        "schedule" => "schedule",
        "promote" | "rollback" | "releases" => "promote",
        "reindex" | "backup" | "schema" | "version" => "maint",
        "__window" => "window",
        other => other,
    };
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/cmd")
            .join(format!("{module}.rs")),
    )
    .unwrap_or_default()
}

#[test]
fn node_add_and_rm_probe_with_fake_ssh() {
    let env = common::TestEnv::new();
    let bin = tempfile::tempdir().expect("fake bin");
    let ssh = bin.path().join("ssh");
    std::fs::write(&ssh, "#!/bin/sh\nprintf '%s\\n' 'vowp 0.1.0-dev'\nexit 0\n").expect("fake ssh");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let path = format!("{}:/usr/bin:/bin", bin.path().display());

    let output = env
        .command(env!("CARGO_BIN_EXE_vowp"))
        .env("PATH", path)
        .arg("--home")
        .arg(env.home_path())
        .arg("--project")
        .arg(env.project_path())
        .args(["node", "add", "vps", "vps.example", "--strict"])
        .output()
        .expect("node add");
    assert!(
        output.status.success(),
        "node add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (code, value, stderr) = env.run_json(&["node", "ls"]);
    assert_eq!(code, 0, "{stderr}");
    let node = value
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["name"] == "vps")
        .expect("vps row");
    assert_eq!(node["ssh_target"], "vps.example");
    assert_eq!(node["is_local"], false);
    assert_eq!(node["capabilities"]["vowp"], "vowp 0.1.0-dev");

    let (code, _, stderr) = env.run(&["node", "rm", "vps"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        env.store()
            .node(&vowp_protocol::NodeName::parse("vps").expect("node"))
            .expect("lookup")
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// H2: backup and restore
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vowp_core::lifecycle::backup::{BackupOptions, backup as core_backup, restore as core_restore};
use vowp_core::{
    AdapterRegistry, CommandOutput, CommandRunner, CommandSpec, CoreError, Deps, FixedRng, SystemGit, SystemRunner,
};
use vowp_protocol::{
    Clock, ErrorKind, FakeClock, JobId, JobKind, MemoryLog, NewJob, NodeName, Project, ProjectName, Role, SessionId,
    Timestamp,
};
use vowp_store::{JobFilter, NodePaths, SessionFilter, Store};
use vowp_tmux::fake::FakeWindows;

/// One hermetic node with a fake clock and a recording command runner.
struct BackupFixture {
    env: common::TestEnv,
    clock: FakeClock,
    runner: RecordingRunner,
}

impl BackupFixture {
    fn new() -> Self {
        Self {
            env: common::TestEnv::new(),
            clock: FakeClock::at(timestamp("2026-01-01T02:00:00Z")),
            runner: RecordingRunner::default(),
        }
    }

    fn deps(&self) -> Deps {
        Deps {
            paths: NodePaths::new(self.env.home_path()),
            config: Arc::new(self.env.config.clone()),
            node: NodeName::parse("local").expect("node"),
            clock: Arc::new(self.clock.clone()),
            log: Arc::new(MemoryLog::new()),
            adapters: Arc::new(AdapterRegistry::new()),
            windows: Arc::new(FakeWindows::new()),
            git: Arc::new(SystemGit::new(Arc::new(SystemRunner))),
            cmd: Arc::new(self.runner.clone()),
            rng: Arc::new(FixedRng(0)),
        }
    }

    fn store(&self) -> Store {
        Store::open_global(&NodePaths::new(self.env.home_path())).expect("global store")
    }

    fn add_project(&self, name: &str) -> ProjectName {
        let name = ProjectName::parse(name).expect("project name");
        let now = self.clock.now();
        self.env
            .store()
            .upsert_project(&Project {
                name: name.clone(),
                path: self.env.project_path().join(name.as_str()).display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::json!({}),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        name
    }

    fn insert_job(&self, project: &ProjectName, title: &str) -> JobId {
        let store = Store::open_with_project(&NodePaths::new(self.env.home_path()), project).expect("project store");
        let id = JobId::parse(&title.to_lowercase().replace(' ', "-")).expect("job id");
        let new = NewJob {
            id: id.clone(),
            title: title.to_owned(),
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
        };
        store.insert_job(project, &new, "test").expect("insert job");
        id
    }

    fn run_backup(&self, store: &Store, opts: BackupOptions) -> Result<vowp_protocol::BackupReport, CoreError> {
        core_backup(&self.deps(), store, &opts)
    }

    fn run_restore(
        &self,
        store: &Store,
        source: &Path,
        to_home: &Path,
        force: bool,
    ) -> Result<vowp_protocol::BackupReport, CoreError> {
        core_restore(
            &self.deps(),
            store,
            &BackupOptions {
                restore: Some(source.to_path_buf()),
                to_home: Some(to_home.to_path_buf()),
                force,
                ..BackupOptions::default()
            },
        )
    }
}

/// A command runner that records argv and simulates `age` file output.
#[derive(Clone, Default)]
struct RecordingRunner {
    calls: Arc<Mutex<Vec<CommandSpec>>>,
    fail_age: bool,
}

impl RecordingRunner {
    fn calls(&self) -> Vec<CommandSpec> {
        self.calls.lock().expect("calls").clone()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CoreError> {
        assert!(
            spec.shell.is_none(),
            "external tools must run through argv, not a shell"
        );
        self.calls.lock().expect("calls").push(spec.clone());
        let program = spec.argv.first().map(String::as_str).unwrap_or_default();
        if program == "age" {
            if self.fail_age {
                return Err(CoreError::Io {
                    path: "age".to_owned(),
                    source: std::io::Error::from(std::io::ErrorKind::NotFound),
                });
            }
            let output = spec
                .argv
                .iter()
                .position(|arg| arg == "-o")
                .and_then(|index| spec.argv.get(index + 1))
                .map(PathBuf::from);
            let source = spec.argv.last().map(PathBuf::from);
            if let (Some(output), Some(source)) = (output, source) {
                if let Some(parent) = output.parent() {
                    std::fs::create_dir_all(parent).expect("age parent");
                }
                std::fs::copy(&source, &output).expect("fake age copy");
            }
        }
        Ok(CommandOutput {
            exit_code: Some(0),
            timed_out: false,
            output: Vec::new(),
            duration: Duration::ZERO,
        })
    }
}

fn timestamp(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("timestamp")
}

fn backup_directories(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .expect("backup dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn backup_default_writes_full_backup_directory() {
    let env = common::TestEnv::new();
    let (code, stdout, stderr) = env.run(&["backup"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("files"), "{stdout}");

    let backups = env.home_path().join("backups");
    let directories = backup_directories(&backups);
    assert_eq!(directories.len(), 1, "{directories:?}");
    let backup = backups.join(&directories[0]);
    assert!(backup.join("global.db").is_file());
    assert!(backup.join("manifest.json").is_file());
    assert!(backup.join("catalog.json").is_file());
}

#[test]
fn backup_global_and_all_project_dbs_are_openable() {
    let fixture = BackupFixture::new();
    let alpha = fixture.env.config.project.name.clone();
    let beta = fixture.add_project("beta");
    fixture.insert_job(&alpha, "alpha job");
    fixture.insert_job(&beta, "beta job");

    let store = fixture.store();
    let destination = fixture.env.home_path().join("all-projects");
    let report = fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(destination.display().to_string()),
                ..BackupOptions::default()
            },
        )
        .expect("backup");
    assert!(!report.files.is_empty());

    assert!(destination.join("global.db").is_file());
    assert!(
        destination
            .join("projects")
            .join(format!("{}.db", alpha.as_str()))
            .is_file()
    );
    assert!(
        destination
            .join("projects")
            .join(format!("{}.db", beta.as_str()))
            .is_file()
    );

    let restore_home = tempfile::tempdir().expect("restore home");
    fixture
        .run_restore(&store, &destination, restore_home.path(), false)
        .expect("restore");

    let paths = NodePaths::new(restore_home.path());
    let global = Store::open_global(&paths).expect("global");
    let projects = global.list_projects().expect("projects");
    assert_eq!(projects.len(), 2);
    for (project, title) in [(&alpha, "alpha job"), (&beta, "beta job")] {
        let restored = Store::open_with_project(&paths, project).expect("project");
        let jobs = restored.list_jobs(&JobFilter::default()).expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, title);
    }
}

#[test]
fn backup_rotation_seven_daily_four_weekly() {
    let fixture = BackupFixture::new();
    let store = fixture.store();
    let start = timestamp("2026-01-01T02:00:00Z");
    for day in 0..20 {
        fixture.clock.set(start.plus(Duration::from_secs(day * 24 * 60 * 60)));
        fixture.run_backup(&store, BackupOptions::default()).expect("backup");
    }

    let names = backup_directories(&fixture.env.home_path().join("backups"));
    // Seven newest days (2026-01-14 .. 2026-01-20) plus the newest day of each of the
    // four latest ISO weeks (2026-01-04, 2026-01-11, 2026-01-18, 2026-01-20).
    assert_eq!(names.len(), 9, "{names:?}");
    for day in [
        "20260104", "20260111", "20260114", "20260115", "20260116", "20260117", "20260118", "20260119", "20260120",
    ] {
        assert!(
            names.iter().any(|name| name.starts_with(day)),
            "missing {day} in {names:?}"
        );
    }
    assert!(!names.iter().any(|name| name.starts_with("20260110")), "{names:?}");
    assert!(!names.iter().any(|name| name.starts_with("20260113")), "{names:?}");
}

#[test]
fn backup_to_rsync_uses_argv_not_shell() {
    let fixture = BackupFixture::new();
    let store = fixture.store();
    let target = "rsync://backup.example/vowp/";
    let report = fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(target.to_owned()),
                ..BackupOptions::default()
            },
        )
        .expect("offsite backup");
    assert_eq!(report.offsite.as_deref(), Some(target));

    let calls = fixture.runner.calls();
    let rsync = calls
        .iter()
        .find(|call| call.argv.first().map(String::as_str) == Some("rsync"))
        .expect("rsync call");
    assert!(rsync.shell.is_none());
    assert_eq!(rsync.argv[0], "rsync");
    assert_eq!(rsync.argv[1], "-a");
    assert_eq!(rsync.argv[2], format!("{}/", report.directory));
    assert_eq!(rsync.argv[3], target);
    assert!(!fixture.env.home_path().join("rsync:").exists());
}

#[test]
fn backup_age_encryption_when_configured() {
    let fixture = BackupFixture::new();
    let store = fixture.store();
    let destination = fixture.env.home_path().join("encrypted");
    let report = fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(destination.display().to_string()),
                encrypt: Some("age1testrecipient".to_owned()),
                ..BackupOptions::default()
            },
        )
        .expect("encrypted backup");
    assert!(report.encrypted);
    assert!(report.files.iter().all(|file| file.path.ends_with(".age")));
    assert!(destination.join("global.db.age").is_file());
    assert!(!destination.join("global.db").exists());

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).expect("manifest"))
            .expect("manifest json");
    assert_eq!(manifest["encrypted"], true);

    let age = fixture
        .runner
        .calls()
        .into_iter()
        .find(|call| call.argv.first().map(String::as_str) == Some("age"))
        .expect("age call");
    assert!(age.argv.windows(2).any(|pair| pair == ["-r", "age1testrecipient"]));
    assert!(age.shell.is_none());

    let restore_home = tempfile::tempdir().expect("restore home");
    fixture
        .run_restore(&store, &destination, restore_home.path(), false)
        .expect("encrypted restore");
    assert!(restore_home.path().join("vowp.db").is_file());
}

#[test]
fn backup_age_missing_binary_is_refused_with_exit_5() {
    let fixture = BackupFixture {
        runner: RecordingRunner {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_age: true,
        },
        ..BackupFixture::new()
    };
    let store = fixture.store();
    let error = fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(fixture.env.home_path().join("missing-age").display().to_string()),
                encrypt: Some("age1testrecipient".to_owned()),
                ..BackupOptions::default()
            },
        )
        .expect_err("missing age must fail");
    assert_eq!(error.kind(), ErrorKind::Refused);
    assert_eq!(vowp_cli::CliError::from(error).exit_code(), 5);
    assert!(!fixture.env.home_path().join("missing-age").exists());
}

#[test]
fn restore_drill_reopens_and_counts_match() {
    let fixture = BackupFixture::new();
    let alpha = fixture.env.config.project.name.clone();
    let beta = fixture.add_project("beta");
    fixture.insert_job(&alpha, "alpha job");
    fixture.insert_job(&beta, "beta job");

    let (code, stdout, stderr) = fixture.env.run(&["session", "new", "--title", "drill session"]);
    assert_eq!(code, 0, "{stderr}");
    let session = SessionId::parse(stdout.trim()).expect("session id");

    let store = fixture.store();
    let backup = fixture.env.home_path().join("drill-backup");
    fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(backup.display().to_string()),
                ..BackupOptions::default()
            },
        )
        .expect("backup");

    let restore_home = tempfile::tempdir().expect("restore home");
    let report = fixture
        .run_restore(&store, &backup, restore_home.path(), false)
        .expect("restore");
    assert!(!report.files.is_empty());

    let paths = NodePaths::new(restore_home.path());
    let global = Store::open_global(&paths).expect("global");
    let sessions = global.list_sessions(&SessionFilter::default()).expect("sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session);
    assert_eq!(sessions[0].title, "drill session");
    assert_eq!(global.list_projects().expect("projects").len(), 2);

    for (project, title) in [(&alpha, "alpha job"), (&beta, "beta job")] {
        let restored = Store::open_with_project(&paths, project).expect("project");
        let jobs = restored.list_jobs(&JobFilter::default()).expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, title);
    }

    let conflict = fixture
        .run_restore(&store, &backup, restore_home.path(), false)
        .expect_err("existing home needs --force");
    assert_eq!(conflict.kind(), ErrorKind::Conflict);
    fixture
        .run_restore(&store, &backup, restore_home.path(), true)
        .expect("forced restore");
}

#[test]
fn backup_includes_session_logs_and_blobs() {
    let fixture = BackupFixture::new();
    let store = fixture.store();
    let session = SessionId::new();
    let session_dir = fixture.env.home_path().join("sessions").join(session.as_str());
    std::fs::create_dir_all(session_dir.join("blobs").join("ab")).expect("blobs dir");
    std::fs::create_dir_all(session_dir.join("capsules")).expect("capsules dir");
    std::fs::write(session_dir.join("log.jsonl"), "{\"seq\":1}\n").expect("log");
    std::fs::write(session_dir.join("blobs").join("ab").join("blob.bin"), b"blob bytes").expect("blob");
    std::fs::write(session_dir.join("capsules").join("cap.json"), "{\"id\":\"cap\"}\n").expect("capsule");
    std::fs::write(session_dir.join("adapters.json"), "{\"adapters\":[]}\n").expect("adapters");
    std::fs::write(session_dir.join("summary.md"), "# summary\n").expect("summary");

    let secrets = fixture.env.home_path().join("secrets");
    std::fs::create_dir_all(&secrets).expect("secrets dir");
    std::fs::write(secrets.join("API_KEY"), "do-not-copy").expect("secret");

    let destination = fixture.env.home_path().join("session-backup");
    let report = fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(destination.display().to_string()),
                ..BackupOptions::default()
            },
        )
        .expect("backup");
    assert!(
        report.skipped.iter().any(|entry| entry.starts_with("secrets/")),
        "{:?}",
        report.skipped
    );

    let root = destination.join("sessions").join(session.as_str());
    assert_eq!(std::fs::read(root.join("log.jsonl")).expect("log"), b"{\"seq\":1}\n");
    assert_eq!(
        std::fs::read(root.join("blobs").join("ab").join("blob.bin")).expect("blob"),
        b"blob bytes"
    );
    assert!(root.join("capsules").join("cap.json").is_file());
    assert!(root.join("adapters.json").is_file());
    assert!(root.join("summary.md").is_file());
    assert!(!destination.join("secrets").exists());

    let restore_home = tempfile::tempdir().expect("restore home");
    fixture
        .run_restore(&store, &destination, restore_home.path(), false)
        .expect("restore");
    let restored = restore_home.path().join("sessions").join(session.as_str());
    assert_eq!(
        std::fs::read(restored.join("log.jsonl")).expect("restored log"),
        b"{\"seq\":1}\n"
    );
    assert_eq!(
        std::fs::read(restored.join("blobs").join("ab").join("blob.bin")).expect("restored blob"),
        b"blob bytes"
    );
}

#[test]
fn backup_manifest_records_counts() {
    let fixture = BackupFixture::new();
    let alpha = fixture.env.config.project.name.clone();
    fixture.insert_job(&alpha, "counted job");
    let store = fixture.store();
    let destination = fixture.env.home_path().join("counted-backup");
    fixture
        .run_backup(
            &store,
            BackupOptions {
                to: Some(destination.display().to_string()),
                ..BackupOptions::default()
            },
        )
        .expect("backup");

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).expect("manifest"))
            .expect("manifest json");
    let counts = manifest["row_counts"]["projects"][alpha.as_str()]["jobs"]
        .as_u64()
        .expect("job count");
    assert_eq!(counts, 1);
    assert_eq!(manifest["row_counts"]["global"]["projects"], 1);
}
