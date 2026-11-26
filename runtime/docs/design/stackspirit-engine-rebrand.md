# StackSpirit Engine — rebrand and zero-downtime migration plan

Status: **design; v6-2 through v6-7 implemented in the tree.** This is the v6-1 deliverable; the operator-facing result is the [migration guide](../guides/stackspirit-migration.md) and the [deprecation policy](../deprecation-policy.md). It names
the target identity, fixes the compatibility contract, gives the exact deployment order for a live
home, a Mac agent node and StackSpirit Desktop, and breaks the work into six independently
mergeable jobs (`v6-2` through `v6-7`).

The programme uses `v6-*` as job ids, not as product versions. The workspace currently reports
`0.5.1-dev`; the rebrand does not by itself change that version.

## 1. Decisions in one table

| Question | Decision | Why |
|---|---|---|
| Primary CLI name | `stackspirit` | Explicit product name in the StackSpirit namespace; `spirit` is too generic and too easy to confuse with unrelated tools. |
| Compatibility CLI | `vowp` stays installed and behaves identically | Stored orchestrator configs, agent transport commands and the public VOWP release all invoke `vowp`. |
| Daemon binary | `stackspirit-daemon` plus `vowp-daemon` | `vowp up` discovers the daemon by sibling name and `VOWP_DAEMON_BIN`; the old name must keep working. |
| Project config | Read `stackspirit.toml`, fall back to `vowp.toml` | Existing repositories are untouched; new private repositories get the new name. |
| Node-level config | `config.toml` unchanged | It carries no product name; changing it adds migration risk without brand value. |
| Home | Resolve `~/.stackspirit`, fall back to `~/.vowp`; never auto-move data | A live home is a database, session logs and a running supervisor, not a directory to rename on startup. |
| Agent state | Resolve `~/.stackspirit-agent`, fall back to `~/.vowp-agent` | Same rule as the home; the node key and surface tokens stay where they are. |
| Control state | Resolve `~/.stackspirit-control`, fall back to `~/.vowp-control` | A control machine caches the remote home path in `control.json`; the old path must keep working. |
| Environment | Read `STACKSPIRIT_*` first, then `VOWP_*`; export both to children | A mixed-version fleet needs one process tree to be readable by old and new binaries. |
| MCP server | `serverInfo.name` becomes `stackspirit-engine`; the 15 tool names stay frozen | Clients namespace tools by their configured server key, not by `serverInfo`; tool names are a user-visible API. |
| MCP config key | New orchestrators use `stackspirit`; existing files keep their present key | Changing an existing key silently breaks saved tool permissions and prompts. |
| systemd/launchd | New hosts get `stackspirit.service` / `dev.stackspirit.agent`; existing units keep their name and are updated in place | Two enabled units for one supervisor or one agent is an outage, not a migration. |
| Crates | Keep `vowp-*` package names and `vowp_*` library names | They are internal, published on crates.io, and renaming them adds workspace churn without a user-facing gain. |
| On-disk data names | Keep `vowp.db`, `project.db`, `.vowp/`, tmux `vowp-*` names and the existing wire format strings | These are data and protocol identifiers, not marketing surfaces. Renaming them is a v7 decision with a real migration. |
| Public VOWP | `syver777/vowp` remains the VOWP-branded open-source release | The private engine is a downstream product; no private branding is pushed to the public tree by this plan. |

## 2. Current state this plan has to preserve

The plan starts from the tree, not from a blank product. These are the exact touch points that make
a plain `sed` rename unsafe:

| Surface | Current implementation | What breaks if it is renamed blindly |
|---|---|---|
| CLI binary | Root package `vowp-cli`, `[[bin]] name = "vowp"`, clap name `vowp` in `crates/vowp-cli/src/cli.rs` | Every stored command, shell script, systemd unit and release smoke test. |
| Daemon | `crates/vowp-daemon` with `[[bin]] name = "vowp-daemon"`; the supervisor parses `vowp-daemon: listening on …` in `crates/vowp-cli/src/cmd/up.rs` | A new startup line silently disables the web window for an older supervisor. |
| Home resolution | `NodePaths::from_env` reads `VOWP_HOME`, else `$HOME/.vowp` (`crates/vowp-store/src/paths.rs`) | A new default moves a live home and strands `~/.vowp` data. |
| Project config | `vowp.toml` is joined directly in `crates/vowp-cli/src/ctx.rs`, `crates/vowp-core/src/capacity/reload.rs`, `crates/vowp-core/src/loops/supervisor.rs`, `crates/vowp-core/src/surface/mcp.rs`, `crates/vowp-cli/src/mcp/backend.rs` and several commands | Partial fallback gives two configs to two subsystems. |
| Agent state | `AgentConfig::default_state_dir` reads `VOWP_AGENT_HOME`, else `$HOME/.vowp-agent` (`crates/vowp-agent/src/config.rs`) | A moved state directory loses the node key and paired surface tokens. |
| Control state | `~/.vowp-control`, `VOWP_CONTROL_HOME`; `control.json.remote_home` stores the home path (`crates/vowp-cli/src/node_ctx.rs`, `crates/vowp-cli/src/remote.rs`) | A control machine that cannot find its cache cannot route commands. |
| MCP server | `serverInfo.name` is `vowp` in `crates/vowp-cli/src/mcp/mod.rs`; orchestrator files register `mcp.vowp` / `mcpServers.vowp` with the command `vowp mcp` | Renaming the config key changes `mcp__vowp__*` tool names and invalidates saved permissions. |
| MCP tools | 15 names: `fleet_status`, `fleet_queue`, `inbox_list`, `capacity_report`, `session_tail`, `inbox_answer`, `job_add`, `job_pause`, `job_retry`, `capacity_set`, `tunnel_open`, `tunnel_close`, `secret_request`, `session_handoff`, `session_resume` (`crates/vowp-cli/src/mcp/tools.rs`) | A tool rename is a breaking API change for every prompt and permission file. |
| Agent transport | Agent SSH forced command is `vowp agent gateway --stdio` (`crates/vowp-agent/src/transport.rs`); `--node` re-executes remote `vowp` (`crates/vowp-cli/src/remote.rs`); setup installs and checks `~/.local/bin/vowp` | A host with only `stackspirit` cannot be reached by an existing agent or control machine. |
| systemd | `scripts/vowp.service` and its embedded copy `crates/vowp-cli/src/setup/vowp.service`: `ExecStart=%h/.local/bin/vowp up --foreground`, `Environment=VOWP_HOME=%h/.vowp` | Replacing the unit under a different name can leave the old enabled unit running too. |
| Agent service | `dev.vowp.agent` / `dev.vowp.agent.plist`; `vowp-agent.service`; `VOWP_AGENT_HOME` (`crates/vowp-agent/src/service.rs`) | A renamed unit plus an old enabled unit means two agents with one node key. |
| Data | `vowp.db`, `projects/<name>/project.db`, session logs, `.vowp/jobs`, `.vowp/checkpoints` | The DB and job contract are on disk; a rename is a migration, not branding. |
| Wire formats | `vowp-agent/1`, `vowp-snapshot/1`, `vowp-bundle-v1`, `/api/v1/*`, `.vowp-bundle.json` | StackSpirit Desktop, agents and the public VOWP release all depend on these strings. |
| tmux | Socket `vowp-<fnv1a(canonical home)>`, sessions `vowp-home` and `vowp-<project>`, `VOWP_TMUX_SOCKET` | Changing the socket strands every existing window and makes `attach` look at an empty server. |
| Packaging | `scripts/install.sh` installs `~/.local/bin/vowp`; release archives are `vowp-<version>-<target>.tar.gz`; npm package `vowp`; Homebrew formula `Vowp`; version test expects `vowp <version>` | Release and install paths are user entry points; changing them without a compatibility alias breaks self-updates and scripts. |

## 3. Target identity and naming

### 3.1 Product and binaries

The product is **StackSpirit Engine**, part of the StackSpirit platform. The engine repository is
`Stack-Spirit/stackspirit-engine` (private). The desktop, web and UI repository is
`Stack-Spirit/stackspirit` (private). The public `syver777/vowp` repository remains the VOWP
open-source release.

