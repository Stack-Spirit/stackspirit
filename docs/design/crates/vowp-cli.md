# crate `vowp-cli` (binary `vowp`)

Thin binary over `vowp-core`: argument parsing (clap derive), rendering (human + `--json`), exit codes, `--node` over ssh, signal handling.
All behaviour of commands is in `cli.md`; this file fixes the **module layout, ownership, shared API and tests**. `anyhow` is allowed only here
(`main.rs`); inner modules return `CliError`. Dependencies: `conventions.md` §3.

## 1. Module layout

The crate is `[[bin]] name = "vowp"`, with a tiny `src/lib.rs` exposing `pub fn run(args: Vec<String>, io: &mut dyn IoStreams) -> i32` so tests call commands in-process
(no subprocess needed except for the few `assert_cmd` smoke tests).

```
crates/vowp-cli/
  Cargo.toml
  src/main.rs         fn main() -> ExitCode { vowp_cli::run(std::env::args().collect(), &mut RealIo) }   (anyhow only here, for panics/IO setup)
  src/lib.rs          pub fn run, IoStreams trait (stdout/stderr/stdin writers+readers; tests use a buffer impl), re-exports
  src/cli.rs          clap derive structs for EVERY command in cli.md (global flags + subcommand enum), parse -> `Command`
  src/ctx.rs          Ctx { paths, deps: vowp_core::Deps, project: ProjectName, json: bool, color: bool }, Ctx::build(&Global) (loads vowp.toml, opens Deps with SystemRunner/SystemGit/Tmux)
  src/output.rs       Table (aligned columns), `Out::{human, json}`, time formatting, `json_error`
  src/errors.rs       CliError + exit-code mapping (cli.md §1.1)
  src/remote.rs       --node over ssh: build_ssh_argv, run_remote
  src/signals.rs      SIGTERM/SIGINT -> CancelToken with signal_hook::flag
  src/cmd/mod.rs      `pub mod` for all of the below and `pub fn dispatch(ctx, Command) -> Result<i32, CliError>`
  src/cmd/init.rs  doctor.rs  job.rs  graph.rs
  src/cmd/up.rs  status.rs  board.rs  attach.rs  plan.rs  window.rs   (`up`,`down`, `status`, `board`, `attach`, `plan`, hidden `__window`)
  src/cmd/session.rs  inbox.rs  cost.rs  node.rs  offload.rs  loops.rs  schedule.rs  maint.rs   (`maint` = reindex, backup, schema, version)
  src/cmd/promote.rs  (`promote`, `rollback`, `releases`)
  src/cmd/capacity.rs  (`capacity show|report|set|reload`; the report adds effective provider
                       caps and reload pending, and `reload` asks the supervisor or checks locally)
  src/cmd/agent.rs    (`agent enroll|pair|run|status` plus hidden `agent gateway`; no store or project context for the Mac commands)
  src/agent_exec.rs   (`CoreRunHandler`: the Mac assignment executor used by `agent run`)
  src/cmd/surface.rs  (`surface pair|ls|revoke`; home-only global-store surface pairing)
  src/framework.rs    embeds the templates from the repo `framework/` dir with include_str!/include_dir-free explicit list; `init` writes them
  tests/*.rs          tests for the commands
```
Every command module in `src/cmd/` is real code. A command that cannot run returns an explicit error (exit code 1, message `not implemented in this build`);
no command may ship as a stub in a final build: the acceptance test `no_command_returns_not_implemented` iterates every command in `cli.rs` with `--help`-free dry arguments and fails on `NotImplemented`.

## 2. Shared API (frozen)

