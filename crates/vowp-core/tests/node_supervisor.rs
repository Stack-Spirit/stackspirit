//! Node supervisor tests: one supervisor, every active project, round-robin.
//!
//! Spec: `docs/design/v0.2/home-and-surfaces.md` §4.2–§4.3. Everything is hermetic: a
//! temporary node home, per-project `vowp.toml` files, a fake clock, a fake window manager
//! and no harness process.

use std::sync::Arc;

use tempfile::TempDir;
use vowp_core::deps::{AdapterRegistry, FixedRng, SystemRunner};
use vowp_core::loops::NodeSupervisor;
use vowp_core::{Deps, SystemGit};
use vowp_protocol::{
    ActiveState, FakeClock, InboxState, Job, JobKind, JobState, MemoryLog, NewJob, NodeName, Project, ProjectName,
    ProjectState, Role, Timestamp, VowpConfig,
};
use vowp_store::{NodePaths, Store};
use vowp_tmux::fake::FakeWindows;

/// Fixed instant every test clock starts at.
fn ts() -> Timestamp {
    Timestamp::parse("2026-11-08T18:00:00Z").expect("timestamp")
}

/// One temporary node with a global store and fake dependencies.
struct NodeEnv {
    tmp: TempDir,
    paths: NodePaths,
    deps: Deps,
    clock: FakeClock,
    log: Arc<MemoryLog>,
}