`stackspirit` is the primary command. `vowp` remains a first-class compatibility command for the
whole v6 series. The two binaries must be built from the same entry point and must accept exactly
the same arguments, exit codes and environment variables. The only intended difference is the
name, version line and help banner:

```text
stackspirit --version        stackspirit 0.5.1-dev
vowp --version               vowp 0.5.1-dev
stackspirit --help           StackSpirit Engine: local-first supervisor for AI coding agents
vowp --help                  VOWP: local-first supervisor for AI coding agents
```

Implementation rule: `crates/vowp-cli/src/main.rs` is the `stackspirit` entry point and a thin
`crates/vowp-cli/src/bin/vowp.rs` wrapper passes a legacy product identity to the same `run`
function. The version output shape for `vowp` must not change; the existing
`scripts/release/version-test.rs` continues to assert it. A new test asserts the `stackspirit`
shape and asserts that both command trees are identical.

`spirit` is not a binary name. If a short alias is wanted later, it is a separate decision and a
separate alias; it is not part of v6.

The daemon follows the same pattern: `stackspirit-daemon` is primary and `vowp-daemon` remains a
thin compatibility binary. The daemon's first stdout line is a wire contract, not branding. In v6
both binaries keep printing exactly:

```text
vowp-daemon: listening on http://127.0.0.1:<port>
```

A new supervisor accepts either `vowp-daemon:` or `stackspirit-daemon:` in that line, so it can
start either daemon during a mixed-version rollout. An old supervisor only understands the `vowp-`
form, which is why the new daemon keeps it.

### 3.2 Project configuration

The project file becomes `stackspirit.toml`. Resolution is a single helper used by every
subsystem, not a per-command convention:

1. If `<root>/stackspirit.toml` exists, use it.
2. Else if `<root>/vowp.toml` exists, use it.
3. If both exist and resolve to different files, fail with a configuration conflict that names
   both paths. If both names point at the same file (a deliberate symlink bridge), accept it.
4. If neither exists, fail with the same "no project config; run `stackspirit init`"
   (`vowp init` also works) error the tree uses today.

Rules that make the fallback safe:

- The schema is unchanged. `VowpConfig` and its serde/TOML validation are reused; the type name is
  internal and is not renamed in v6.
- Existing repositories keep `vowp.toml`. Nothing rewrites, moves or deletes it.
- `init` writes `stackspirit.toml` when neither file exists. On an existing checkout it updates
  the file that is already there, so it never creates two configs.
- `--force` follows the same rule.
- `--project <path>`, `project add`, `doctor`, `orchestrator`, `agent`, `offload`, `surface`,
  capacity reload, the supervisor and MCP all use the helper.
- Error text and `doctor` name the file that was actually used. A legacy file is reported as
  `config: vowp.toml (legacy name)`; it is not an error.
- The home's reserved `_node` project uses the same helper. A new home gets
  `~/.stackspirit/stackspirit.toml`; an existing home keeps `~/.vowp/vowp.toml`.

This is a one-way compatibility for **new** repositories: an old `vowp` binary cannot read a
repository that only has `stackspirit.toml`. Existing repositories and the live home never become
that shape during v6, so the zero-downtime path is unaffected. A repository that must stay usable
by an old binary should keep `vowp.toml`.

The node-level `config.toml` keeps its name. It lives inside the selected home, so the home
directory rename is enough.

### 3.3 Home, agent and control directories

The resolver is deliberately boring and never writes during discovery. It selects one path; it
does not copy, move, create or delete anything.

For the node home:

1. `--home <DIR>` wins and is used as given.
2. `STACKSPIRIT_HOME` wins when set and non-empty.
3. `VOWP_HOME` is used when set and non-empty.
4. Candidates are `$HOME/.stackspirit` and `$HOME/.vowp`.
   - A candidate is a real home when it contains `vowp.db` (the database filename is unchanged).
   - Resolve existing candidates through `canonicalize`; if both names resolve to the same
     directory, they are one home and the `~/.stackspirit` spelling is preferred for display.
   - If both are distinct real homes, fail with `Conflict` and tell the owner to choose one with
     `--home` or to remove the stray copy. Never pick one silently.
   - If only one is a real home, use it.
   - If neither is a real home, use `$HOME/.stackspirit`; `init` creates it there.
5. `$HOME` must be set when no explicit path was supplied.

Agent state resolves the same way against `$HOME/.stackspirit-agent` and `$HOME/.vowp-agent`, with
`agent.json` as the marker. Control state resolves against `$HOME/.stackspirit-control` and
`$HOME/.vowp-control`, with `control.json` as the marker. `STACKSPIRIT_AGENT_HOME`,
`STACKSPIRIT_CONTROL_HOME` and their `VOWP_*` counterparts are the explicit overrides.

The preferred migration is a **link bridge**, not a data move:

- On an existing home, create `~/.stackspirit` as a symlink to `~/.vowp`. Both names now resolve
  to the same canonical directory. The old binary keeps using `~/.vowp`; the new binary can use
  the StackSpirit name; there is no copy, no schema change and no supervisor restart.
- On an existing agent node, link `~/.stackspirit-agent` to `~/.vowp-agent` the same way.
- On an existing control machine, link `~/.stackspirit-control` to `~/.vowp-control`.
- On a fresh host, create the new real directory and create the old name as a symlink to it. That
  keeps old binaries, old units and old scripts working with no data duplication.
- The link bridge is optional for resolving data, because the resolver already falls back. It is
  useful because it makes the new name true without a move, and because a symlink keeps the
  canonical path stable for tmux.

A real move is a separate, offline operation. It is not part of the normal upgrade. The planned
`stackspirit home migrate` command has two modes:

```text
stackspirit home migrate --link --dry-run          # show the symlink plan; write nothing
stackspirit home migrate --link --yes              # create the compatibility symlinks
stackspirit home migrate --move --to DIR --yes     # offline copy/rename; requires a stopped node
```

`--move` refuses while `supervisor.lock` names a live process, while the agent socket is live, or
while an orchestrator host is running. It copies with metadata, verifies the copy by opening it
read-only and running the same checks as `doctor`, then swaps only after the copy is complete. It
leaves a legacy symlink by default. On an agent, the command is limited to the agent state; on a
control machine it is limited to `control.json` and the cache. A move that changes the canonical
path changes the tmux socket hash; the command prints the old and new socket names and the exact
`VOWP_TMUX_SOCKET` / `STACKSPIRIT_TMUX_SOCKET` line to keep the old view until the owner is ready
to recreate it. tmux windows are views, not workers, so no run is lost if they are recreated; the
warning exists so `attach` does not look empty by surprise.

The `--move` mode is maintenance-window work, not part of the zero-downtime deployment.

### 3.4 Environment variables

The new names are read first and the old names remain supported. The set that matters to users is:

| New | Legacy | Used by |
|---|---|---|
| `STACKSPIRIT_HOME` | `VOWP_HOME` | CLI, supervisor, daemon |
| `STACKSPIRIT_AGENT_HOME` | `VOWP_AGENT_HOME` | agent commands, agent service |
| `STACKSPIRIT_CONTROL_HOME` | `VOWP_CONTROL_HOME` | control machine |
| `STACKSPIRIT_API_TOKEN` | `VOWP_API_TOKEN` | daemon writes |
| `STACKSPIRIT_DAEMON_BIN` | `VOWP_DAEMON_BIN` | supervisor daemon discovery |
| `STACKSPIRIT_TMUX_SOCKET` | `VOWP_TMUX_SOCKET` | private tmux scope, scripts and tests |

Rules:

- `--home` beats both home variables. An explicit variable is authoritative: if
  `STACKSPIRIT_HOME` points at a path that does not exist, the command reports that path; it does
  not silently fall back to `VOWP_HOME` or `~/.vowp`.
- If both names are set to different paths, the new name wins and one warning names both
  variables. If they resolve to the same path, there is no warning.
- A supervisor and agent always set **both** names in every child environment, with the same
  resolved value. Old binaries in the tree see `VOWP_*`; new binaries see `STACKSPIRIT_*` first.
  This is what makes a mixed-version fleet safe.