```rust
pub trait IoStreams { fn out(&mut self) -> &mut dyn std::io::Write; fn err(&mut self) -> &mut dyn std::io::Write; fn input(&mut self) -> &mut dyn std::io::BufRead; fn is_tty(&self) -> bool; }

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")] Invalid(String), #[error("{0}")] NotFound(String), #[error("{0}")] Conflict(String),
    #[error("{0}")] Guard(String), #[error("{0}")] Limited(String), #[error("remote: {0}")] Remote(String),
    #[error("not implemented: {0}")] NotImplemented(&'static str),
    #[error(transparent)] Core(#[from] vowp_core::CoreError), #[error(transparent)] Store(#[from] vowp_store::StoreError),
    #[error(transparent)] Protocol(#[from] vowp_protocol::ProtocolError), #[error("io: {0}")] Io(#[from] std::io::Error),
}
impl CliError { pub fn kind(&self) -> vowp_protocol::ErrorKind; pub fn exit_code(&self) -> i32; pub fn hint(&self) -> Option<String>; }

pub struct Global { pub json: bool, pub node: Option<String>, pub project: Option<String>, pub home: Option<PathBuf>, pub no_color: bool, pub quiet: bool, pub verbose: u8 }
pub struct Ctx { /* see layout */ }
impl Ctx { pub fn build(g: &Global, io: &dyn IoStreams) -> Result<Ctx, CliError>; pub fn store(&self) -> Result<vowp_store::Store, CliError>; pub fn clock(&self) -> &dyn Clock; }

pub struct Table { /* header + rows */ }
impl Table { pub fn new(headers: &[&str]) -> Self; pub fn row(&mut self, cells: Vec<String>); pub fn render(&self, color: bool) -> String; }
pub fn print_json<T: serde::Serialize>(io: &mut dyn IoStreams, v: &T) -> Result<(), CliError>;       // pretty-printed, trailing newline
pub fn print_error(io: &mut dyn IoStreams, e: &CliError, json: bool);                                   // cli.md §1.1 shapes
pub fn local_time(ts: Timestamp, now: Timestamp) -> String;                                           // "HH:MM" today else "MM-DD HH:MM" (UTC offset from [node] tz_offset_minutes)

pub mod remote {
    pub fn build_ssh_argv(target: &str, args: &[String], interactive: bool) -> Vec<String>;             // pure; quoting via vowp_tmux::quote::shell_quote
    pub fn run_remote(target: &str, args: &[String], interactive: bool) -> Result<i32, CliError>;        // spawns ssh; 255 -> Remote error
}
```

## 3. Agent commands (`cmd/agent.rs`)

`vowp agent` is the Phase 0 Mac-side client from
[`vowp-agent.md`](vowp-agent.md) and `cli.md` §12. The Mac commands are handled before `Ctx::build`
and before the generic `--node` redirect: they use `~/.vowp-agent` (or `VOWP_AGENT_HOME`) and never
open a project store. `agent run` and `chat --agent` build adapters through
`vowp_core::AdapterRegistry`; neither opens a database. The hidden `agent gateway` command is the
home-side exception: it builds a `Ctx`, requires the home role and opens `Ctx::store()`.

- `agent enroll --target <ssh-target> --code-file <path|->` reads `-` from the `IoStreams` stdin,
  trims the line, validates the 52-character code shape and passes it only to
  `AgentClient::enroll`. The code never appears in argv, `ps`, output or a log. Success writes
  `agent.json` 0600 and prints the node, home and protocol; `--json` prints
  `{"node","home","protocol","heartbeat_secs"}`.
- `agent pair --code-file <path|-> --surface <id> --session <id> [--app <app>]` reads the code
  from the `IoStreams` stdin, calls `AgentClient::pair` and prints the surface id, scopes, expiry
  and token path. `--json` prints `{"surface_id","scopes","expires_at","token_path"}`. The token
  is written only to `~/.vowp-agent/surfaces/<id>/token` (0600) and is never printed. A bad or
  replayed code is a non-retryable refusal (exit 5).
- `agent run [--foreground] [--role surface|executor|both]` loads `agent.json` and `node.key`,
  builds the SSH transport, wraps a `CoreRunHandler` in an `Arc` and calls
  `AgentClient::run_with_handler`. `status.json` is rewritten on every heartbeat. The Phase 0
  role spellings are unchanged and always run in the foreground; `--foreground` is accepted for
  the documented surface. A revoked credential exits 5 and a superseded connection exits 4.
- `agent status [--json]` loads `status.json`; without `agent.json` it exits 3 with the enrollment
  command in the message. It fills missing fields from `agent.json` and `node.pub`, recounts
  `spool/`, and prints the human lines `connection`, `node`, `home`, `protocol`, `last heartbeat`,
  `pending spool`, `key`, optional `surface token expires` and optional `last error`.
- `agent gateway --stdio` (hidden) is the home side: it requires `--stdio`, builds a `Ctx`, refuses
  any role but home with exit 5, opens `Ctx::store()` and calls
  `vowp_core::agent::gateway::serve`. Stdout carries JSON frames only; errors go to stderr. Phase 0
  has no `--socket`.

`AgentStatus` is CLI-local (it is not an exported protocol schema), has no token or secret field,
and serialises as:

```json
{
  "connection": "connected",
  "node": "mac-studio",
  "home": "home-1",
  "protocol": "vowp-agent/1",
  "last_heartbeat": "2026-11-14T10:00:00.000Z",
  "last_error": null,
  "pending_spool_bytes": 42,
  "key_fingerprint": "sha256:0123456789abcdef",
  "surface_token_expires_at": null
}
```

### Assignment executor (`agent_exec.rs`)

