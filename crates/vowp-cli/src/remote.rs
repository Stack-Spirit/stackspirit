//! `--node <name>` redirection over ssh and the control-machine route to the home.
//!
//! Spec: `docs/design/cli.md` §7, `docs/design/crates/vowp-cli.md` §2 and
//! `docs/design/v0.2/home-and-surfaces.md` §8.1–§8.3. The argv builder is pure and tested
//! without executing anything; [`run_remote`] returns the remote exit code unchanged and
//! maps ssh's own failure (255) to [`CliError::Remote`].
//!
//! On a control machine every command that needs the home is routed here, before
//! [`crate::Ctx::build`] would look for a local database. `fleet status` refreshes the
//! snapshot cache from the home's `snapshot.json`; `status` and `fleet status` serve the
//! cache with an "as of" time when the home is unreachable. `status --check` keeps the
//! v0.1 watchdog contract: a cached snapshot whose heartbeat is stale exits 1. Writes
//! never use the cache.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use serde::{Deserialize, Serialize};
use vowp_protocol::{FleetStatus, Node, NodeName, Snapshot, Timestamp};
use vowp_store::Store;
use vowp_tmux::quote::shell_quote;

use crate::CliError;
use crate::cli::{Command as CliCommand, Global, SessionCommand};
use crate::node_ctx::{CachedSnapshot, ControlConfig, ControlHome, NodeCtx};
use crate::output::{self, Table};

/// Redirects the command when `--node` names a remote node or this is a control machine.
///
/// Returns `Ok(None)` when the command must run locally. On a control machine that means
/// the control/tunnel/node/setup group and every command that only reads local files; a
/// command that needs local truth either routes to the home or is refused by
/// [`control_refusal`].
pub fn maybe_run(global: &Global, command: &CliCommand, original_args: &[String]) -> Result<Option<i32>, CliError> {
    let node = NodeCtx::discover(global)?;
    let control = node.require_control().ok().cloned();

    if let Some(name) = &global.node {
        let name = NodeName::parse(name)?;
        match resolve_target(global, control.as_ref(), &name)? {
            Some(target) => {
                let args = match control.as_ref() {
                    Some(control) if control.config.home == name.as_str() => {
                        control_remote_args(&control.config.remote_home, original_args)
                    }
                    _ => strip_node_flag(original_args),
                };
                let interactive = is_interactive(command);
                return run_remote(&target, &args, interactive).map(Some);
            }
            None if control.is_none() => return Ok(None),
            None => {}
        }
    }

    let Some(control) = control else {
        return Ok(None);
    };
    match control_action(command) {
        ControlAction::Local => Ok(None),
        ControlAction::Refuse => Err(control_refusal()),
        ControlAction::Route(RouteKind::Plain) => {
            let args = control_remote_args(&control.config.remote_home, original_args);
            run_remote(&control.config.ssh_target, &args, is_interactive(command)).map(Some)
        }
        ControlAction::Route(RouteKind::Status) => {
            route_status(global, &control, command, original_args, false).map(Some)
        }
        ControlAction::Route(RouteKind::FleetStatus) => {
            let cached = fleet_status_cached(command);
            let refresh = fleet_status_refresh(command);
            route_status(global, &control, command, original_args, cached || refresh).map(Some)
        }
    }
}

/// How a command behaves on a control machine.
enum ControlAction {
    /// Run in this process, against the control directory.
    Local,
    /// The command needs local truth and is refused.
    Refuse,
    /// Send it to the home.
    Route(RouteKind),
}

/// What kind of answer a routed command needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteKind {
    /// Plain pass-through of the home's human or JSON output.
    Plain,
    /// `vowp status`, which has a cached fallback.
    Status,
    /// `vowp fleet status`, which also has a cached fallback and `--cached`/`--refresh`.
    FleetStatus,
}