impl NodeEnv {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let paths = NodePaths::new(tmp.path().join("node"));
        paths.ensure().expect("node paths");
        let mut config = VowpConfig::with_defaults_for_tests();
        config.project.name = ProjectName::parse("base").expect("project");
        config.project.path = Some(tmp.path().join("base").display().to_string());
        let clock = FakeClock::at(ts());
        let log = Arc::new(MemoryLog::new());
        let windows = Arc::new(FakeWindows::new());
        let cmd = Arc::new(SystemRunner);
        let deps = Deps {
            paths: paths.clone(),
            config: Arc::new(config),
            node: NodeName::parse("local").expect("node"),
            clock: Arc::new(clock.clone()),
            log: log.clone(),
            adapters: Arc::new(AdapterRegistry::new()),
            windows: windows.clone(),
            git: Arc::new(SystemGit::new(cmd.clone())),
            cmd,
            rng: Arc::new(FixedRng(0)),
        };
        Self {
            tmp,
            paths,
            deps,
            clock,
            log,
        }
    }

    /// Returns a global store handle.
    fn global(&self) -> Store {
        Store::open_global(&self.paths)
            .expect("global store")
            .with_clock(Arc::new(self.clock.clone()))
    }

    /// Writes a project checkout with `vowp.toml` and registers it.
    fn add_project<F>(&self, name: &str, configure: F) -> ProjectName
    where
        F: FnOnce(&mut VowpConfig),
    {
        let name = ProjectName::parse(name).expect("project name");
        let root = self.tmp.path().join(name.as_str());
        std::fs::create_dir_all(root.join(".vowp/jobs")).expect("project dir");
        let mut config = VowpConfig::with_defaults_for_tests();
        config.project.name = name.clone();
        config.project.path = Some(root.display().to_string());
        config.project.worktree_root = Some(self.tmp.path().join("worktrees").display().to_string());
        configure(&mut config);
        std::fs::write(
            root.join("vowp.toml"),
            toml::to_string_pretty(&config).expect("config toml"),
        )
        .expect("write vowp.toml");

        let store = self.global();
        let now = ts();
        store
            .upsert_project(&Project {
                name: name.clone(),
                path: root.display().to_string(),
                remote_url: None,
                default_branch: "main".to_owned(),
                config: serde_json::to_value(&config).expect("config json"),
                created_at: now,
                updated_at: now,
            })
            .expect("project row");
        name
    }

    /// Inserts one job into one project's database.
    fn insert_job(&self, project: &ProjectName, id: &str, state: JobState) -> Job {
        let store = Store::open_with_project(&self.paths, project)
            .expect("project store")
            .with_clock(Arc::new(self.clock.clone()));
        let job = store
            .insert_job(
                project,
                &NewJob {
                    id: vowp_protocol::JobId::parse(id).expect("job id"),
                    title: format!("job {id}"),
                    kind: JobKind::Build,
                    role: Role::Worker,
                    profile: Some("worker".to_owned()),
                    needs: Vec::new(),
                    owns: Vec::new(),
                    reads: Vec::new(),
                    gates: None,
                    priority: None,
                    loop_: None,
                    brief_text: Some("Do the work.\n".to_owned()),
                },
                "test",
            )
            .expect("insert job");
        if job.state != state {
            store
                .set_job_state(
                    &job.id,
                    state,
                    &vowp_store::StateChange {
                        note: None,
                        parked_reason: None,
                    },
                )
                .expect("state");
        }
        store.require_job(&job.id).expect("job")
    }

    /// Returns the project names named by the `projects` step, in tick order.
    fn tick_order(report: &vowp_core::loops::NodeTickReport) -> Vec<String> {
        let detail = &report
            .steps
            .iter()
            .find(|step| step.name == "projects")
            .expect("projects step")
            .detail;
        let names = detail.split_once(": ").map(|(_, names)| names).unwrap_or_default();
        names
            .split(", ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

impl std::ops::Deref for NodeEnv {
    type Target = Deps;

    fn deref(&self) -> &Deps {
        &self.deps
    }
}

#[test]
fn new_registers_the_reserved_node_project_but_does_not_tick_it() {
    let env = NodeEnv::new();
    let node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    assert_eq!(node.projects().count(), 0, "only _node exists");
    let global = env.global();
    let reserved = ProjectName::parse("_node").expect("reserved");
    assert!(global.project(&reserved).expect("lookup").is_some());
    assert!(env.paths.project_db_path(&reserved).is_file());
    assert_eq!(node.order(), Vec::<ProjectName>::new());
}

#[test]
fn one_supervisor_round_robins_every_active_project() {
    let env = NodeEnv::new();
    for name in ["alpha", "beta", "gamma"] {
        env.add_project(name, |_| {});
    }
    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");

    let first = node.tick().expect("tick 1");
    assert_eq!(NodeEnv::tick_order(&first), ["alpha", "beta", "gamma"]);
    assert_eq!(first.projects.len(), 3);
    assert!(first.skipped.is_empty(), "{:?}", first.skipped);

    let second = node.tick().expect("tick 2");
    assert_eq!(NodeEnv::tick_order(&second), ["beta", "gamma", "alpha"]);
    let third = node.tick().expect("tick 3");
    assert_eq!(NodeEnv::tick_order(&third), ["gamma", "alpha", "beta"]);
    assert_eq!(
        node.order()
            .into_iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>(),
        ["alpha", "beta", "gamma"]
    );
}

#[test]
fn archived_projects_are_skipped_and_removed_supervisors_stop() {
    let env = NodeEnv::new();
    let alpha = env.add_project("alpha", |_| {});
    let archived = env.add_project("archived", |_| {});
    env.global()
        .set_project_state(&archived, ProjectState::Archived, "owner")
        .expect("archive");
    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    let report = node.tick().expect("tick");
    assert!(report.projects.contains_key(&alpha));
    assert!(!report.projects.contains_key(&archived));
    assert!(
        report.skipped.iter().any(|(name, _)| name == &archived),
        "{:?}",
        report.skipped
    );
}

#[test]
fn broken_project_parks_alone_after_three_failed_ticks() {
    let env = NodeEnv::new();
    let alpha = env.add_project("alpha", |_| {});
    let broken = env.add_project("broken", |_| {});
    // A needs cycle makes `Graph::new` fail on every tick, so the project's `states`
    // step (and dispatch) fails deterministically while every other project is healthy.
    let first = env.insert_job(&broken, "cycle-a", JobState::Pending);
    let second = env.insert_job(&broken, "cycle-b", JobState::Pending);
    let broken_store = Store::open_with_project(&env.paths, &broken).expect("store");
    for (src, dst) in [(&first.id, &second.id), (&second.id, &first.id)] {
        broken_store
            .add_edge(&vowp_protocol::Edge {
                src: src.clone(),
                dst: dst.clone(),
                kind: vowp_protocol::EdgeKind::Needs,
            })
            .expect("cycle edge");
    }

    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    node.startup().expect("startup");
    for _ in 0..3 {
        let report = node.tick().expect("tick");
        assert!(report.projects.contains_key(&alpha), "alpha keeps ticking");
        assert!(
            report.projects.contains_key(&broken),
            "broken still reports its failure"
        );
        assert!(
            report.projects[&broken].steps.iter().any(|step| !step.ok),
            "the broken project must report a failed step: {:?}",
            report.projects[&broken].steps
        );
        assert!(
            report.projects[&alpha].steps.iter().all(|step| step.ok),
            "the healthy project must stay green"
        );
    }

    let global = env.global();
    let entry = global.project_entry(&broken).expect("lookup").expect("entry");
    assert_eq!(entry.state, ProjectState::Paused);
    assert!(entry.paused_reason.starts_with("broken:"), "{}", entry.paused_reason);
    let broken_store = Store::open_with_project(&env.paths, &broken).expect("store");
    let loop_ = broken_store.loop_("l1-supervisor").expect("loop").expect("row");
    assert_eq!(loop_.fail_streak, 3, "three consecutive failures");
    let open = global.list_inbox(Some(InboxState::Open)).expect("inbox");
    assert!(
        open.iter().any(|item| {
            item.project.as_ref() == Some(&broken)
                && item.kind == vowp_protocol::InboxKind::Notice
                && item.title == "project broken paused"
        }),
        "{open:?}"
    );

    let fourth = node.tick().expect("tick 4");
    assert!(fourth.projects.contains_key(&alpha));
    assert!(!fourth.projects.contains_key(&broken));
    assert!(
        fourth
            .skipped
            .iter()
            .any(|(name, reason)| name == &broken && reason.starts_with("broken:")),
        "{:?}",
        fourth.skipped
    );

    // `vowp project resume` clears the reason; the next tick tries the project again.
    global
        .set_project_state(&broken, ProjectState::Active, "")
        .expect("resume");
    let fifth = node.tick().expect("tick 5");
    assert!(fifth.projects.contains_key(&broken), "resumed project ticks again");
    assert!(
        fifth.projects[&broken].steps.iter().any(|step| !step.ok),
        "the underlying failure is still visible"
    );
}

#[test]
fn projects_do_not_shadow_each_others_databases() {
    let env = NodeEnv::new();
    let alpha = env.add_project("alpha", |_| {});
    let beta = env.add_project("beta", |_| {});
    let alpha_job = env.insert_job(&alpha, "alpha-job", JobState::Pending);
    let beta_job = env.insert_job(&beta, "beta-job", JobState::Pending);

    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    node.tick().expect("tick");

    let alpha_store = Store::open_with_project(&env.paths, &alpha).expect("alpha store");
    let beta_store = Store::open_with_project(&env.paths, &beta).expect("beta store");
    assert_ne!(
        alpha_store.require_job(&alpha_job.id).expect("alpha job").state,
        JobState::Pending,
        "the node tick touched alpha's job"
    );
    assert_ne!(
        beta_store.require_job(&beta_job.id).expect("beta job").state,
        JobState::Pending,
        "the node tick touched beta's job"
    );
    assert!(
        alpha_store.job(&beta_job.id).expect("lookup").is_none(),
        "a project store never sees another project's jobs"
    );
    assert!(
        beta_store.job(&alpha_job.id).expect("lookup").is_none(),
        "a project store never sees another project's jobs"
    );
}

#[test]
fn one_active_project_keeps_the_v01_board_and_node_heartbeat() {
    let env = NodeEnv::new();
    env.add_project("solo", |_| {});
    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    let report = node.tick().expect("tick");
    assert_eq!(report.projects.len(), 1);
    assert!(env.paths.root().join("heartbeat").is_file());
    let board: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env.paths.root().join("board.json")).expect("board"))
            .expect("board json");
    assert_eq!(board["project"], "solo");
    let store = Store::open_with_project(&env.paths, &ProjectName::parse("solo").expect("name")).expect("store");
    let loop_ = store.loop_("l1-supervisor").expect("loop").expect("row");
    assert_eq!(loop_.state, ActiveState::Active);
    assert_eq!(loop_.last_status, "ok");
}