`CoreRunHandler` is the [`vowp-agent`](vowp-agent.md) `RunHandler` behind `agent run`. It uses
the merged worktree, path-map and run-sink APIs; it does not change the `vowp-agent` trait and
does not open a store. One handler lives for one agent connection.

- **Accept.** The assignment's project must exist in the saved path map (`ProjectUnmapped`).
  `PathMap::new` canonicalises the local and worktree roots and checks they exist, are
  directories and carry a write bit (`PathMissing`/`PathNotWritable`/`NotADirectory`); the
  `cwd_ref` is resolved through the same map, so traversal, a symlink escape, a case-only alias
  and a subdirectory outside the allow list (`CwdOutsideAllow`) are refused. The checkout's
  `vowp.toml` supplies the harness registry; a missing or unprobeable harness is
  `HarnessMissing`. Every `env_names` and `secret_names` entry must resolve locally, otherwise
  the rejection is `EnvMissing` with the **name** in the detail. Free disk below the 5 GiB
  reserve is `DiskLow`, an active run for the same job or an existing unowned worktree is
  `WorktreeConflict`, and an invalid base commit/remote is `Other`. The handler reads
  `agent.json` on each accept, so `agent paths add|rm` is visible without a restart.
- **Worktree.** `run` calls `WorktreeManager::create` with the home-supplied base commit, the
  assignment's remote and the reserve, producing `<worktree_root>/<job>` on `job/<id>`. A
  clean, owned worktree at exactly the base is reused; a stale owned one is recreated; an
  unowned directory is refused. The main checkout is never checked out, reset or cleaned.
- **Adapter and events.** The adapter starts in the worktree with the assignment prompt and the
  resolved environment. `adapter_for` is shared with `chat --agent`, so the `vowp.toml` harness
  configuration is read in one place. Events are converted to `RemoteEvent`s with fresh stable
  ids and streamed through the injected `RunSink`; the adapter's terminal `turn.ended` is
  skipped because the terminal `TurnSummary` is canonical. A quiet adapter gets a liveness
  heartbeat every ten seconds. The loop stops the adapter when the sink is stopped, when cancel
  or a fence arrives, or when the sink reports the spool cap; the summary records `cancelled`,
  `fenced` or `spool_full` in its result text and uses `Killed`/`Interrupted` or `Error`/`Error`
  as appropriate.
- **Env, secrets and redaction.** Only names cross the wire. Values resolve from this machine's
  environment, are passed to the adapter, and are replaced with `[REDACTED:<NAME>]` in every
  event payload, raw line, limit message and result text before the sink sees them. The known
  worktree and project roots are replaced with `worktree:<project>/<job>` and
  `project:<project>`; the agent state directory becomes `<agent-home>`. Rejection details,
  push failures and local error summaries carry no absolute path and no value.
- **Result return and cleanup.** After the adapter exits the summary carries the exit code,
  outcome, native id, cost, head commit and `PushStatus`. A non-empty `remote_url` causes
  `Worktree::push_job(remote_url)`: only `refs/heads/job/<id>` is pushed, the push URL must
  match the home project after credentials are stripped, no tag or default-branch push is
  attempted, and a mismatch or git failure is `PushStatus::Failed` with the worktree kept for
  inspection. An empty `remote_url` is `PushStatus::NotRequested`. `Cleanup` removes only the
  job worktree and `job/<id>` branch through `WorktreeManager::cleanup`; it never touches the
  main checkout or a directory VOWP did not create, and it waits briefly for a cancelled run to
  leave the worktree.

`agent status` is unchanged. The `status.json` snapshot carries the connection fields and no
path map, secret name or secret value. The human view keeps its local-only `paths` block, and
`--json` includes the same `paths`, exactly as before.

### Surface commands (`cmd/surface.rs`)

`vowp surface` is handled before the generic `--node` redirect and the normal command dispatch,
because its `--node` flag names the surface's node rather than an SSH target. The
commands are home-only (a control machine exits 5) and open the global store through
`Ctx::fleet_store()`, so no project selection is required.

- `surface pair --session <id> --node <name> --kind opencode|chat [--app <app>]` calls
  `vowp_core::agent::pair::create_pending_surface`: it validates the node, session and app,
  inserts a `pending` surface bound to the session, and inserts a ten-minute `surface`
  enrollment code whose plaintext is shown once. The command prints the code, the expiry and
  the exact Mac command `vowp agent pair --code-file - --surface <id> --session <id>
  [--app <app>]`; the code is never written to a file, the database or stderr. `--json` prints
  `{"surface","node","session","kind","app","code","expires_at","command"}`. A missing
  `--node`/`--session`, an unknown kind or an invalid id is exit 2; an unknown node or session
  is exit 3; a duplicate pending surface for the same node/session/kind/app is exit 4; a
  revoked node is exit 5 through `CoreError`.
