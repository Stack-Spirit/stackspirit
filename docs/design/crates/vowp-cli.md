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
  src/cmd/session.rs  inbox.rs  cost.rs  node.rs  loops.rs  schedule.rs  maint.rs   (`maint` = reindex, backup, schema, version)
  src/cmd/promote.rs  (`promote`, `rollback`, `releases`)
  src/cmd/capacity.rs  (`capacity show|report|set|reload`; the report adds effective provider
                       caps and reload pending, and `reload` asks the supervisor or checks locally)
  src/cmd/agent.rs    (`agent enroll|pair|run|status` plus hidden `agent gateway`; no store, project or `vowp-core` for the Mac commands)
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
open a project store or `vowp-core`. The hidden `agent gateway` command is the home-side exception:
it builds a `Ctx`, requires the home role and opens `Ctx::store()`.

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
  builds the SSH transport, and calls `AgentClient::run`. `status.json` is rewritten on every
  heartbeat. Phase 0 always runs in the foreground; `--foreground` is accepted for the documented
  surface. A revoked credential exits 5 and a superseded connection exits 4.
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

### Local chat (`chat --agent`)

`vowp chat --agent --session <id> [--executor <exec>] [--node <name>]` runs the Phase 1 local
surface. The positional id stays required for the home spelling; `lib::run` inserts the
`--session` value before clap parses an agent command, so `vowp chat` without an id still fails
with clap's missing-argument error and the help tree keeps `<ID>`. Agent mode is dispatched from
`lib::execute` to `src/chat/agent.rs` before the control-machine redirect and before `Ctx::build`.

The handler loads `~/.vowp-agent` (or `VOWP_AGENT_HOME`), the node key and the surface token
bound to the session, then calls `vowp_agent::ChatClient::turn`. It opens no VOWP database: the
local checkout's `vowp.toml` supplies the project name, the working directory and the default
executor (the first rung of the `worker` profile; `--executor` overrides it). The agent acquires
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
plus `surface_commands_are_registered_and_node_is_a_filter` in `tests/core.rs`.

## 6. Out of scope
Shell completions (v0.2), TUI board beyond redraw loop, windows (OS) support.