/// Returns whether the command needs the home on a control machine.
fn control_action(command: &CliCommand) -> ControlAction {
    match command {
        // These read or write the control machine itself.
        CliCommand::Control(_)
        | CliCommand::Tunnel(_)
        | CliCommand::Node { .. }
        | CliCommand::Setup(_)
        | CliCommand::Doctor(_)
        | CliCommand::Version
        | CliCommand::Schema(_)
        | CliCommand::Mcp(_)
        | CliCommand::Window(_)
        | CliCommand::Exec(_)
        | CliCommand::VaultExec(_)
        | CliCommand::VaultAdmin(_) => ControlAction::Local,
        // These need a local supervisor, graph or worktrees.
        CliCommand::Up(_) | CliCommand::Down(_) | CliCommand::Board(_) => ControlAction::Refuse,
        CliCommand::Status(_) => ControlAction::Route(RouteKind::Status),
        CliCommand::Fleet(args) => match &args.command {
            crate::cmd::fleet::FleetCommand::Status(_) => ControlAction::Route(RouteKind::FleetStatus),
            _ => ControlAction::Route(RouteKind::Plain),
        },
        _ => ControlAction::Route(RouteKind::Plain),
    }
}

/// Returns the guard error for a command that cannot run on a control machine.
fn control_refusal() -> CliError {
    CliError::Guard(
        "this is a control machine; the home runs the supervisor (use `vowp control status` and `vowp fleet status`)"
            .to_owned(),
    )
}

/// Resolves the ssh target named by `--node`, or `None` when it is this machine.
fn resolve_target(global: &Global, control: Option<&ControlHome>, name: &NodeName) -> Result<Option<String>, CliError> {
    if let Some(control) = control {
        if control.config.home == name.as_str() {
            return Ok(Some(control.config.ssh_target.clone()));
        }
        let nodes = read_node_registry(&control.root)?;
        let node = nodes
            .into_iter()
            .find(|node| node.name == *name)
            .ok_or_else(|| CliError::NotFound(format!("node {name}")))?;
        if node.is_local {
            return Ok(None);
        }
        return node
            .ssh_target
            .map(Some)
            .ok_or_else(|| CliError::Invalid(format!("node {name} has no ssh target")));
    }

    let paths = crate::ctx::node_paths(global)?;
    if !paths.db_path().is_file() {
        return Err(CliError::NotFound(
            "vowp home is not initialised; run `vowp init`".to_owned(),
        ));
    }
    let store = Store::open(&paths)?;
    let node = store
        .node(name)?
        .ok_or_else(|| CliError::NotFound(format!("node {name}")))?;
    if node.is_local {
        return Ok(None);
    }
    node.ssh_target
        .map(Some)
        .ok_or_else(|| CliError::Invalid(format!("node {name} has no ssh target")))
}

/// Returns true for commands that attach to a terminal on the remote node.
pub fn is_interactive(command: &CliCommand) -> bool {
    match command {
        CliCommand::Attach(_) | CliCommand::Chat(_) => true,
        CliCommand::Session {
            command: SessionCommand::Chat(_),
        } => true,
        CliCommand::Board(args) => !args.once,
        _ => false,
    }
}

/// Removes `--node <name>` and `--node=<name>` from an argument list.
pub fn strip_node_flag(args: &[String]) -> Vec<String> {
    strip_value_flag(args, "--node")
}

/// Removes `--home <dir>` and `--home=<dir>` from an argument list.
pub fn strip_home_flag(args: &[String]) -> Vec<String> {
    strip_value_flag(args, "--home")
}

/// Removes one global flag that takes a separate or `=` value.
fn strip_value_flag(args: &[String], flag: &str) -> Vec<String> {
    let prefix = format!("{flag}=");
    let mut stripped = Vec::with_capacity(args.len());
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == flag {
            skip_next = true;
        } else if !arg.starts_with(&prefix) {
            stripped.push(arg.clone());
        }
    }
    stripped
}

/// Builds the arguments for a control-machine call to the home.
///
/// The local arguments are forwarded unchanged except that the local `--home` (if one
/// slipped through) is replaced by the home's absolute path. The quoted ssh argv does not
/// expand `~`, so the home path is always absolute.
fn control_remote_args(remote_home: &str, args: &[String]) -> Vec<String> {
    let mut routed = vec!["--home".to_owned(), remote_home.to_owned()];
    routed.extend(strip_home_flag(&strip_node_flag(args)));
    routed
}