- Units generated after v6-5 also set both names.
- Internal-only variables stay on their current `VOWP_*` spelling: `VOWP_LEASE_TOKEN`,
  `VOWP_ORCHESTRATOR_APP`, `VOWP_GATEWAY_BIN`, `VOWP_EXEC_BIN`, `VOWP_VAULT_*`, and test-only
  variables. They are passed between processes of the same tree and are not user configuration;
  adding a second spelling would create two sources of truth.

### 3.5 Daemon, systemd and launchd

The daemon keeps the existing HTTP contract:

- `/api/v1/*` route names, status codes, JSON shapes, SSE streams and the token-free read rule are
  unchanged.
- `VOWP_API_TOKEN` / `STACKSPIRIT_API_TOKEN` keep the same write-enable meaning.
- The `web-window` file and its `vowp-daemon: listening on …` contract are unchanged as described
  in §3.1.
- `--home`, `--project`, `--port`, default ephemeral port and 127.0.0.1 binding are unchanged.

The home systemd unit is selected by existence, never by writing both:

- An existing `~/.config/systemd/user/vowp.service` is updated in place. Its name, enablement and
  running process are not duplicated. This is the only safe path on the live VPS.
- A fresh host gets `~/.config/systemd/user/stackspirit.service`.
- A host with neither unit gets the new unit.
- A host with both files is a hard error; the installer tells the owner which one is enabled and
  asks for an explicit `--rename-unit`, which stops and disables the old unit before writing and
  enabling the new one. The upgrade path never renames a unit automatically.
- The unit content is the same contract with the selected binary, the selected home and both
  `STACKSPIRIT_HOME` and `VOWP_HOME` environment lines. The existing
  `scripts/vowp.service` / embedded-copy equality test becomes a two-template equality test.

The agent service follows the same one-service rule:

- Existing macOS label `dev.vowp.agent` and Linux unit `vowp-agent.service` are updated in place.
- Fresh installs use `dev.stackspirit.agent` and `stackspirit-agent.service`.
- `agent service install` refuses to create a second unit while a generated unit exists. A
  `--rename-unit` flag performs the stop/disable/write/enable sequence explicitly; it is not the
  default.
- The plist/systemd unit carries both `STACKSPIRIT_AGENT_HOME` and `VOWP_AGENT_HOME`.
- The generated-file marker and the "never remove a foreign unit" rule remain unchanged.

The watchdog keeps a single cron entry. The legacy `vowp-watchdog.sh` path is retained on hosts
that already have it; a fresh host gets `stackspirit-watchdog.sh`. The installer replaces its own
cron marker rather than adding a second line, and the script exports both home variable names.

### 3.6 MCP server and tool namespace

MCP has three separate names that must not be conflated:

1. **Server self-description.** `serverInfo.name` changes to `stackspirit-engine` (and the
   `server/discover` probe does the same). This is display metadata; clients do not use it to
   address tools.
2. **The client's server key.** In an opencode file this is `mcp.<key>`; in a Claude file it is
   `mcpServers.<key>`. Clients namespace tool names with this key (`mcp__vowp__fleet_status`,
   `mcp__stackspirit__fleet_status`). This is the value that can invalidate permissions.
3. **The tool names.** The 15 names in §2 are frozen for v6. They do not gain a `vowp_` or
   `stackspirit_` prefix.

The migration is therefore:

- Existing orchestrator directories keep their current key. `orchestrator resume` detects an
  existing `mcp.vowp` or `mcpServers.vowp` entry and preserves it; it never rewrites a working
  config to a new key by itself.
- New orchestrators created after v6-4 use the key `stackspirit` in both app formats.
- The command inside the generated config remains `vowp mcp` for v6. Stored configs, rollback and
  the agent proxy already depend on the `vowp` alias; the key and the command are independent.
- A future `stackspirit-mcp` standalone server can use the `stackspirit` key and the same tool
  names. It is not part of v6.
- `vowp mcp` and `stackspirit mcp` serve the same JSON-RPC methods, tool list, lease checks and
  proxy path. `serverInfo.name` is the only changed value.
- External clients that manually registered `vowp mcp` keep working through the alias. They are
  not forced to rename their key.
- If an owner wants an old orchestrator moved to the new key, that is an explicit action: stop
  the host, update the app config or recreate the orchestrator, then re-approve permissions.
  v6 does not do it silently.

The `clientInfo` name sent by the CLI-side MCP proxy changes from `vowp-control` to
`stackspirit-control`; it is not used for routing.

### 3.7 Crates, versions and internal names

The workspace keeps:

- package names `vowp-protocol`, `vowp-store`, `vowp-graph`, `vowp-tmux`, `vowp-adapters`,
  `vowp-agent`, `vowp-core`, `vowp-daemon`, `vowp-cli`;
- library names `vowp_protocol`, `vowp_store`, and so on;
- the existing module and type names.

The reason is not nostalgia. These crates are published on crates.io, the public VOWP repository
builds the same workspace, and every rename would touch the lockfile, release scripts and public
lineage without changing what an owner sees. Branding belongs in the product shell: binary names,
help text, service descriptions, documentation and packaging.

A small `vowp_protocol::brand` module is the single source for the product name, binary names,
config names, directory names, environment names and MCP identity. The CLI and daemon re-export or
use those constants; no new global string table is scattered through the tree. `Product` is a
two-value enum (`StackSpirit`, `Vowp`) used for display names only; it does not change behavior.

The rebrand makes no schema change: `GLOBAL_SCHEMA_VERSION` stays 8 and
`PROJECT_SCHEMA_VERSION` stays 2. It makes no protocol change: `PROTOCOL_VERSION` stays 1 and the
`vowp-agent/1`, `vowp-snapshot/1` and `vowp-bundle-v1` markers stay as they are.

Version policy:

- The rebrand does not invent a new semantic version. The workspace version remains the source of
  truth for `--version` and release artifacts.
- A private StackSpirit release must be tagged in the private repository; it must not reuse or
  move a public `syver777/vowp` tag.
- The public VOWP release line remains separate. Shared fixes are cherry-picked, not published
  from the private tree.

### 3.8 Relationship with StackSpirit Desktop, Web, CLI and MCP

| Product | Relationship to this engine | Contract it must rely on |
|---|---|---|
| **StackSpirit Engine** | This repository: supervisor, store, graph, loops, adapters, agent, daemon, CLI and embedded MCP. | The compatibility contract in §4. |
| **StackSpirit Desktop** (`Stack-Spirit/stackspirit`) | A control surface and UI. It talks to a home over SSH and the loopback `/api/v1` API through a tunnel; it does not open the database. | Stable `/api/v1` documents, `orchestrator endpoint --json`, `snapshot.json`, and the `stackspirit`/`vowp` CLI. It should resolve both names and fall back to `vowp`, never hard-code one. |
| **StackSpirit Web** | The PWA/web window served by `stackspirit-daemon` from the home. It is a UI over the same API and SSE streams. | The existing routes, service-worker bypass, and no-store stream headers. Route and file names do not change. |
| **StackSpirit CLI** | The `stackspirit` binary from this engine, with the `vowp` compatibility binary. | The command surface, exit codes, JSON output and `--home`/environment rules. |
| **StackSpirit MCP** | Today an embedded `stackspirit mcp` / `vowp mcp` stdio server. A future standalone MCP process can be split out without changing tool names. | The 15 tool names, their schemas, `serverInfo`, and the `stackspirit`/`vowp` server key policy. |
| **Public VOWP** (`syver777/vowp`) | The open-source VOWP-branded release. | Keeps `vowp` names, packaging and 0.4.0-era public documentation. This plan does not push private branding there. |

Recommended desktop resolver order when it shells out: `stackspirit` on the target, then `vowp`.
For the remote home, the stored agent/control path still uses `vowp`; because the alias is always
installed, it works. A later desktop release can prefer `stackspirit` after it has probed the home.

### 3.9 Frozen compatibility surface

These names are deliberately **not** rebranded in v6. They are a storage or wire contract, not a
label:

- `vowp.db`, `project.db`, `vowp.db-wal`, `vowp.db-shm`;
- `.vowp/jobs`, `.vowp/checkpoints`, `.vowp/templates`, `.vowp/worktrees`, `.vowp/graph.yaml` and
  the rest of the project state directory;