- `surface ls [--node <name>]` lists the surface ids this CLI created, reading each surface's
  current state from the store and each token's scopes/expiry from `surface_tokens`. Human
  output is `ID NODE KIND STATE SCOPES EXPIRES`; `--json` prints an array of
  `{"id","node","kind","state","scopes","expires_at"}`. It never prints a token or a token
  hash. The id index is `<home>/surfaces.json` (0600): surface ids are not secrets, and the
  v4 store read model has no list-all query, so the CLI records the ids it created.
- `surface revoke <id> [--yes]` calls `vowp_core::agent::pair::revoke_surface`, which revokes
  every active token and marks the surface `revoked`. Without `--yes` it refuses with exit 5;
  an unknown surface is exit 3. `--json` prints
  `{"surface","status":"revoked","tokens_revoked":N}`. No token is printed.

### Node enrollment commands (`cmd/node.rs`)

These are home-only (exit 5 on a control machine) and require the home store:

- `node enroll <name> [--label <label>]...` calls `vowp_core::agent::enroll::create_pending`, which
  inserts an `agent`-kind `pending` node and a ten-minute code. It prints the 52-character code once
  and the `vowp agent enroll --target <home-ssh-target> --code-file -` command; `--json` prints
  `{"node","code","expires_at","command"}`. The code is never written to a file or the log. An
  existing node is exit 4. `--label` is repeatable.
- `node revoke <name> [--yes]` calls `enroll::revoke`, which marks the node `revoked` and revokes
  every credential. Without `--yes` it refuses (exit 5). `--json` prints
  `{"node","status":"revoked","credentials_revoked":N}`. An unknown node is exit 3. Phase 0 has no
  live connection registry; the gateway sends `Revoked` on the node's next frame.
- `node show <name> [--json]` returns the redacted `NodeCard` plus `last_seen_at` from the node row
  and never a path map, absolute local path or secret name. Human output prints status, presence,
  labels, capacity, mapped projects, agent version, protocol, last seen and last error. `node ls`
  and its JSON `Node` shape are unchanged.

### Offload commands (`cmd/offload.rs`)

The read-only v0.4 Phase 2 commands from `cli.md` §14. Neither sends `AssignRun`, creates a
worktree or runs an agent.

- `offload policy [--project <p>] [--json]` uses `ctx.config().offload`; it does not open a store
  for the document itself. It fills defaults through `OffloadConfig::project_policy` and
  `node_policy`, adds the selected project and registered node cards, and prints one
  `ProjectPolicyRow` per project and one `NodePolicyRow` per node. `--project` is the global
  selector: clap records whether that flag was passed, and an explicit selection restricts the
  project rows to that project, including when the selected root is the cwd. The human view is a
  scalar block followed by two `Table`s; `--json` is one `EffectivePolicy` object.
- `offload status [--json]` opens `Ctx::fleet_store`, reads node cards, live assignments
  (`queued|sent|accepted|running|disconnected`) and per-node limits, then opens every registered
  project read-only in turn. For each ready job whose effective placement (job row, else project
  default, else `offload.default`) is not `home` it builds `vowp_core::offload::NodeFacts` and
  calls `evaluate` with the project's effective policy. The human view is the
  `nodes`/`assignments`/`ready` tables; `--json` is `OffloadStatusOutput`.
  `AssignmentRow` deliberately omits `AssignmentPayload` (its prompt is not part of this view), and
  no row carries an absolute local path.
- `job add --placement` parses `Placement`, refuses `node:<name>` with exit 5 when
  `offload.enabled = false` before inserting anything, writes the `job_placement` row immediately
  after the job row, and deletes the job if either the placement or brief write fails. `job show`
  adds the placement to human output and to its JSON object; `job ls` is unchanged.

### Local chat (`chat --agent`)

`vowp chat --agent --session <id> [--executor <exec>] [--node <name>]` runs the Phase 1 local
surface. The positional id stays required for the home spelling; `Cli::try_parse_from` in
`src/cli.rs` inserts the `--session` value before clap parses an agent command, so `vowp chat`
without an id still fails with clap's missing-argument error and the help tree keeps `<ID>`. The
parser records `--agent`, and `lib.rs` dispatches it to `src/chat/agent.rs` before the
control-machine route and before `Ctx::build`. The Mac therefore needs no local home database and
the command is never forwarded to the home. The bridge resolves its checkout from `--project
<dir>` or the nearest `vowp.toml`; it does not open the node store.