/// Builds `ssh -o BatchMode=yes -o ConnectTimeout=10 [-t] <target> vowp <quoted args>`.
///
/// Every remote argument is POSIX single-quoted with [`shell_quote`]; `ssh` joins the final
/// words into one command string and the remote shell must see the exact original arguments.
pub fn build_ssh_argv(target: &str, args: &[String], interactive: bool) -> Vec<String> {
    let mut argv = vec![
        "ssh".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=10".to_owned(),
    ];
    if interactive {
        argv.push("-t".to_owned());
    }
    argv.push(target.to_owned());

    let quoted: Vec<String> = args.iter().map(|arg| shell_quote(arg)).collect();
    let mut remote = String::from("vowp");
    if !quoted.is_empty() {
        remote.push(' ');
        remote.push_str(&quoted.join(" "));
    }
    argv.push(remote);
    argv
}

/// Runs `ssh` with inherited stdio and returns the remote exit code.
///
/// Exit 255 means ssh itself failed (connection, authentication, missing remote `vowp`);
/// that becomes [`CliError::Remote`] and exit code 7.
pub fn run_remote(target: &str, args: &[String], interactive: bool) -> Result<i32, CliError> {
    let argv = build_ssh_argv(target, args, interactive);
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| CliError::Remote("empty ssh argv".to_owned()))?;
    let status = ProcessCommand::new(program)
        .args(rest)
        .status()
        .map_err(|error| CliError::Remote(format!("ssh {target}: {error}")))?;
    match status.code() {
        Some(255) => Err(CliError::Remote(format!("ssh {target} failed with exit 255"))),
        Some(code) => Ok(code),
        None => Err(CliError::Remote(format!("ssh {target} terminated by a signal"))),
    }
}