- `.vowp-bundle.json` and the `vowp-bundle-v1` marker;
- `vowp-snapshot/1`;
- `vowp-agent/1` and the `/agent/v1` WSS path;
- `/api/v1/*`;
- the tmux socket prefix `vowp-`, session names `vowp-home` and `vowp-<project>`, and window names
  `job-<id>`;
- the OS account `vowp` used by an existing home and the isolated vault user `vowp-vault`;
- the historical `vowp` command strings stored in existing agent transports and orchestrator MCP
  configurations.

The `.vowp/` project directory deserves a note. It is the on-disk job contract: briefs, reports,
checkpoints and worktrees all live under it. Renaming it would invalidate existing plan paths,
`owns` globs, reports and every `job` command that writes there. A future `stackspirit/` project
directory can be introduced only with a dual-read/dual-write migration and a job-contract version
bump. It is not part of the rebrand.

Similarly, the database file name is not worth changing. Keeping `vowp.db` means any pre-rebrand
binary, backup script or restore instruction still finds the data. A future v7 can add
`stackspirit.db` with a verified copy/migration if the owner wants the name, but v6 does not.

## 4. Compatibility contract

### 4.1 Invariants

These are hard requirements for every v6 slice:

1. **No schema change.** No rebrand slice adds a migration, changes `PRAGMA user_version`, or
   rewrites an existing database. A rollback is therefore a binary rollback, not a restore.
2. **No wire change.** Agent protocol, snapshot, bundle, HTTP and SSE shapes stay compatible.
3. **New reads old.** Every new binary reads the legacy config file, home directory, agent state,
   control state, env spelling and daemon startup line.
4. **Old works on old data.** Existing hosts keep their current paths and config names until an
   owner explicitly chooses otherwise; upgrading a binary does not move data.
5. **The alias is always installed.** Installing `stackspirit` installs `vowp`; installing
   `stackspirit-daemon` installs `vowp-daemon`. The alias is part of the compatibility contract,
   not a deprecated afterthought.
6. **One process per role.** The installer never writes a second home unit, a second agent unit,
   or a second cron watchdog entry. The legacy unit is updated in place or explicitly renamed.
7. **Both environment names are exported.** Any process that starts a child exports
   `STACKSPIRIT_HOME` and `VOWP_HOME`, both agent names, and both daemon/tmux names where relevant.
8. **No silent path choice.** Two distinct homes/agent states/control states are a conflict. A
   broken symlink is not a home. A missing explicit variable is an error, not a fallback.
9. **No automatic data migration.** Discovery is pure. Only `home migrate --move --yes` (or a
   documented manual equivalent) moves data, and it refuses while the node is running.
10. **One-way only for new projects.** Existing repositories keep `vowp.toml` so old binaries keep
    working. A repository created with `stackspirit.toml` requires a v6 binary.

### 4.2 Mixed-version matrix

The compatibility matrix is intentionally boring because the rebrand changes presentation, not
data:

| Combination | Result |
|---|---|
| Old agent ↔ new home | Works. The agent protocol is still `vowp-agent/1`; enrollment and node keys are unchanged. |
| New agent ↔ old home | Works. The new agent uses the legacy transport command `vowp agent gateway --stdio`; `vowp` exists on the home. |
| Old supervisor ↔ new daemon | Works. The new daemon prints the legacy `vowp-daemon:` line and serves the same `/api/v1` routes. |
| New supervisor ↔ old daemon | Works. The new supervisor accepts both startup-line prefixes. |
| New CLI ↔ old home | Works. The resolver falls back to `~/.vowp`, `vowp.toml`, `config.toml` and legacy env names. |
| Old CLI ↔ new home | Works while the legacy path or symlink exists. Fresh installs create `~/.vowp` as a symlink to the new home by default; an explicit move leaves the legacy symlink. |
| New daemon ↔ old Desktop | Works. Routes, JSON, pagination, SSE and auth are unchanged. |
| New MCP server ↔ old client | Works. Tool names and schemas are unchanged; `serverInfo` is not used for addressing. |
| Old MCP server ↔ new client | Works. The new client keeps the old config key for existing orchestrators and uses the same tool names. |
| New generated MCP config ↔ old engine after rollback | Works only if the generated command is `vowp mcp`, which is why v6 keeps that command. The key may be new; the command is old. |
| New repository with `stackspirit.toml` ↔ old CLI | Does not work by design. This is a new-project boundary, not the live migration path. |
| Old backup/restore scripts ↔ new home | Works while `vowp.db` and the directory layout are unchanged; scripts that use `VOWP_HOME` also work because the legacy variable is still read. |

If any combination above fails a test, the slice is not mergeable.

### 4.3 Failure behavior

The fallback must be loud, not magical:

- Two real homes: exit `Conflict` (5) with both paths and the `--home` hint. Do not use one and
  ignore the other.
- Two real agent state directories: the same refusal, with the `STACKSPIRIT_AGENT_HOME` hint.
- Both config files differ: exit `Conflict` (5) naming both and the `init --force`/manual merge
  options. Do not pick the newer mtime.
- Explicit `STACKSPIRIT_HOME`/`VOWP_HOME` missing: report that exact path. Do not fall back to
  `$HOME`.
- Broken symlink: treat it as absent for selection, but report it in `doctor`; do not create
  through it.
- A unit rename requested while both units exist: refuse unless `--rename-unit` is explicit; the
  command stops/disables the old unit before enabling the new one.
- A home move requested while `supervisor.lock` is live or an agent socket is live: refuse. No
  copy-of-a-live-WAL risk.

## 5. Zero-downtime migration sequence

### 5.1 What "zero downtime" means here

- No active run, gate task or merge is killed. The supervisor is replaced through
  `vowp up --replace` / `vowp down --drain`, not by `systemctl restart` during work.
- No session log, database or job artifact is moved while a writer is live.
- No agent re-enrollment and no desktop re-pair. Node keys, surface tokens and session ids stay.
- No MCP tool rename; a running app may reconnect, but it sees the same tools.
- The web window may briefly reconnect while the daemon child restarts; `/api/v1` clients retry
  with their existing backoff.
- The legacy `vowp` name exists on every host throughout v6, so stored commands never point at a
  missing binary.

### 5.2 Deployment order

Use two cumulative releases. The split matters because the first is the compatibility bridge; the
second is the brand rollout. Both can be built from the same branch after the listed slices land,
but the live order is:

Commands in this section that use `stackspirit` are the target contract, not commands that exist
today. Until Release A lands, the installed binary is `vowp`; run the same steps with `vowp` and
then switch the command name once the bridge binary is verified.

**Release A — compatibility bridge (`v6-2` + `v6-3`)**

1. **Back up and record.** On the home, run the existing commands:

   ```sh
   vowp backup
   cp -a ~/.vowp ~/vowp-pre-stackspirit-backup
   vowp --version
   vowp doctor
   ```

2. **Install the bridge binaries beside the old ones.** Keep the old binary until the new one has
   answered `--version` and `doctor` against the old home. Save the current binaries before the
   alias replaces them; they are the rollback copies.

   ```sh
   cp -a ~/.local/bin/vowp ~/.local/bin/vowp.pre-stackspirit
   cp -a ~/.local/bin/vowp-daemon ~/.local/bin/vowp-daemon.pre-stackspirit
   install -m 0755 stackspirit ~/.local/bin/stackspirit.new
   mv ~/.local/bin/stackspirit.new ~/.local/bin/stackspirit
   ln -sfn stackspirit ~/.local/bin/vowp
   install -m 0755 stackspirit-daemon ~/.local/bin/stackspirit-daemon.new
   mv ~/.local/bin/stackspirit-daemon.new ~/.local/bin/stackspirit-daemon
   ln -sfn stackspirit-daemon ~/.local/bin/vowp-daemon
   stackspirit --version
   vowp --version
   stackspirit --home ~/.vowp doctor
   ```

   The `vowp`/`vowp-daemon` links are not optional. They keep the old systemd unit, agent
   transport, remote `--node` command, orchestrator configs and scripts working. If
   `~/.local/bin/vowp` is already a symlink, `cp -a` preserves that shape and the target binary
   must be copied explicitly before the link is replaced.