The handler loads `~/.vowp-agent` (or `VOWP_AGENT_HOME`), the node key and the surface token
bound to the session, then calls `vowp_agent::ChatClient::turn`. The local checkout's `vowp.toml`
supplies the project name, the working directory and the default executor (the first rung of the
`worker` profile; `--executor` overrides it). The agent acquires
the lease, sends `TurnBegin` with a project-relative `PathRef`, runs the adapter on this machine,
spools each event before sending it and prints assistant text, tools, errors and notes in the
home chat's 80-column format. The home log is authoritative; the CLI never opens a second session
log or takes the home lease. Ctrl-C (through `SignalWatcher`) stops the adapter and releases the
lease; a takeover or lease loss prints `This surface is read-only: <reason>` and exits 0; a full
spool exits 7 with the cap in the message. `--follow` and `--takeover` are refused in agent mode.

## 4. Rules

1. Every command handler is `fn run(ctx: &Ctx, io: &mut dyn IoStreams, args: &XArgs) -> Result<i32, CliError>`; no handler prints directly to `std::io`.
2. `--json` output types are the protocol types (types.md) wherever one exists (`Job`, `Session`, `InboxItem`, …); only summaries (`status`, `doctor`, `cost`) have CLI-local `Serialize` structs, defined next to the command.
3. Commands that mutate job state use `vowp_core::requests`: if a supervisor holds the lock they `push` a request and poll the job row ≤ 5 s; else `apply_direct`.
4. `Ctx::build` never creates `~/.vowp` implicitly except for `init`; other commands fail with `NotFound` ("run `vowp init`").
5. Interactive commands (`chat`, `attach`, `board`) check `is_tty()`; without a TTY they degrade as stated in `cli.md`.
6. Secrets: `doctor` reports env var **names** present/absent only.
7. `vowp --help` text for each command is the first line of its section in `cli.md`.

## 5. Named tests

Listed in `cli.md` §8, grouped by the file table above. Additional: `help_lists_every_v0_1_command` (compares clap's subcommand set with a constant list copied from `cli.md`),
`no_command_returns_not_implemented`, `json_flag_outputs_valid_json_for_every_listing_command`, `exit_codes_follow_error_kind`, `ctx_build_requires_init`, `io_streams_buffer_impl_captures_output`,
`capacity_reload_check_reports_without_writing`, `capacity_reload_refuses_when_no_supervisor_runs`;
the agent slice adds `agent_status_without_enrollment_exits_three`, `agent_status_prints_json`,
`agent_enroll_help_names_code_file` and `agent_enroll_accepts_code_file_stdin`; the node
enrollment slice adds `node_enroll_creates_a_pending_node_and_prints_the_code_once`,
`node_enroll_refuses_a_duplicate_node`, `node_revoke_marks_the_node_and_revokes_credentials`,
`node_show_returns_the_redacted_card` and `node_enroll_is_home_only` in `tests/agent.rs`. The
surface-pairing slice adds `agent_pair_help_names_code_surface_and_session`,
`agent_pair_accepts_code_file_stdin`,
`agent_pair_without_enrollment_exits_three_and_prints_no_code`,
`surface_pair_cli_prints_code_once_and_lists_pending`,
`surface_revoke_marks_the_token_revoked`, `surface_command_requires_a_node_and_session`,
`surface_ls_never_prints_a_token` and `surface_commands_are_home_only` in `tests/agent.rs`,
plus `surface_commands_are_registered_and_node_is_a_filter` in `tests/core.rs`. The
offload-policy slice adds `job_add_refuses_node_placement_when_offload_is_disabled`,
`job_add_persists_any_placement_and_job_show_reads_it`,
`offload_policy_json_is_the_effective_policy` and
`offload_status_lists_nodes_assignments_and_waiting_reasons` in `tests/offload.rs`. The
assignment-executor slice adds `agent_run_creates_a_worktree_and_streams_adapter_events`,
`agent_run_pushes_only_the_job_branch_and_reports_the_head`,
`agent_run_refuses_a_missing_env_name_and_redacts_injected_values`,
`agent_run_keeps_the_worktree_on_push_failure`,
`agent_run_cleans_up_after_a_terminal_summary`, `agent_run_cancel_stops_the_adapter`,
`agent_run_does_not_touch_the_main_checkout` and `agent_run_rejects_an_unmapped_project` in
`tests/agent.rs`. They use a fake adapter script, a fake in-process gateway, real git in
temporary repositories and a temporary `VOWP_AGENT_HOME`.

## 6. Out of scope
Shell completions (v0.2), TUI board beyond redraw loop, windows (OS) support.