/// Routes `status` or `fleet status`, with the snapshot cache as the offline fallback.
///
/// Only a successful `fleet status` refreshes the cache from the home's `snapshot.json`;
/// `status` is a pass-through and adds no second ssh call. When ssh itself fails (255) and
/// the command did not ask for a refresh, the last cached snapshot is printed with its
/// "as of" time. `status --check` returns 1 when that cache has no fresh heartbeat, so the
/// watchdog contract survives an unreachable home. A write never reaches this function.
fn route_status(
    global: &Global,
    control: &ControlHome,
    command: &CliCommand,
    original_args: &[String],
    cache_only: bool,
) -> Result<i32, CliError> {
    let fleet_status = matches!(control_action(command), ControlAction::Route(RouteKind::FleetStatus));
    let refresh = fleet_status && fleet_status_refresh(command);
    let check = status_check(command);
    let kind = if fleet_status {
        RouteKind::FleetStatus
    } else {
        RouteKind::Status
    };

    if cache_only && !refresh {
        return print_cached(control, kind, global.json);
    }

    let args = control_remote_args(&control.config.remote_home, original_args);
    match run_remote(&control.config.ssh_target, &args, false) {
        Ok(code) => {
            // `fleet status` is the read-model command that refreshes the snapshot cache.
            // `status` is a pass-through; it must not add a second ssh call behind the
            // user's command.
            if code == 0 && fleet_status {
                let _ = refresh_cache(control);
            }
            Ok(code)
        }
        Err(CliError::Remote(message)) if !refresh => match cached_snapshot(&control.root)? {
            Some(cached) => {
                let now = Timestamp::now();
                let stale = cached_is_stale_at(&cached, now);
                print_cached_value(kind, &cached, global.json, &message, stale)?;
                // A stale heartbeat is the v0.1 watchdog's failure signal, even when the
                // only available document came from the cache.
                if check && heartbeat_is_stale(&cached.snapshot, now) {
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
            None => Err(CliError::Remote(message)),
        },
        Err(error) => Err(error),
    }
}

/// Returns the `--check` flag of `status`.
fn status_check(command: &CliCommand) -> bool {
    match command {
        CliCommand::Status(args) => args.check,
        _ => false,
    }
}

/// Returns the `--cached` flag of `fleet status`.
fn fleet_status_cached(command: &CliCommand) -> bool {
    match command {
        CliCommand::Fleet(args) => match &args.command {
            crate::cmd::fleet::FleetCommand::Status(status) => status.cached,
            _ => false,
        },
        _ => false,
    }
}

/// Returns the `--refresh` flag of `fleet status`.
fn fleet_status_refresh(command: &CliCommand) -> bool {
    match command {
        CliCommand::Fleet(args) => match &args.command {
            crate::cmd::fleet::FleetCommand::Status(status) => status.refresh,
            _ => false,
        },
        _ => false,
    }
}

/// Prints the cached snapshot, requiring one to exist.
fn print_cached(control: &ControlHome, kind: RouteKind, json: bool) -> Result<i32, CliError> {
    let Some(cached) = cached_snapshot(&control.root)? else {
        return Err(CliError::NotFound(format!(
            "no cached snapshot in {}; run `vowp fleet status --refresh`",
            control.root.join("snapshot.json").display()
        )));
    };
    let stale = cached_is_stale_at(&cached, Timestamp::now());
    print_cached_value(kind, &cached, json, "", stale)?;
    Ok(0)
}

/// Prints `status` or `fleet status` from a cached snapshot.
fn print_cached_value(
    kind: RouteKind,
    cached: &CachedSnapshot,
    json: bool,
    detail: &str,
    stale: bool,
) -> Result<(), CliError> {
    let now = Timestamp::now();
    if json {
        let text = match kind {
            RouteKind::FleetStatus => {
                let status = FleetStatus {
                    snapshot: cached.snapshot.clone(),
                    source: "cache".to_owned(),
                    fetched_at: cached.fetched_at,
                    stale,
                };
                serde_json::to_string_pretty(&status).map_err(vowp_protocol::ProtocolError::from)?
            }
            RouteKind::Status => {
                let value = status_json(&cached.snapshot, cached.fetched_at, stale);
                serde_json::to_string_pretty(&value).map_err(vowp_protocol::ProtocolError::from)?
            }
            RouteKind::Plain => return Ok(()),
        };
        write_stdout(&format!("{text}\n"));
        return Ok(());
    }

    let as_of = output::local_time(cached.fetched_at, now);
    let mut text = format!(
        "home: {} (cache, as of {as_of}, {})\n",
        cached.snapshot.home,
        if stale { "stale" } else { "fresh" }
    );
    if !detail.is_empty() {
        text.push_str(&format!("connection: {detail}\n"));
    }
    text.push_str(&render_snapshot_summary(&cached.snapshot));
    write_stdout(&text);
    Ok(())
}

/// Builds the v0.1-compatible `status --json` document from a snapshot.
fn status_json(snapshot: &Snapshot, fetched_at: Timestamp, stale: bool) -> serde_json::Value {
    let mut jobs = std::collections::BTreeMap::new();
    let mut running = 0_u32;
    for project in &snapshot.projects {
        for (state, count) in &project.jobs_by_state {
            *jobs.entry(state.clone()).or_insert(0_u32) += count;
        }
        running += project.running.len() as u32;
    }
    serde_json::json!({
        "home": &snapshot.home,
        "source": "cache",
        "as_of": fetched_at.as_rfc3339(),
        "stale": stale,
        "supervisor": {
            "running": snapshot.supervisor.running,
            "pid": snapshot.supervisor.pid,
            "last_tick": snapshot.supervisor.last_tick.map(|at| at.as_rfc3339()),
            "heartbeat_age_secs": serde_json::Value::Null,
            "paused": snapshot.supervisor.paused,
        },
        "jobs": jobs,
        "slots": {
            "busy": running,
            "total": snapshot.capacity.target.target_workers,
        },
        "limits": &snapshot.limits,
        "cost_today_usd": snapshot.projects.iter().filter_map(|p| p.cost_today_usd).reduce(|a, b| a + b),
        "inbox_open": snapshot.inbox.len(),
        "projects": snapshot.projects.iter().map(|project| serde_json::json!({
            "name": &project.name,
            "state": project.state,
            "running": project.running.len(),
            "ready": project.jobs_by_state.get("ready").copied().unwrap_or(0),
            "blocked": project.blocked.len(),
            "paused_reason": &project.paused_reason,
        })).collect::<Vec<_>>(),
    })
}

/// Renders the short human summary shared by cached `status` and `fleet status`.
fn render_snapshot_summary(snapshot: &Snapshot) -> String {
    let mut text = String::new();
    if snapshot.supervisor.running {
        match snapshot.supervisor.pid {
            Some(pid) => text.push_str(&format!("supervisor: running (pid {pid})\n")),
            None => text.push_str("supervisor: running\n"),
        }
    } else {
        text.push_str("supervisor: not running\n");
    }

    let mut table = Table::new(&["PROJECT", "STATE", "RUNNING", "QUEUED", "BLOCKED"]);
    for project in &snapshot.projects {
        let queued: u32 = project
            .jobs_by_state
            .iter()
            .filter(|(state, _)| {
                matches!(
                    state.as_str(),
                    "ready" | "waiting" | "admitted" | "queued" | "needs_attention"
                )
            })
            .map(|(_, count)| *count)
            .sum();
        table.row(vec![
            project.name.to_string(),
            format!("{:?}", project.state).to_lowercase(),
            project.running.len().to_string(),
            queued.to_string(),
            project.blocked.len().to_string(),
        ]);
    }
    if snapshot.projects.is_empty() {
        text.push_str("projects: none\n");
    } else {
        text.push_str(&table.render(false));
    }
    text.push_str(&format!("inbox: {} open\n", snapshot.inbox.len()));
    text
}

/// Returns whether a cached snapshot is stale.
///
/// A cache is stale when the fetch is older than three supervisor ticks, or when the
/// heartbeat recorded inside it is older than the v0.1 three-minute watchdog limit. The
/// second check matters when a fresh fetch carried an already stalled heartbeat.
fn cached_is_stale_at(cached: &CachedSnapshot, now: Timestamp) -> bool {
    snapshot_is_stale(&cached.snapshot, cached.fetched_at, now) || heartbeat_is_stale(&cached.snapshot, now)
}

/// Returns whether a cached snapshot is stale, measured at the current time.
pub(crate) fn cached_is_stale(cached: &CachedSnapshot) -> bool {
    cached_is_stale_at(cached, Timestamp::now())
}

/// Returns whether a snapshot is older than three supervisor ticks.
fn snapshot_is_stale(snapshot: &Snapshot, fetched_at: Timestamp, now: Timestamp) -> bool {
    let tick = if snapshot.supervisor.tick_secs == 0 {
        60
    } else {
        snapshot.supervisor.tick_secs
    };
    let limit = i64::try_from(tick.saturating_mul(3)).unwrap_or(i64::MAX);
    now.unix_secs().saturating_sub(fetched_at.unix_secs()) > limit
}

/// Returns whether the supervisor heartbeat in `snapshot` is stale.
///
/// This is the v0.1 watchdog contract from `cli.md` §2: a missing heartbeat or one older
/// than [`crate::cmd::status::STALE_AFTER_SECS`] means `status --check` exits 1.
fn heartbeat_is_stale(snapshot: &Snapshot, now: Timestamp) -> bool {
    snapshot.supervisor.last_tick.is_none_or(|last_tick| {
        now.unix_secs().saturating_sub(last_tick.unix_secs()) > crate::cmd::status::STALE_AFTER_SECS
    })
}

/// Fetches `snapshot.json` from the home and writes the local cache.
fn refresh_cache(control: &ControlHome) -> Result<CachedSnapshot, CliError> {
    let remote_path = format!("{}/snapshot.json", control.config.remote_home.trim_end_matches('/'));
    let command = format!("cat {}", shell_quote(&remote_path));
    let output = ProcessCommand::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &control.config.ssh_target,
            &command,
        ])
        .output()
        .map_err(|error| CliError::Remote(format!("ssh {}: {error}", control.config.ssh_target)))?;
    if !output.status.success() {
        return Err(CliError::Remote(format!(
            "ssh {} could not read {remote_path}",
            control.config.ssh_target
        )));
    }
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError::Remote(format!("invalid snapshot from the home: {error}")))?;
    let fetched_at = Timestamp::now();
    write_private(
        &control.root.join("snapshot.json"),
        &serde_json::to_vec_pretty(&snapshot).map_err(vowp_protocol::ProtocolError::from)?,
    )?;
    let meta = SnapshotMeta {
        fetched_at: Some(fetched_at),
        source: Some("remote".to_owned()),
    };
    write_private(
        &control.root.join("snapshot.meta.json"),
        &serde_json::to_vec_pretty(&meta).map_err(vowp_protocol::ProtocolError::from)?,
    )?;
    Ok(CachedSnapshot {
        snapshot,
        fetched_at,
        source: "remote".to_owned(),
    })
}