3. **Create the link bridge for the new names.** This does not touch data:

   ```sh
   test -e ~/.stackspirit || ln -s .vowp ~/.stackspirit
   stackspirit home migrate --link --dry-run   # planned command; prints the same link actions
   stackspirit home migrate --link --yes
   ```

   If `~/.stackspirit` already exists as a real directory, stop and resolve it before continuing;
   the resolver will refuse two distinct homes.

4. **Replace the supervisor with a drain.** The old supervisor is still running from the old
   binary; the new command starts from the new binary. `--replace` drains first and admits no new
   work until the new supervisor owns the lock.

   ```sh
   stackspirit up --replace
   stackspirit status --check
   vowp status --check
   ```

   The home path, socket, session logs and `vowp.db` are unchanged, so the tmux views stay valid.
   The daemon child restarts and writes a fresh `web-window` address; its clients reconnect with
   their existing backoff.

5. **Verify the home before touching agents.**

   ```sh
   stackspirit doctor
   stackspirit fleet status
   stackspirit orchestrator ls
   stackspirit mcp --help
   ```

   `doctor` must report the legacy config/home as a warning, not an error. A `vowp` and
   `stackspirit` status must agree.

6. **Upgrade agent nodes.** On each Mac/Linux agent, first stop or move any local orchestrator so
   a service restart cannot drop a live app window; the home log is authoritative and
   `orchestrator move --home` preserves the session if needed. Then install both binaries and the
   link bridge:

   ```sh
   vowp agent status
   # On the control machine: vowp orchestrator ls; move any local one back with
   # vowp orchestrator move <name> --home before restarting this service.
   cp -a ~/.local/bin/vowp ~/.local/bin/vowp.pre-stackspirit
   cp -a ~/.local/bin/vowp-daemon ~/.local/bin/vowp-daemon.pre-stackspirit
   install -m 0755 stackspirit ~/.local/bin/stackspirit.new
   mv ~/.local/bin/stackspirit.new ~/.local/bin/stackspirit
   ln -sfn stackspirit ~/.local/bin/vowp
   install -m 0755 stackspirit-daemon ~/.local/bin/stackspirit-daemon.new
   mv ~/.local/bin/stackspirit-daemon.new ~/.local/bin/stackspirit-daemon
   ln -sfn stackspirit-daemon ~/.local/bin/vowp-daemon
   test -e ~/.stackspirit-agent || ln -s .vowp-agent ~/.stackspirit-agent
   vowp agent service install --now
   vowp agent status
   vowp agent doctor
   ```

   `agent service install` updates the existing generated unit in place. It must not create
   `dev.stackspirit.agent`/`stackspirit-agent.service` while the old generated unit is enabled;
   that would produce two agents. A rename is a separate explicit action after the bridge release.

7. **Update StackSpirit Desktop and the control CLI.** The desktop release should resolve
   `stackspirit` first and `vowp` second, keep the same tunnel and `/api/v1` endpoint, and never
   parse a home path out of the snapshot. The control CLI updates in place; `control status` and
   `fleet status` still work because `control.json` keeps the legacy `remote_home` or a symlink.

**Release B — brand rollout (`v6-4` through `v6-7`)**

1. Deploy Release B to the home first and repeat the drain/replace check. No schema migration runs.
2. New orchestrators use the `stackspirit` MCP key and the new serverInfo. Existing orchestrators
   keep `vowp`. Verify one new orchestrator's `tools/list` and one existing orchestrator's tool
   calls.
3. Upgrade agents. New installs may use the new service label; existing units stay on the old
   label until an explicit `--rename-unit`. The generated unit carries both env names either way.
4. Update Desktop/Web to the new release. No engine route or token change is involved.
5. Run `scripts/install.sh`/packaging only for fresh hosts or when the owner explicitly wants the
   new names. Running the installer on a host with old generated units must update in place, not
   duplicate.
6. v6-7 regenerates documentation and templates and adds the user-facing migration guide.

**Optional, later: real directory move**

Only after the bridge has run everywhere and the owner has a quiet window. `home migrate --move`
copies, verifies, swaps, leaves a legacy symlink and prints the socket-preservation line. It is
not needed for the rebrand to be complete; the link bridge already gives the new name. A move is a
maintenance operation with a full backup.

### 5.3 Exact home sequence in one page

For the live VPS, the safe order is:

```text
backup + filesystem copy
  -> install stackspirit and vowp alias (old supervisor still running)
  -> create ~/.stackspirit -> ~/.vowp link (no data move)
  -> stackspirit --home ~/.vowp doctor
  -> stackspirit up --replace            (drain; old runs finish; no schema change)
  -> status on both names; fleet/orchestrator checks
  -> agent nodes: stop/move local orchestrators, install aliases, service install --now
  -> desktop/control update (same API)
  -> Release B: MCP key for new orchestrators, new units only on fresh hosts
  -> docs/templates cutover
  -> optional home migrate --move in a quiet window
```

### 5.4 Rollback

Because v6 has no schema or wire migration, rollback is a binary rollback:

1. If Release B is unhealthy, move `vowp.pre-stackspirit` and `vowp-daemon.pre-stackspirit`
   back over the aliases (or copy them over the selected binary paths), then restart with
   `vowp up --replace`. Existing `vowp` names and old configs are still present.
2. If an agent service was renamed, run the explicit reverse rename; the old unit remains
   generated by the same installer and can be restored.
3. If a new orchestrator used the `stackspirit` MCP key, the old engine can still run its
   `vowp mcp` command; the app may show the new key/prefix, so re-approve tools or recreate the
   key. No session data changes.
4. After a real directory move, stop the new supervisor, restore the pre-move directory or make
   the new path point back at the backup, restore the old symlink layout, and start the old
   binary. Keep the pre-move filesystem copy and `vowp backup` until the owner is satisfied.
5. Never roll back by editing a database or deleting a session log. There is no schema to undo.

## 6. Slice breakdown

Each slice is independently mergeable, has its own ownership, and must leave the tree with the
previous slices working. `needs_done` is the dependency edge the brief should declare. Every slice
ends with a report whose `status` is `done`, whose `commits` are bare short SHAs, and whose
`tests` field is one quoted string.

| Slice | `needs_done` |
|---|---|
| `v6-2-stackspirit-cli-alias-and-branding` | none |
| `v6-3-stackspirit-home-config-compatibility` | `v6-2` |
| `v6-4-stackspirit-mcp-namespace` | `v6-3` |
| `v6-5-stackspirit-service-and-daemon-units` | `v6-2`, `v6-3` |
| `v6-6-stackspirit-packaging-and-release` | `v6-3`, `v6-5` |
| `v6-7-stackspirit-docs-templates-and-cutover` | `v6-2` through `v6-6` |

### 6.1 `v6-2-stackspirit-cli-alias-and-branding`

**Goal.** Add the new binary identity without changing any default path, config name, unit, MCP
key or daemon line. This is the safe first step because it is additive.

**Owns.**

```text
Cargo.toml
crates/vowp-protocol/src/brand.rs              (new)
crates/vowp-protocol/src/lib.rs
crates/vowp-cli/src/main.rs
crates/vowp-cli/src/bin/vowp.rs                (new wrapper)
crates/vowp-cli/src/lib.rs
crates/vowp-cli/src/cli.rs
crates/vowp-cli/tests/branding.rs              (new)
crates/vowp-daemon/Cargo.toml
crates/vowp-daemon/src/main.rs
crates/vowp-daemon/src/bin/stackspirit-daemon.rs (new wrapper, if the wrapper shape is used)
scripts/release/version-test.rs
```

**What changes.**

- Add `stackspirit` and `vowp` bin targets; both call one entry point with a `Product` value.
- Add `stackspirit-daemon` and keep `vowp-daemon`; both call one daemon entry.
- Add the brand constants in `vowp_protocol::brand` (product name, binary names, legacy names,
  config names, directory names, env names, MCP identity). No path resolver uses them yet.
- `stackspirit --help`/`--version` use the new display identity. `vowp --help`/`--version` remain
  byte-for-byte compatible with the current output, including the version test's expected line.