#[test]
fn two_active_projects_defer_the_node_board_but_still_heartbeat() {
    let env = NodeEnv::new();
    env.add_project("one", |_| {});
    env.add_project("two", |_| {});
    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    let report = node.tick().expect("tick");
    assert_eq!(report.projects.len(), 2);
    assert!(!env.paths.root().join("board.json").exists(), "D1 owns the node board");
    assert!(env.paths.root().join("heartbeat").is_file());
    assert!(!env.log.contains("board failed"), "{:?}", env.log.lines());
}

#[test]
fn a_project_config_that_disagrees_with_the_registry_parks_that_project_only() {
    let env = NodeEnv::new();
    let alpha = env.add_project("alpha", |_| {});
    let wrong = env.add_project("wrong", |_| {});
    // Rewrite the checkout's config so it names another project.
    let root = env.tmp.path().join("wrong");
    let mut config = VowpConfig::with_defaults_for_tests();
    config.project.name = ProjectName::parse("other").expect("name");
    config.project.path = Some(root.display().to_string());
    std::fs::write(root.join("vowp.toml"), toml::to_string_pretty(&config).expect("config")).expect("rewrite");

    let mut node = NodeSupervisor::new(env.deps.clone()).expect("supervisor");
    let report = node.tick().expect("tick");
    assert!(report.projects.contains_key(&alpha));
    assert!(
        report
            .skipped
            .iter()
            .any(|(name, reason)| name == &wrong && reason.contains("registry entry")),
        "{:?}",
        report.skipped
    );
    let entry = env.global().project_entry(&wrong).expect("lookup").expect("entry");
    assert_eq!(entry.state, ProjectState::Paused);
    assert!(entry.paused_reason.starts_with("broken:"));
}

#[test]
fn the_node_systemd_unit_matches_the_home_supervisor_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let installed = std::fs::read_to_string(root.join("scripts/vowp.service")).expect("installed unit");
    let template =
        std::fs::read_to_string(root.join("framework/templates/scripts/vowp.service")).expect("unit template");
    assert_eq!(installed, template, "the two unit copies must not drift");
    for line in [
        "Environment=VOWP_HOME=%h/.vowp",
        "ExecStart=%h/.local/bin/vowp up --foreground",
        "Restart=on-failure",
        "RestartSec=5",
        "KillMode=mixed",
        "WantedBy=default.target",
    ] {
        assert!(installed.contains(line), "unit is missing {line:?}:\n{installed}");
        assert!(
            vowp_core::loops::l8::SYSTEMD_UNIT.contains(line),
            "the embedded unit template is missing {line:?}"
        );
    }
    assert!(
        !installed.contains("Restart=always"),
        "always would restart the supervisor after `vowp down` exits 0"
    );
}