/// The on-disk `snapshot.meta.json` document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SnapshotMeta {
    /// When the snapshot was fetched.
    #[serde(default)]
    pub fetched_at: Option<Timestamp>,
    /// Where it came from (`remote`, `local` or `cache`).
    #[serde(default)]
    pub source: Option<String>,
}

/// Reads the control snapshot cache; a missing or malformed cache is `None`.
pub(crate) fn cached_snapshot(root: &Path) -> Result<Option<CachedSnapshot>, CliError> {
    let snapshot_path = root.join("snapshot.json");
    let Ok(text) = std::fs::read_to_string(&snapshot_path) else {
        return Ok(None);
    };
    let Ok(snapshot) = serde_json::from_str::<Snapshot>(&text) else {
        return Ok(None);
    };
    let meta = std::fs::read_to_string(root.join("snapshot.meta.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<SnapshotMeta>(&text).ok())
        .unwrap_or_default();
    Ok(Some(CachedSnapshot {
        fetched_at: meta.fetched_at.unwrap_or(snapshot.generated_at),
        source: meta.source.unwrap_or_else(|| "cache".to_owned()),
        snapshot,
    }))
}

/// Returns the control directory: `$VOWP_CONTROL_HOME`, else `$HOME/.vowp-control`.
pub(crate) fn control_root() -> Result<PathBuf, CliError> {
    if let Some(root) = std::env::var_os("VOWP_CONTROL_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CliError::NotFound("HOME is not set; set VOWP_CONTROL_HOME to a control directory".to_owned())
        })?;
    Ok(PathBuf::from(home).join(".vowp-control"))
}

/// Reads `control.json` from `root`, when it exists.
pub(crate) fn read_control_config(root: &Path) -> Result<Option<ControlConfig>, CliError> {
    let path = root.join("control.json");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let config: ControlConfig =
        serde_json::from_str(&text).map_err(|error| CliError::Invalid(format!("{}: {error}", path.display())))?;
    if config.role != "control" {
        return Err(CliError::Invalid(format!(
            "{} has role {:?}; expected \"control\"",
            path.display(),
            config.role
        )));
    }
    Ok(Some(config))
}

/// Reads the control machine's `nodes.json` registry mirror.
pub(crate) fn read_node_registry(root: &Path) -> Result<Vec<Node>, CliError> {
    let path = root.join("nodes.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(|error| CliError::Invalid(format!("{}: {error}", path.display())))
}

/// Replaces the control machine's `nodes.json` registry mirror.
pub(crate) fn write_node_registry(root: &Path, nodes: &[Node]) -> Result<(), CliError> {
    let text = serde_json::to_vec_pretty(nodes).map_err(vowp_protocol::ProtocolError::from)?;
    write_private(&root.join("nodes.json"), &text)
}

/// Writes `bytes` to `path` atomically with mode 0600, creating private parents.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    set_private_mode(&tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Creates `dir` and its parents, then makes the directory private (0700).
pub(crate) fn ensure_private_dir(dir: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(dir)?;
    set_private_mode(dir)
}

/// Sets mode 0600 on a file or 0700 on a directory.
fn set_private_mode(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Writes routing output that has no [`IoStreams`] (the control fallback).
///
/// This mirrors [`run_remote`], which already passes the remote process's stdio straight
/// through: the routing layer runs before [`crate::Ctx`] and therefore has no buffer.
fn write_stdout(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}