- No default home, config, unit, MCP or data name changes in this slice.

**Acceptance.**

- `cargo build --workspace --bins` produces both CLI and both daemon binaries.
- `stackspirit --version` prints `stackspirit <version>`; `vowp --version` still prints
  `vowp <version>`.
- A test enumerates the two command trees and proves they are identical.
- A test starts each daemon binary against a temporary home and asserts both print the legacy
  `vowp-daemon: listening on` line.
- `stackspirit <command>` and `vowp <command>` produce the same exit code and JSON for a small
  set of read-only commands against the same temporary home.

**What must fail if absent.** The `branding` test cannot find the `stackspirit` binary or sees a
different command tree; the daemon line test fails if the primary daemon invents a new prefix;
the `vowp` version test fails if the compatibility output changed.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test -p vowp-cli --test branding`, `cargo test -p vowp-cli --test surface`,
`cargo test -p vowp-cli --test version`, `cargo test -p vowp-daemon`, `cargo test --workspace`,
`python3 docs/reference/check-reference.py`, `report`.

### 6.2 `v6-3-stackspirit-home-config-compatibility`

**Goal.** Make new names resolve safely and implement the link/move migration without changing
any data during normal upgrades. This is the bridge release's second half.

**Owns.**

```text
Cargo.toml
crates/vowp-protocol/src/brand.rs
crates/vowp-store/src/paths.rs
crates/vowp-store/src/lib.rs
crates/vowp-cli/src/ctx.rs
crates/vowp-cli/src/node_ctx.rs
crates/vowp-cli/src/agent_exec.rs
crates/vowp-cli/src/chat/agent.rs
crates/vowp-cli/src/cmd/home.rs                 (new)
crates/vowp-cli/src/cmd/mod.rs
crates/vowp-cli/src/cmd/init.rs
crates/vowp-cli/src/cmd/migrate.rs
crates/vowp-cli/src/cmd/doctor.rs
crates/vowp-cli/src/cmd/gc.rs
crates/vowp-cli/src/cmd/offload.rs
crates/vowp-cli/src/cmd/orchestrator.rs
crates/vowp-cli/src/cmd/project.rs
crates/vowp-cli/src/cmd/session.rs
crates/vowp-cli/src/cmd/surface.rs
crates/vowp-cli/src/cmd/agent.rs
crates/vowp-cli/src/mcp/backend.rs
crates/vowp-cli/src/setup/mod.rs
crates/vowp-cli/src/setup/commands.rs
crates/vowp-cli/src/setup/verify.rs
crates/vowp-cli/src/cli.rs
crates/vowp-agent/src/config.rs
crates/vowp-core/src/deps.rs
crates/vowp-core/src/agent/dispatch.rs
crates/vowp-core/src/agent/result.rs
crates/vowp-core/src/capacity/reload.rs
crates/vowp-core/src/loops/supervisor.rs
crates/vowp-core/src/surface/mcp.rs
crates/vowp-core/src/vault/mod.rs
crates/vowp-daemon/src/config.rs
crates/vowp-cli/tests/home.rs                   (new)
crates/vowp-cli/tests/init.rs
crates/vowp-cli/tests/project_path.rs
crates/vowp-cli/tests/agent.rs
crates/vowp-cli/tests/doctor.rs
crates/vowp-cli/tests/setup.rs
crates/vowp-store/tests/migrate.rs
crates/vowp-core/tests/deps.rs
crates/vowp-core/tests/capacity_reload.rs
crates/vowp-core/tests/node_supervisor.rs
crates/vowp-core/tests/agent.rs
crates/vowp-core/tests/vault.rs
```

**What changes.**

- Implement the home/agent/control resolver from §3.3 in one place; export pure helpers for
  tests and a small environment-reading entry point.
- Implement the project config resolver from §3.2; replace every direct `root.join("vowp.toml")`
  with the helper (including `find_project_root`, capacity fingerprints, supervisor reload, MCP,
  daemon, agent, offload and doctor).
- Implement `stackspirit home migrate` (link and offline move modes) and register the new
  command. It writes only after `--dry-run` or `--yes`, refuses live nodes, and never edits a
  database.
- Add `STACKSPIRIT_*` read precedence and export both names to children. Keep internal-only
  variables on their current spelling.
- New `init` writes `stackspirit.toml`; existing checkouts keep `vowp.toml`. New home creation
  uses `stackspirit.toml` for the reserved `_node` file; existing homes keep `vowp.toml`.
- Keep `.vowp/`, `vowp.db` and all tmux names unchanged.
- `doctor` reports the selected home, agent/control state and config file, and marks a legacy
  path as a warning with the migration hint.

**Acceptance.**

- A home with only `~/.vowp` is selected; a home with only `~/.stackspirit` is selected; both
  distinct homes are a conflict; a new-name symlink to the old home is accepted as one home.
- `STACKSPIRIT_HOME` wins over `VOWP_HOME`; a conflicting pair produces one warning; an explicit
  missing new path is not silently replaced.
- A repository with only `vowp.toml`, only `stackspirit.toml`, or both same-file works; both
  different files is a conflict. `init --force` updates the existing name and does not create a
  second file.
- `home migrate --link --dry-run` writes nothing; `--link --yes` creates the symlink and leaves
  the source untouched; `--move` refuses with a live `supervisor.lock` and succeeds only after a
  verified copy.
- A child process started by the supervisor sees both home/agent env names with the same value.
- The existing v0.1→v0.2 database migration still works; this slice adds no schema step.

**What must fail if absent.** A test that only puts a home at `~/.vowp` with a DB fails if a
naive `~/.stackspirit` default wins; a test that creates both homes fails if the resolver picks
one silently; a test that runs `init` on an existing `vowp.toml` fails if a second file appears;
the move test fails if `--dry-run` writes or if a live-lock move proceeds.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test -p vowp-store --test migrate`, `cargo test -p vowp-cli --test home`,
`cargo test -p vowp-cli --test init`, `cargo test -p vowp-cli --test project_path`,
`cargo test -p vowp-cli --test agent`, `cargo test -p vowp-cli --test setup`,
`cargo test -p vowp-cli --test tmux_isolation`, `cargo test --workspace`, `report`.

### 6.3 `v6-4-stackspirit-mcp-namespace`

**Goal.** Change the MCP display identity and the key used for new orchestrators while preserving
every existing tool name and every existing orchestrator config.

**Owns.**

```text
crates/vowp-cli/src/mcp/mod.rs
crates/vowp-cli/src/mcp/backend.rs
crates/vowp-cli/src/mcp/tools.rs
crates/vowp-cli/src/mcp/proxy.rs
crates/vowp-core/src/surface/orchestrator.rs
crates/vowp-agent/src/orchestrator.rs
crates/vowp-adapters/src/acp/run.rs
crates/vowp-cli/tests/mcp.rs
crates/vowp-cli/tests/orchestrator.rs
crates/vowp-agent/tests/orchestrator.rs
crates/vowp-adapters/tests/acp.rs
```

**What changes.**

- `serverInfo.name` and `server/discover` report `stackspirit-engine` in both binaries.
- Generated opencode/Claude files for **new** orchestrators use the `stackspirit` key and the
  command `vowp mcp` (the alias keeps rollback and stored commands working).
- Existing generated files keep their current `vowp` key; `orchestrator resume` detects and
  preserves it.
- The 15 tool names and every input schema stay exactly as they are; the proxy's `tools/list`
  still forwards the same list.
- `clientInfo` becomes `stackspirit-control`; no routing depends on it.
- The ACP client initialization name changes to `stackspirit`; the ACP wire fields are otherwise
  unchanged.

**Acceptance.**

- Initialize and discover both report `stackspirit-engine`, and `tools/list` contains exactly the
  15 names with no prefix change.
- A new orchestrator writes `mcp.stackspirit` and `mcpServers.stackspirit`; a resumed existing
  orchestrator keeps `vowp`.
- A tool call through both `vowp mcp` and `stackspirit mcp` reaches the same backend.
- No generated file contains a secret; the existing no-token assertions still pass.
- The orchestrator tests prove that a resume does not silently re-key an existing config.

**What must fail if absent.** `server_info_names_the_engine` fails on the old `vowp` value; the
new-orchestrator test fails if the key stays `vowp`; the resume test fails if an existing key is
rewritten; the tools test fails if a tool name or count changes.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test -p vowp-cli --test mcp`, `cargo test -p vowp-cli --test orchestrator`,
`cargo test -p vowp-agent --test orchestrator`, `cargo test -p vowp-adapters --test acp`,
`cargo test --workspace`, `report`.

### 6.4 `v6-5-stackspirit-service-and-daemon-units`

**Goal.** Make the service layer choose one unit, carry both environment names and use the new
daemon identity without duplicating a running service.

**Owns.**

```text
crates/vowp-agent/src/service.rs
crates/vowp-cli/src/cmd/agent.rs
crates/vowp-cli/src/cmd/doctor.rs
crates/vowp-cli/src/cmd/up.rs
crates/vowp-cli/src/setup/commands.rs
crates/vowp-cli/src/setup/mod.rs
crates/vowp-cli/src/setup/vowp.service
crates/vowp-cli/src/setup/stackspirit.service
crates/vowp-daemon/src/config.rs
crates/vowp-daemon/src/main.rs
crates/vowp-daemon/src/bin/stackspirit-daemon.rs
scripts/vowp.service
scripts/stackspirit.service
crates/vowp-agent/tests/service.rs
crates/vowp-cli/tests/agent.rs
crates/vowp-cli/tests/doctor.rs
crates/vowp-cli/tests/setup.rs
crates/vowp-cli/tests/up.rs
crates/vowp-daemon/tests/pwa.rs
```

**What changes.**

- Add `stackspirit.service` as a second template for fresh installs. Keep `vowp.service` as the
  legacy template, updated in place for existing hosts. The embedded copies must stay identical
  to their `scripts/` counterparts, and a test proves it.
- Both units set `STACKSPIRIT_HOME` and `VOWP_HOME` to the same selected home, point `ExecStart`
  at the selected binary and use the selected working directory.
- `setup`/`doctor`/install selection follows §3.5. A host with both units is refused; a rename is
  explicit.
- Agent service picks `dev.stackspirit.agent`/`stackspirit-agent.service` on a fresh host and
  updates the old label in place on an existing host. Both env names are written.
- Daemon discovery tries `STACKSPIRIT_DAEMON_BIN`, then `VOWP_DAEMON_BIN`, then the
  `stackspirit-daemon` sibling, then `vowp-daemon`, then `PATH` for both. The startup-line parser
  accepts both prefixes.
- `vowp-daemon --help` keeps its current usage text; `stackspirit-daemon --help` uses the new
  name and the same options.

**Acceptance.**

- A legacy home unit is updated, not duplicated; a fresh home gets the new name; a host with both
  files exits with a conflict unless `--rename-unit`.
- The rendered systemd and launchd units contain both env names and the selected binary.
- A fake service-manager test proves no second `enable`/`start` is issued.
- The daemon starts under both names against a temporary home, prints the legacy line and serves
  one `/api/v1/board` request.
- `vowp up` finds a `vowp-daemon` sibling when only that name is installed and a
  `stackspirit-daemon` sibling when only the new name is installed.

**What must fail if absent.** The unit test fails if a legacy host gets two units; the env test
fails if one name is missing; the daemon discovery test fails if the new sibling is ignored; the
startup-line test fails if the new supervisor does not accept the new prefix.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test -p vowp-agent --test service`, `cargo test -p vowp-cli --test agent`,
`cargo test -p vowp-cli --test doctor`, `cargo test -p vowp-cli --test setup`,
`cargo test -p vowp-cli --test up`, `cargo test -p vowp-daemon`, `cargo test --workspace`,
`report`.

### 6.5 `v6-6-stackspirit-packaging-and-release`

**Goal.** Make installation, uninstallation and release artifacts produce the new names without
breaking the public VOWP release or creating a second copy of anything on an existing host.

**Owns.**

```text
scripts/install.sh
scripts/uninstall.sh
scripts/vowp-watchdog.sh
scripts/vowp-watchdog.cron
scripts/tests/run.sh
scripts/release/build.rs
scripts/release/check-packages.sh
scripts/release/package-archive.sh
scripts/release/publish-crates.sh
scripts/release/version-test.rs
.github/workflows/release.yml
packaging/homebrew/README.md
packaging/homebrew/update.sh
packaging/homebrew/vowp.rb
packaging/homebrew/stackspirit-engine.rb        (new)
packaging/npm-stackspirit/**                   (new, private variant)
packaging/npm/package.json
packaging/npm/bin/vowp.js
packaging/npm/lib/download.js
packaging/npm/lib/install.js
packaging/npm/lib/tar.js
packaging/npm/lib/targets.js
packaging/npm/scripts/postinstall.js
packaging/npm/scripts/publish.sh
packaging/npm/test/*.test.js
```

**What changes.**

- `scripts/install.sh` gains a product mode (`--product stackspirit` or the equivalent release
  environment) that installs both names, both daemon names, the watchdog and the selected unit.
  The public/default mode remains the VOWP installer; the public repository and its archive names
  do not change.
- Existing hosts: the installer detects a legacy unit/watchdog/cron entry and updates it in
  place. It never adds a second `systemctl` service, a second cron line or a second watchdog.
- Fresh hosts: new names and new unit; legacy symlinks are created by default unless explicitly
  disabled.
- Uninstall removes only files it installed; it recognizes both names, keeps node state, and
  refuses to delete a foreign unit.
- Release packaging builds both CLI binaries and both daemon binaries. The private archive name is
  `stackspirit-engine-<version>-<target>.tar.gz`; the public VOWP archive path stays
  `vowp-<version>-<target>.tar.gz`.
- The release smoke test runs both `stackspirit --version` and `vowp --version`.
- Homebrew/npm templates gain private StackSpirit variants but do not publish anything. Publishing
  remains an owner action outside this job.

**Acceptance.**

- A dry-run install on a fresh temporary HOME names `stackspirit`, `vowp`,
  `stackspirit-daemon`, `vowp-daemon`, `stackspirit.service` and both env names.
- A dry-run install on a legacy fake home updates `vowp.service` and the old watchdog/cron entry;
  it does not create `stackspirit.service`, a second cron line or a second watchdog.
- `uninstall` removes the generated files and leaves state; a dry-run is accurate.
- `package-archive.sh` rejects an archive missing either CLI or either daemon binary and accepts a
  complete one.
- The release workflow's smoke test checks both version lines and both archive products.
- No network is required in tests; the existing fake `curl`, `systemctl`, `crontab` and temp HOME
  are reused.

**What must fail if absent.** The fresh-install test fails if the `vowp` alias is not created;
the legacy-install test fails if `stackspirit.service` is added beside `vowp.service`; the archive
test fails if the daemon is omitted; the smoke test fails if only the old version line is checked.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `bash scripts/tests/run.sh`, the offline npm tests under
`packaging/npm/test` when Node is available, `report`.

### 6.6 `v6-7-stackspirit-docs-templates-and-cutover`

**Goal.** Make the private tree self-consistent, regenerate the reference pages from the primary
binary, add the operator migration guide and state the deprecation policy. No behavior changes.

**Owns.**

```text
README.md
CHANGELOG.md
docs/**
docs/tests/**
docs/reference/generate.py
docs/reference/check-reference.py
framework/**
crates/vowp-cli/src/framework.rs
crates/vowp-cli/src/setup/verify.rs
```

**What changes.**

- Rewrite the private README, install guide, VPS guide, agent guide, troubleshooting and security
  page for the StackSpirit names, with a visible "VOWP compatibility" note.
- Add a user-facing migration guide (planned path: `docs/guides/stackspirit-migration.md`) with the
  exact home/agent/desktop sequence from §5.
- Update the design index and the v6 scope note; leave the historical v0.2–v0.5 notes intact.
- Regenerate `docs/reference/cli.md` and `docs/reference/schema/` from the primary binary; the
  reference generator must accept the binary name or default to `stackspirit`. If the generator
  still emits a `vowp` title, a small compatibility note at the top is mandatory.
- Update framework templates and prompts from `vowp` command examples to `stackspirit` where they
  are instructions, while keeping `.vowp/` paths and report/brief paths unchanged.
- Write the deprecation policy: `vowp`/`vowp-daemon`/legacy units/`vowp.toml`/`~/.vowp` are
  supported for all of v6; removal is a v7 decision after stored configs and agent transports are
  migrated. No v6 slice prints a "removed" error for a legacy name.

**Acceptance.**

- `bash docs/tests/run.sh` passes; links and every documented command resolve.
- `python3 docs/reference/check-reference.py` passes, including the new `home` command if it was
  added in v6-3.
- `cargo build -p vowp-cli --bin stackspirit` followed by `python3 docs/reference/generate.py
  --check` passes.
- Every `stackspirit` example in the user docs is either a command that exists in `cli.rs` or is
  visibly marked planned.
- The migration guide contains the link bridge, backup, drain, agent, desktop, rollback and
  optional move sections; it does not promise that old binaries can read a new-only project.
- No public VOWP repository file, URL or tag is changed by this job.

**What must fail if absent.** The docs link check fails on a missing migration guide or a broken
relative link; the reference check fails if `docs/reference/cli.md` omits `home`; the generate
check fails if the page is stale against the primary binary; a docs command check fails if a
documented command does not exist and is not marked planned.

**Gates.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `bash docs/tests/run.sh`, `python3 docs/reference/check-reference.py`,
`python3 docs/reference/generate.py --check` after building `stackspirit`, `report`.

## 7. Test strategy and gates

### 7.1 Hermetic rules for every slice

The repository's normal rules apply without exception:

- Temporary homes and temp repositories only; never the real `$HOME`, `~/.vowp`,
  `~/.vowp-agent` or `~/.vowp-control`.
- A private tmux socket (`-L <unique>`) **and** a private `TMUX_TMPDIR`; create the temp directory
  first, refuse `default`, and unset `TMUX`/`TMUX_PANE`. A test or script that touches tmux must
  refuse a non-private socket before spawning.
- Fake `claude`, `opencode`, `systemctl`, `launchctl`, `crontab`, `curl`, `ssh` and `tmux` where
  the behavior is not the subject of the test.
- Environment probes use child processes, not `std::env::set_var` in a parallel test process.
  Rust 2024 makes the latter unsafe and tests must stay `#![forbid(unsafe_code)]`.
- No network. Daemon tests may bind a loopback ephemeral port as the existing tests do; they must
  not reach an external host.
- No secret values in fixtures, logs or generated unit text.

### 7.2 Test matrix

| Behavior | Test shape | What must fail if absent |
|---|---|---|
| Both binaries have one command tree | Spawn both with `--help`/`--version`, compare the command lists and exit codes | A wrapper that drifts or a missing alias |
| Home fallback and conflict | Temp HOME with `~/.vowp`, `~/.stackspirit`, symlink and both-real cases; child process probes | A silent choice or a wrong default |
| Config fallback and conflict | Temp checkout with each name and both; run `init`, `doctor`, `project add` | A direct `vowp.toml` join left behind |
| Env precedence and dual export | Child process with both variables; assert selected path and child env | New-only or old-only export |
| Link/move migration | Temp home with DB/session data; dry-run/yes/live-lock; symlink assertions | A write during dry-run or a move under a live lock |
| MCP identity | In-process JSON-RPC initialize, discover, tools/list | Old serverInfo, changed tool names, lost key preservation |
| Agent service | Fake manager; render unit; legacy and fresh hosts; both units collision | A duplicate unit or missing env name |
| Daemon discovery | Temp bin directory with one or both names; parse both startup lines | New sibling ignored or new line not accepted |
| Packaging | Existing `scripts/tests/run.sh` style with fake commands and temp HOME; archive contents | A missing alias, duplicate service or missing daemon |
| Docs | `docs/tests/run.sh`, reference generator/checker | Stale command page or broken link |

### 7.3 Gates every slice runs

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Then the slice-specific tests above, and `report` last. The docs slice adds
`bash docs/tests/run.sh`, `python3 docs/reference/check-reference.py` and the generator check. The
packaging slice adds `bash scripts/tests/run.sh`. Every gate result and the exact commands belong
in the job report.

## 8. Risks, non-goals and owner decisions

### 8.1 Non-goals for v6

- Renaming the Unix account `vowp` or the vault user `vowp-vault`.
- Renaming `vowp.db`, `project.db`, `.vowp/`, `vowp.db-wal`/`-shm`, session logs or blob paths.
- Renaming JSON format tags, HTTP routes, SSE event names, WSS paths or MCP tool names.
- Changing the database schema or the agent protocol.
- Publishing to crates.io, npm, Homebrew or GitHub Releases from the private tree.
- Pushing StackSpirit branding to the public `syver777/vowp` repository.
- A v7 removal of legacy names. v6 supports them.
- A new cloud service, hosted relay or public endpoint.

### 8.2 Risks and mitigations

| Risk | Mitigation |
|---|---|
| A home is moved while the supervisor or a job is live, splitting WAL or losing a log | Discovery never moves. A move requires a stopped supervisor, a backup, a verified copy and a legacy symlink. Normal upgrades use the link bridge. |
| Two enabled supervisors or two agents after a unit rename | Selection is by existing unit; a rename is explicit, stops/disables first, and tests assert exactly one service action. |
| A stored `vowp mcp`/`vowp agent gateway`/remote `vowp` command breaks because only `stackspirit` was installed | The installer always creates both names; a test fails if the alias is missing. The alias is a compatibility contract for all of v6. |
| An existing orchestrator loses its MCP tool permissions | Existing config keys are preserved; only new orchestrators use `stackspirit`; tool names never change. |
| The new daemon's startup line hides the web window from an old supervisor | The new daemon keeps the legacy line; the new supervisor accepts both. |
| A project still reads `vowp.toml` in one code path and `stackspirit.toml` in another | One helper, a grep-style test that rejects direct joins, and the full workspace test suite. |
| tmux windows disappear after a real move because the socket hash changes | Migration prints the old/new socket and the explicit environment line; windows are views, so no run is lost; a move is maintenance-only. |
| Old binaries cannot read a repository created with `stackspirit.toml` | This is a documented new-project boundary; existing repos keep the old file. Do not create a new-only repo on a host that still needs rollback. |
| Public VOWP release is accidentally changed by private branding | Packaging keeps a product selector and separate artifact names; the docs job has a no-public-files gate; no publishing is run. |

### 8.3 Owner decisions recorded with defaults

These are not blocking for the design, but the implementation slices should not drift from them
without an owner decision:

1. **`stackspirit`, not `spirit`.** Default taken: `stackspirit`; no short alias in v6.
2. **The `vowp` alias lives for all of v6.** Default taken: never remove it in a v6 release.
3. **Keep `.vowp/` and `vowp.db` in v6.** Default taken: keep them; a v7 migration can rename the
   data layer with a real plan.
4. **Keep crate names.** Default taken: no crate or library rename; brand only the product shell.
5. **New MCP key for new orchestrators only.** Default taken: no automatic re-key; existing
   sessions keep `vowp`.
6. **Private release version and tag.** Default taken: keep the workspace semver, tag only in the
   private repository, never move a public `syver777/vowp` tag. The owner chooses the exact
   release number when a release is cut.

## 9. Definition of done for the rebrand programme

The programme is complete when all of the following are true:

- `stackspirit` and `stackspirit-daemon` are the primary user-facing names, and `vowp` /
  `vowp-daemon` are installed aliases with identical behavior.
- New homes, agents and control machines resolve the StackSpirit names; old ones keep working
  through fallback and the link bridge.
- A live home can be upgraded by drain/replace with no schema change, no lost run and no lost
  session log; the rollback is a binary rollback.
- A Mac agent can be upgraded without re-enrollment, with one service unit and both env names.
- StackSpirit Desktop and Web continue to use `/api/v1`, the snapshot, `orchestrator endpoint` and
  the same MCP tools.
- The private documentation builds against the primary binary and the public VOWP tree is
  untouched.
- Every v6 slice has a `done` report with reproducible gate evidence.
