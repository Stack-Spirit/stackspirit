# StackSpirit D6 readiness — local orchestrators (gated)

**Status: D6 remains gated and is not implemented or enabled.**
`features.local_orchestrator_actions` is `false` in this build. The Engine view can show the
local-orchestrator gate, its evidence, and the exact missing capabilities; it has no create or
attach action.

The **merged VOWP source** contains the Phase 3/4 local-orchestrator CLI surface:
`vowp surface pair|ls|revoke|rotate`, `vowp agent enroll|paths|pair|run|status|service|doctor`,
`vowp orchestrator new|ls|stop|resume` with the local-node `--node` meaning, and
`vowp mcp --agent-socket`. The **installed `vowp 0.3.0` binary** is stale: it has
`surface pair|ls|revoke` but no `surface rotate`, no `agent service|doctor`, and no
`orchestrator new` local-node after-help. The home also has no active local-agent node, and the
local agent is not enrolled. A VOWP version string is **not** evidence: the probes in §1 and §2
are what count.

There is **no `vowp agent surface start` / `attach` / `detach` command** in the merged CLI or the
installed binary. That spelling was a design sketch. The local app host is created by the
home-side `vowp orchestrator new --node <node> --app opencode`; the Mac agent materializes it.

- **Evidence date:** 2026-11-16.
- **Job:** `ss-20-d6-contract-reconcile`; this document is one of the tracked paths changed.
- **VOWP source under test:** merge `121f319` (`v4-local-4f-cli-surface`), follow-up
  `d523314`; source version still reports `0.3.0`.
- **VOWP CLI installed under test:** `vowp 0.3.0`.
- **Writes performed by this job:** none to VOWP, the local agent, or the VOWP home. No
  `vowp agent enroll`, `vowp surface pair`, or `vowp orchestrator new` was run.
- **Redaction:** no real host, hostname, key, token, home path, session id, surface id, or
  one-time code value appears in this document; placeholders are used instead.

## 1. Current evidence

### 1.1 Probe scope

All commands below were run read-only on the job host against the **installed** `vowp` binary.
No real SSH host was contacted and no VOWP database was opened by hand. The desktop's real D6
probe runs the home half over SSH; these local commands exercise the same CLI surface and are
enough to disprove readiness, not to prove a live desktop probe. Source claims cite the VOWP
checkout at `d523314`; they are not installed evidence.

`vowp agent status` is a **Mac-side** command. This job host is the home/control side, so its
`vowp agent status` result says only that this host has no agent enrollment. The owner must run
the same command on the Mac for the local-agent half. It is never valid home evidence.

Runtime claims below cite a command and exit code. Source claims cite a file or commit in §1.3
and §7; a source file is not installed evidence.

### 1.2 Command evidence (2026-11-16)

`<project>`, `<node>`, and `<surface>` are redaction placeholders. The actual names are
deliberately omitted.

| # | Exact command | Exit | Parsed result (redacted) | What it proves |
|---:|---|---:|---|---|
| 1 | `vowp --version` | 0 | `vowp 0.3.0` | The CLI runs. A version string alone does not enable D6. |
| 2 | `vowp agent --help` | 0 | Subcommands `enroll`, `paths`, `pair`, `run`, `status`; no `service`, `doctor`, or `surface` | The installed agent CLI is the older Phase 0/1 surface. |
| 3 | `vowp agent surface --help` | 2 | `error: unrecognized subcommand 'surface'` | The stale design command is absent from the installed binary. It is also absent from the merged source. |
| 4 | `vowp surface --help` | 0 | Subcommands `pair`, `ls`, `revoke`; no `rotate`; top-level `--node` after-help present | The home surface lifecycle exists, but the installed build lacks rotation. |
| 5 | `vowp surface pair --help` | 0 | `--session`, `--kind`, `--app`; **no `--replace`**; command-specific local-node after-help present | Pairing exists; the Phase 4 replacement form does not. |
| 6 | `vowp surface rotate --help` | 2 | `error: unrecognized subcommand 'rotate'` | Surface token rotation is absent from the installed build; the merged source has it. |
| 7 | `vowp surface ls --json` | 0 | `[]` | No paired surface exists. |
| 8 | `vowp agent status --json` | 3 | `not_found`; “agent is not enrolled”; no status document | The local agent half is not enrolled. This is a Mac-side command; the result is from the job host. |
| 9 | `vowp agent paths ls --json` | 3 | Same `not_found` enrollment error | No local path map is available. |
| 10 | `vowp node ls --json` | 2 | `invalid_input`; “no project selected; pass `--project <name>` or run `vowp init`” | A bare node inventory is not usable on this multi-project home. |
| 11 | `vowp --project <project> node ls --json` | 0 | 1 node; fields `name`, `ssh_target: null`, `is_local: true`, `capabilities`, `last_seen_at`, `created_at`; **no `kind` or `status`** | On this multi-project home the v0.3 `Node` shape appears only with a project selected. Necessary but not sufficient for D6. |
| 12 | `vowp --project <project> node show <node> --json` | 0 | `kind: home`; `status: active`; `presence: offline`; `protocol: null`; `agent_version: null`; `runs: 0`; `mapped: 0 entries`; `capacity: null` | There is no active local-agent node; the only registered node is the home node, offline. |
| 13 | `vowp offload status --json` | 0 | `nodes`, `assignments`, and `ready` arrays; 1 node, `presence: offline`, 0 assignments | The Phase 2 offload read model exists, but no local agent is online. |
| 14 | `vowp orchestrator --help` | 0 | Subcommands `new`, `ls`, `stop`, `resume` | Orchestrator commands exist before the full local surface; not sufficient evidence. |
| 15 | `vowp orchestrator new --help` | 0 | `--node` appears only under the global option “Run the command on that node over ssh”; the `orchestrator new` local-node after-help is absent | The local-node meaning of `--node` is not proven. A substring check false-positives on the global SSH flag. |
| 16 | `vowp orchestrator ls --json` | 0 | 4 cards: 2 `running`, 2 `stopped`; **0 of 4 have a `node` field** | The read model works, but installed cards do not carry a local node. The gate's node must come from `node show`. |
| 17 | `vowp mcp --help` | 0 | Required `--surface <ID>`; `--agent-socket <PATH>` advertised | The MCP proxy flag exists. The rest of the proxy cannot be exercised without a local surface. |
| 18 | `vowp node paths --help` | 2 | `error: unrecognized subcommand 'paths'` | The old design's home path read is absent. The merged source also has no `node paths` subcommand. |
| 19 | `vowp agent service --help` / `vowp agent doctor --help` | 2 | `unrecognized subcommand` | The installed build lacks the Phase 4 local service/diagnostics commands; the merged source has them. |
| 20 | `vowp surface revoke --help` | 0 | `<ID>` and `--yes` advertised | The revocation form exists, but no live surface was revoked. |

### 1.3 Installed binary versus the merged VOWP source

The VOWP checkout at `d523314` contains the merged Phase 4 CLI surface. The installed
`vowp 0.3.0` binary lags it:

| Surface | Merged source (`d523314`) | Installed `vowp 0.3.0` |
|---|---|---|
| `vowp surface pair` | present with `--replace` and the local-node after-help | present, but **no `--replace`** |
| `vowp surface ls` | present | present |
| `vowp surface revoke` | present with `--yes` | present with `--yes` |
| `vowp surface rotate` | present with `--wait`/`--grace` | **absent** |
| `vowp agent enroll` / `paths` / `pair` / `run` / `status` | present | present (some Phase 4 flags absent) |
| `vowp agent service` / `doctor` | present | **absent** |
| `vowp agent surface start` / `attach` / `detach` | **absent** (design sketch only) | **absent** |
| `vowp node paths` | **absent** | **absent** |
| `vowp orchestrator new` | `--node` has the local-node after-help | `--node` is only the global SSH selector |
| `vowp orchestrator ls` / `stop` / `resume` | present; `OrchestratorCard.node` is optional | present; installed cards emit no `node` |
| `vowp mcp --agent-socket` | present | present |

The source version still reports `0.3.0`, so a build with the merged surface may look
identical to the stale installed binary in `vowp --version`. That is exactly why the gate
uses command/flag/after-help probes, not the version.

Source presence is **not** installed capability. Until the build containing `121f319`/
`d523314` is installed on both the home and the Mac, the home's installed CLI is what the
SSH probe sees and the Mac's installed CLI is what the local-agent half sees. The
installed binary is not D6-ready.

### 1.4 VOWP Phase 4 job evidence

These are read-only VOWP supervisor reads, not capability probes. They show the CLI release
job landed in source while the docs release is still in review.

| Exact command | Exit | Parsed result | Meaning |
|---|---:|---|---|
| `vowp --project <vowp-project> job show v4-local-4f-cli-surface --json` | 0 | `state: done` | The Phase 4 CLI surface job is merged. |
| `vowp --project <vowp-project> job show v4-local-4d-bundle-result --json` | 0 | `state: done` | The bundle-result dependency is merged. |
| `vowp --project <vowp-project> job show v4-local-4g-hardening-docs --json` | 0 | `state: reviewing` | The docs/release gate follows the CLI surface and is in review. |
| `vowp --project <vowp-project> job ls --all --json` | 0 | `v4-local-4b-node-runtime`, `v4-local-4c-rotation-ux`, `v4-local-4d-bundle-result`, `v4-local-4e-metrics-history`, and `v4-local-4f-cli-surface` are `done`; `v4-local-4g-hardening-docs` is `reviewing` | The Phase 4 CLI chain is complete; only docs/release remains in review. |

**Conclusion of §1:** the **merged source** is D6-capable at the CLI surface, but the
**installed** VOWP CLI is not: it lacks `surface rotate`, `agent service|doctor`, the
`orchestrator new` local-node after-help, and an active local-agent node. The local agent is
not enrolled. `vowp 0.3.0`, `vowp orchestrator ls`, `vowp node ls`, and an
`vowp offload status` exit 0 are necessary but not sufficient.

## 2. Gate contract: what `LocalOrchestratorGate=Ready` requires

`LocalOrchestratorGate=Ready` is a read-only capability result. The home half runs over SSH; the
local-agent half runs on the Mac with the local `CommandRunner`, never over SSH. **Both halves
must pass.** `LocalAgentStatus` is displayed separately so the owner can see why the Mac half
fails, but a local agent that is not installed, not enrolled, or not `connected` leaves the gate
`Waiting`, never `Ready`. `local_orchestrator_enabled()` additionally requires
`features.local_orchestrator_actions`, which is `false` in this build.

| # | Requirement | Exact probe | Pass condition | Current result (2026-11-16) |
|---:|---|---|---|---|
| 1 | Home reachable | `vowp --version` over SSH | Exit 0 and a parseable version; **evidence only, never a gate by itself** | Exit 0, `vowp 0.3.0` |
| 2 | Selected project | Active project from the last snapshot | A safe project token; otherwise `Waiting` ("active project") and no bare node read | Available |
| 3 | Node inventory | `vowp --project <project> node ls --json` over SSH | Exit 0 and a JSON array with at least one node; malformed ⇒ `Incompatible` | Exit 0; 1 node, v0.3 `Node` shape |
| 4 | Active local-agent node | `vowp --project <project> node show <node> --json` for each node (bounded, in order) | A `NodeCard` with `kind`, `status`, and `presence`; `kind == "agent"`, `status == "active"`, `presence == "online"` | Only `kind: home`, `status: active`, `presence: offline` |
| 5 | Agent protocol | The selected `NodeCard.protocol` | Exactly `vowp-agent/1`; another value is `Incompatible`; missing is `Waiting` | `protocol: null` |
| 6 | Agent version | The selected `NodeCard.agent_version` | Present and non-empty; missing is `Waiting` | `agent_version: null` |
| 7 | Phase 2 offload executor | `vowp offload status --json` | Exit 0 with `nodes`, `assignments`, and `ready` arrays | Exit 0; 1 node, `presence: offline`, 0 assignments |
| 8 | Surface pairing | `vowp surface pair --help` | Exit 0; `--session`, `--kind`, `--app`, `--replace`, and the command-specific local-node after-help | Exit 0, local-node after-help present, but **no `--replace`** in the installed build |
| 9 | Surface read model | `vowp surface ls --help` | Exit 0 | Exit 0 |
| 10 | Surface revocation | `vowp surface revoke --help` | Exit 0 and `--yes` advertised | Exit 0; `--yes` present |
| 11 | Surface rotation | `vowp surface rotate --help` | Exit 0; `--wait`/`--grace` and the command-specific local-node after-help | Exit 2, unrecognized subcommand |
| 12 | MCP proxy | `vowp mcp --help` | Exit 0; `--surface` and `--agent-socket` advertised | Exit 0; both present |
| 13 | Orchestrator materialization | `vowp orchestrator new --help` | Exit 0; `--app`, `--force`, and the local-node after-help | Exit 0, but `--node` is only the global “over ssh” flag |
| 14 | Orchestrator lifecycle | `vowp orchestrator stop --help` and `resume --help` | Both exit 0 | Both exit 0 |
| 15 | Orchestrator read model | `vowp orchestrator ls --json` | Exit 0 with a JSON array | Exit 0; 4 cards, 0 with a `node` field |
| 16 | Local agent connection | `vowp agent status --json` on the Mac | Exit 0; `connection: "connected"`; protocol `vowp-agent/1`; node captured | Exit 3; not enrolled on this host |
| 17 | Local surface pairing | `vowp agent pair --help` on the Mac | Exit 0; `--code-file`, `--surface`, `--session` | Exit 0 help; no live pairing |
| 18 | Local agent run | `vowp agent run --help` on the Mac | Exit 0; `--foreground` and `--role`; this is also the bridge/backfill path | Exit 0 help; no live run |
| 19 | Local path map | `vowp agent paths --help` on the Mac | Exit 0; `add`, `rm`, `ls` | Exit 0 help; paths read exits 3 (not enrolled) |
| 20 | Local cache | `vowp agent status --json` on the Mac | `cache_entries` and `cache_bytes` present | Unavailable: exit 3 before a status document |
| 21 | Optional service/diagnostics | `vowp agent service --help` / `vowp agent doctor --help` | Exit 0, recorded as evidence only; not a Ready gate | Exit 2 on the installed build; merged source has both |

The node-row “current result” above was obtained with `--project <project>`. The shipped probe
adds that flag when the active project is known; if it is not known, the gate is `Waiting`
("active project") and no bare global node read runs (§2.2, item 1).

### 2.1 Local-agent state mapping

`LocalAgentStatus` is displayed separately from the home gate. However, `LocalOrchestratorGate`
itself is withheld from `Ready` until the Mac agent is `connected`. Not installed, not enrolled,
connecting, reconnecting, revoked, superseded, stopped, and incompatible all leave the gate
`Waiting` and the action disabled.

| `vowp agent status --json` result | `LocalAgentStatus.state` |
|---|---|
| Local `vowp` missing | `not_installed` |
| Exit 3 (no enrollment) | `not_enrolled` |
| Exit 0; `connection: unknown` | `unknown` |
| Exit 0; `connection: connecting` | `connecting` |
| Exit 0; `connection: connected` | `connected` |
| Exit 0; `connection: reconnecting` | `reconnecting` |
| Exit 0; `connection: revoked` | `revoked` |
| Exit 0; `connection: superseded` | `superseded` |
| Exit 0; `connection: stopped` | `stopped` |
| Exit 0 with a protocol other than `vowp-agent/1` | `incompatible` |

`vowp agent status` normalizes an unwritten or unknown stored connection to `stopped` before it
prints; StackSpirit maps a missing or unrecognized `connection` value to `unknown`. Only
`connected` leaves the gate eligible for `Ready`; every other state keeps it `Waiting`.

The enable rule in `crates/spirit-core/src/engine/gate.rs` is exact:

```text
local_orchestrator_enabled =
    features.local_orchestrator_actions
    && local_orchestrator is Ready
    && local_agent.state == connected
```

### 2.2 Probe caveats that must stay visible

These are not optional footnotes; each one can make a naive probe report the wrong thing.

1. **Both node reads need a selected project on this home.** The bare
   `vowp node ls --json` exits 2, and bare `vowp node show <node> --json` exits 2 with the same
   `invalid_input` “no project selected” message; the same commands with
   `--project <project>` exit 0. `spirit-core` now adds the active project to both reads. If no
   project is known, the gate is `Waiting` ("active project") and does not run a bare global node
   read. This is not fixed by owner enrollment.
2. **`--node` in help can false-positive on the global SSH selector.** The installed
   `vowp orchestrator new --help` shows `--node` as the global SSH redirect. The same trap exists
   on `vowp surface pair --help` and `vowp surface rotate --help`; a substring probe sees the
   flag and passes even though the local-node behavior is absent. Require the command-specific
   after-help sentence (“names the node that runs the app” / “names the node the surface runs
   on”, plus “not an SSH redirect”). The top-level `vowp surface --help` sentence is not enough
   for `surface pair`. Ambiguous output is `Waiting`, never `Ready`.
3. **`vowp node paths` is not a CLI command.** The old design sketch in
   `local-orchestrator.md` §12.1 lists it, but neither the installed binary nor the merged
   `vowp-cli` has the subcommand. The local cache is a **Mac-side** read from
   `vowp agent status --json` (`cache_entries`, `cache_bytes`, `cache_oldest_at`). Do not
   substitute `vowp agent paths ls --json` as home evidence and do not require `node paths`.
4. **The installed `orchestrator ls` has no `node` field.** The merged protocol has an optional
   `OrchestratorCard.node`, but installed cards do not emit it. The gate's node comes from
   `vowp node show`; the UI must not infer a node from an absent field. This is a display gap,
   not a reason to mark the gate `Ready`.
5. **Help probes are weaker than a capability document.** If a help output is ambiguous, the
   gate is `Waiting` (or `Unknown`), never `Ready`.
6. **A non-connected local agent is not Ready.** `LocalAgentStatus` is displayed separately,
   but `LocalOrchestratorGate::Ready` is withheld until `vowp agent status --json` is
   `connected` and the local `agent pair|run|paths` helps and cache fields are present. A
   stopped, revoked, superseded, reconnecting, not-installed, or not-enrolled agent never
   produces a `Ready` gate.
7. **A synthetic `gate-phase3-ready` fixture is not live evidence.** The fixture proves only
   that the disabled-action copy and capability list render. It must never be cited as a live
   orchestrator.
8. **The installed `surface pair` lacks `--replace`.** The merged source has the one-command
   replacement form, but the installed build does not. A missing flag is a missing capability;
   it keeps the gate `Waiting` and does not justify falling back to the stale
   `agent surface start|attach` spelling (which does not exist in either build).

## 3. Owner setup steps (after a build containing the merged Phase 4 CLI surface is installed)

Do these only after a build containing the merged `v4-local-4f-cli-surface` surface is
installed on both the home and the Mac. Use VOWP's documented install/upgrade path for that
build; this document does not prescribe an installer because the release method is VOWP-owned.
Values in `<...>` are placeholders, not secrets. Commands marked “write” change local VOWP or
agent state; they are owner actions, not part of this documentation job.

### 3.1 Verify the installed build first

```sh
vowp --version
vowp surface pair --help | grep -F -- '--replace'
vowp surface pair --help | grep -F -- 'names the node the surface runs on'
vowp surface rotate --help
vowp surface revoke --help | grep -F -- '--yes'
vowp agent pair --help | grep -F -- '--code-file'
vowp agent run --help | grep -F -- '--role'
vowp agent paths --help | grep -F -- ' ls '
vowp agent service --help
vowp agent doctor --help
vowp mcp --help | grep -F -- '--agent-socket'
vowp orchestrator new --help | grep -F -- 'not an SSH redirect'
vowp orchestrator stop --help
vowp orchestrator resume --help
vowp --project <project> offload status --json >/dev/null
vowp --project <project> orchestrator ls --json >/dev/null
```

Expected after installing the merged Phase 4 build: every help command exits 0. `surface pair`
lists `--replace` and its local-node after-help; `surface rotate` lists `--wait` and `--grace`;
`agent pair`, `agent run`, and `agent paths` list their subcommands/flags; `mcp --help` lists
`--agent-socket`; `orchestrator new --help` matches `not an SSH redirect`; and
`orchestrator stop|resume` exist. On the installed `vowp 0.3.0`, `surface rotate` exits 2 and
`surface pair` lacks `--replace`; those are the intended failures until the merged build is
installed, not reasons to fall back to the stale `agent surface start|attach` design.

`vowp agent service` and `vowp agent doctor` are optional. A foreground `vowp agent run` works
without service mode; missing doctor only removes a diagnostic. Record either result and do not
treat a missing optional command as D6-ready or D6-blocking by itself.

Do **not** run `vowp surface pair`, `vowp surface rotate`, `vowp orchestrator new`, or a real
`vowp agent run` in this step; help and read-only probes only.

If the required commands or flags are still missing, the build did not include
`v4-local-4f-cli-surface`. Stop and record the evidence; do not work around it.

### 3.2 On the home: create the pending node and one-time code (write)

```sh
vowp --project <project> node enroll <node> --label <label>
```

This creates a `pending` local-agent node and prints a one-time code **once**. The code is a
secret with a short TTL. Do not put it in a command argument, a shell variable assignment, a
log, a report, or this repository; clear the terminal scrollback if needed. Record only the node
name placeholder and the command exit code.

`--project` is the global project selector; on a multi-project home it selects the right project
store. The current installed `node enroll --help` lists `--label` plus the global options. Use
the release's documented flags and do not invent one.

### 3.3 On the Mac: enroll with a private code file (write)

```sh
umask 077
code_file="$(mktemp)"
# Paste the one-time code into "$code_file" in an editor. Do not echo it.
vowp agent enroll --target <home-ssh-target> --code-file "$code_file"
rm -f "$code_file"
```

`--code-file` reads the code from a file, so the code never appears in argv or `ps`. Never use
`--code-file <(echo ...)`, never put the code directly after `--code-file` as a literal, and
never commit the file. The commands remove the private file after success or failure; do not
leave it behind.

Expected: exit 0; the agent writes its private node key and agent configuration in its state
directory (private modes). If enrollment fails, record the redacted error and the exit code;
do not reuse the code.

### 3.4 Map the project paths and run the agent (write)

```sh
vowp agent paths add --project <absolute-local-checkout> --name <project> \
  --worktree-root <absolute-worktree-root>
vowp agent paths ls --json
vowp agent run --foreground --role both
```

`--project` here is an **absolute local checkout path** on the Mac, not a home project name.
Keep `--worktree-root` inside an allowed root. The path map is validated on the Mac and on the
home; a missing root or a symlink escape is refused. Run one agent in the foreground and leave
it running. Do not start a second `vowp agent run`, and do not start a second supervisor for
this.

If the installed release provides a supported service mode (`vowp agent service install` or
equivalent), use that instead of the foreground process; do not run both modes at once.

### 3.5 Verify the home and the Mac

Home-side reads (use the same project selection the probe must pass; see §2.2):

```sh
vowp --project <project> node show <node> --json
vowp offload status --json
vowp surface ls --json
vowp orchestrator ls --json
```

Expected parsed fields:

- `node show`: `kind: "agent"`, `status: "active"`, `presence: "online"` (degraded is honest but
  not a live-action state), `protocol: "vowp-agent/1"`, `agent_version` present, `mapped`
  includes the project, and `runs` is whatever it is.
- `offload status`: exit 0 with `nodes`, `assignments`, and `ready`; the local node's `presence`
  is `online` or `degraded`; no offline node is used for a live action.
- `surface ls`: exit 0. A pending/paired surface row is enough after pairing; do not record
  tokens, prefixes, or surface ids.
- `orchestrator ls`: exit 0. A local card may carry a `node` if the installed build emits it; do
  not invent one.

Mac-side reads:

```sh
vowp agent status --json
vowp agent paths ls --json
```

Expected: `agent status` exits 0 with `connection: "connected"`, `protocol: "vowp-agent/1"`,
`last_heartbeat` present, `cache_entries` and `cache_bytes` present, and `last_error` null or a
redacted safe message. `agent paths ls` exits 0; do not record absolute paths. `connecting`,
`reconnecting`, `stopped`, `revoked`, `superseded`, and `incompatible` all leave the gate
`Waiting` and the D6 action disabled.

### 3.6 Re-probe StackSpirit and record the gate

1. Connect the desktop to the home through the normal host connect flow.
2. Open **Engine** and use **Re-probe** on the **Local orchestrator** gate. The gate is scoped
   to the current connection; do not trust a snapshot from before the connection.
3. Confirm `LocalAgentStatus` is `connected` and `LocalOrchestratorGate` is `Ready`.
4. Confirm the feature flag is still false. The expected post-owner-setup state is:

   - `local_orchestrator.state = "ready"`
   - `agent_protocol = "vowp-agent/1"`
   - `node` present (record as `<node>`)
   - all capabilities present: `node_card`, `offload_status`, `surface_pair`,
     `surface_list`, `surface_revoke`, `surface_rotate`, `mcp_agent_socket`,
     `orchestrator_new_node`, `orchestrator_lifecycle`, `orchestrator_read`,
     `local_agent_connected`, `local_surface_pair`, `local_agent_run`, `backfill`,
     `local_path_map`, `local_cache`
   - `local_agent.state = "connected"`
   - `features.local_orchestrator_actions = false`
   - the **Run locally** action remains disabled with: “VOWP is ready; local orchestrator
     execution is not implemented in this build.”

5. If the gate is `WaitingForVowpPhase3`, `Incompatible`, or `Unreachable`, record the exact
   `missing`, `detected`, `required`, and `evidence` fields. Do not edit code, add a force
   switch, or flip the feature flag to make it green.

### 3.7 Evidence to record

Record only the command or observation, its exit code, and the parsed fields below. Store raw
JSON privately; do not commit it.

| Evidence | Record | Never record |
|---|---|---|
| Version and help probes | Command, exit code, whether the flag/command is present | Raw help text containing paths or names |
| Node card | `kind`, `status`, `presence`, `protocol`, `agent_version`, `runs`, `mapped` count | Node name, SSH target, absolute paths, capacity values that identify a host |
| Offload status | Exit code, node count, per-node `status`/`presence`, `assignments` count, `ready` count | Node names, assignment ids, host names |
| Surface list | Exit code, row count, per-row `state`/`kind`, whether a token expiry exists | Surface id, token, prefix, session id, node name |
| Orchestrator list | Exit code, card count, state counts, count of cards with a `node` field | Orchestrator names, session ids, join strings, paths |
| Local agent status | Exit code, `connection`, `protocol`, whether `last_heartbeat` is present, whether `cache_entries`/`cache_bytes` are present, redacted `last_error` | Key fingerprint, token, code, state-directory path, absolute paths, cache values |
| StackSpirit gate | `state`, `missing`, `capabilities`, `evidence` | Raw snapshot with node name, session ids, or paths |

The final committed evidence must contain no real host, hostname, key, token, one-time code,
home path, session id, or surface id. Use `<node>`, `<project>`, `<home-ssh-target>`, and
`<timestamp>` placeholders when a value is needed for shape only.

## 4. What StackSpirit will build when D6 is ready

D6 is the local-orchestrator phase from `PLAN.md` §9.8. It is not implemented in this build. When
the gate is `Ready` **and** `features.local_orchestrator_actions` is enabled, the planned scope is:

- **Create and pair.** The Engine view can create or attach a local orchestrator through
  VOWP's own commands: home-side `vowp surface pair`, Mac-side `vowp agent pair`, and
  `vowp orchestrator new --node <node> --app opencode` for the app host. `surface ls` /
  `surface rotate` / `surface revoke` own the surface token lifecycle; `orchestrator ls` /
  `stop` / `resume` own the read model and lifecycle. `vowp mcp --agent-socket` is the MCP
  proxy, and `vowp chat --agent --session <id>` is the chat surface. StackSpirit does not
  implement a parallel supervisor or a second session writer. There is no
  `vowp agent surface start` / `attach` / `detach` call in this build.
- **Surface ownership.** The local surface uses the StackSpirit Engine or the selected external
  harness. VOWP owns the canonical orchestrator record, session log, lease, and backfill.
  StackSpirit displays VOWP's state; it does not invent a running state.
- **Honest display.** The app shows the node, protocol, agent status, lease holder, and session
  state. Disconnected, degraded, reconnecting, revoked, superseded, and stopped states are
  visible and explain what is unavailable.
- **Feature-flag behavior.** `local_orchestrator_actions` is `false` now and is flipped only by
  the D6 build job after this checklist passes. The enable rule is the three-part
  `local_orchestrator_enabled()` expression in §2.1. A `Ready` gate with the flag off still
  shows “VOWP is ready; local orchestrator execution is not implemented in this build.” The
  backend must re-check the gate, connection, and flag on every D6 action; a stale renderer is
  not authority.
- **No fake and no version-only.** `vowp --version`, a synthetic `gate-phase3-ready` fixture, an
  `orchestrator ls` exit 0, or a `vowp node ls` exit 0 cannot make the gate `Ready` by
  themselves. The Phase 2 offload read, the home surface/orchestrator command probes, and the
  Mac `agent status`/`pair`/`run`/`paths`/cache reads in §2 are required. A non-connected local
  agent is never `Ready`. `vowp agent status` is never run against the home. Ambiguous help
  output remains `Waiting`/`Unknown`.
- **Refusal.** D6 refuses to start when the gate is not `Ready`, the local agent is not
  `connected`, the path map is invalid, the protocol is incompatible, or the node is revoked.
  The refusal names the missing prerequisite; there is no hidden force switch.

### 4.1 One-home, one-lease, and security invariants

| Invariant | What D6 must do |
|---|---|
| One home | The Mac never opens a VOWP database and never keeps a second session log. Pending local events are an outbox, not a log. |
| One lease | At most one surface writes a session. A local-surface write needs a valid surface token **and** the current lease token; a surface without the lease is read-only. |
| No credential copying | The node key proves the node, the surface token scopes the surface, and the lease token authorizes the writer. No home secret value, provider key, harness login, or SSH private key crosses the boundary. |
| No absolute local paths in shared state | New events, run rows, and snapshot cards use `PathRef` (`project:`, `worktree:`, `subdir:`). Absolute Mac paths stay on the Mac. |
| No root | The local agent never runs as root; the app never asks for root. |
| Revocation is honest | A revoked node fails the next handshake; StackSpirit stops and explains. It does not retry with the old credential or claim a live surface. |
| Provider names only | VOWP receives provider/model names and environment names, never provider key values. |

## 5. What not to do now

- **Do not enable D6.** Leave `features.local_orchestrator_actions = false`. Do not flip a build
  feature, add a hidden force switch, or enable a **Run locally** control.
- **Do not implement D6 actions in this job.** This document is readiness only.
- **Do not run `vowp agent enroll`, `vowp agent run`, `vowp node enroll`, or
  `vowp agent paths add` as part of this job.** Those are owner actions after the VOWP build
  lands; they write local/agent state.
- **Do not start a second supervisor.** Do not run a second `vowp up` or a second
  `vowp agent run` to make a probe pass.
- **Do not open a VOWP database by hand.** Read only through the public CLI. Do not copy
  `vowp.db`, project DBs, session logs, or home secrets.
- **Do not copy a home secret, token, one-time code, or key.** The node key proves the node; the
  surface token scopes the surface; the lease token authorizes the writer. No value crosses.
- **Do not claim a live orchestrator from a synthetic fixture.** `gate-phase3-ready` proves the
  disabled-action copy, not a running local orchestrator.
- **Do not treat Phase 1 commands as the local surface.** `vowp surface pair`, `vowp surface ls`,
  `vowp surface revoke`, `vowp node ls`, `vowp node show`, and `vowp orchestrator ls` exist
  before the full local host; they are necessary but not sufficient. `vowp surface rotate`,
  `vowp agent pair|run|paths|service|doctor`, the local-node `orchestrator new` surface, and the
  Mac `agent status` cache fields are also required.
- **Do not require the stale `vowp agent surface start` / `attach` / `detach` spelling.** It does not
  exist in the merged source or the installed binary. The local app host is
  `vowp orchestrator new --node <node> --app opencode`.
- **Do not treat a version string as evidence.** `vowp 0.3.0` (or a later version) is recorded
  only with the command evidence in §1 and §2.

## 6. Unblock checklist

Keep this table current. If the installed CLI changes, re-run §1 and update the evidence rows and
this table. An owner action is not “done” until the named probe has the stated result.

| Missing or unproven | Action (owner or StackSpirit) | Exact probe that proves it | Current (2026-11-16) |
|---|---|---|---|
| Install the Phase 4 CLI build | `v4-local-4f-cli-surface` is done in source; install that build on the home and Mac. Do not substitute the source checkout. | `vowp --project <vowp-project> job show v4-local-4f-cli-surface --json` → `state: done`; record `vowp --version` | Job `done`; installed binary is still `0.3.0` |
| Surface lifecycle `surface rotate` and `surface pair --replace` | Install the build above on the home. | `vowp surface rotate --help` → exit 0 with `--wait`/`--grace`; `vowp surface pair --help` → `--replace` present | `rotate` exit 2; `pair` has no `--replace` |
| Active local-agent node with protocol `vowp-agent/1` | On the home, create the pending node; on the Mac, enroll with the private code file, map paths, and run one agent. | `vowp --project <project> node show <node> --json` → `kind: agent`, `status: active`, `presence: online`, `protocol: vowp-agent/1`, `agent_version` present | Only `kind: home`, `status: active`, `presence: offline`, `protocol: null` |
| Local agent state `connected` | Enroll and run the agent as above; keep it running. | `vowp agent status --json` → exit 0, `connection: connected`, `protocol: vowp-agent/1`, `last_heartbeat`, `cache_entries`, `cache_bytes` present | Exit 3, not enrolled |
| Mac command surface `agent pair` / `run` / `paths` | Install the merged build on the Mac; these help commands are required by the gate. | `vowp agent pair --help`, `vowp agent run --help`, `vowp agent paths --help` → exit 0 with their documented flags/subcommands | Help exits 0 on the installed build; no live pairing/run |
| Phase 3 local-node meaning of `vowp orchestrator new --node` | Install the build; verify the help's local-node sentence. Do not create an orchestrator yet. | `vowp orchestrator new --help` contains “names the node that runs the app” and “not an SSH redirect” | `--node` is only the global “Run the command on that node over ssh” option |
| StackSpirit probe passes a project for `vowp node ls --json` and `vowp node show <node> --json` | Fixed in `ss-19-d6-probe-fix`; re-run the gate on a multi-project home. | Gate evidence includes `vowp node ls --json: exit 0` and a `vowp node show --json` card (or the documented `--project`-qualified commands) | Probe now passes `--project`; bare commands exit 2, with `--project` they exit 0 |
| Orchestrator card carries the local `node` for display (optional) | Only after a local orchestrator exists, verify the build's card. The gate node comes from `node show`; this is not a reason to fake `Ready`. | `vowp --project <project> orchestrator ls --json` → exit 0 and the local card has `node: <node>` | Exit 0; 4 cards, 0 with a `node` field on the installed build |
| Optional `agent service` / `agent doctor` | Install the merged build if the owner wants service mode or diagnostics. Foreground `agent run` does not need them. | `vowp agent service --help` and `vowp agent doctor --help` → exit 0 | Both exit 2 on the installed build; merged source has both |

A `Ready` gate alone is not the finish line. The D6 build must also prove that the flag-off copy
is unchanged, the local agent’s non-connected states leave the gate `Waiting`, the path-map
validation refuses a bad root, and no home secret or absolute path appears in shared state.

## 7. References

StackSpirit:

- [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md) §11.3 — local-orchestrator gate, probe algorithm,
  capability labels, security invariants, and the known contract gaps.
- [`docs/PLAN.md`](PLAN.md) §9.8 — D6 local-orchestrator scope; §8.5.7 — gate cards and feature
  flags; §9.1 — D6 is gated on VOWP Phase 2/3/4.
- [`docs/RELEASE-D5-ASIF.md`](RELEASE-D5-ASIF.md) §7.1 — D6 remains gated after D5.
- [`docs/RELEASE-DESKTOP-MVP.md`](RELEASE-DESKTOP-MVP.md) §4 and §8 — D6 limitation and the
  D1–D4 feature-flag statement.
- [`docs/OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) — owner-run evidence template and sign-off
  rule; D6 is not an owner-verified behavior in this build.
- [`../README.md`](../README.md) — capability gates and the cross-project VOWP relationship.

VOWP, read-only (these files live in the companion VOWP repository and are not modified here):

- `docs/design/v0.4/local-orchestrator.md` — §1.3 invariants, §4.2 node enrollment, §4.3 surface
  pairing, §5.7 local orchestrators, §6.2 path mapping, §10.2 no credential copying, §12 CLI
  surface, §14.1 phases. Note: §12.2 still shows the design-sketch `agent surface start|attach`
  spelling; the merged `cli.rs` is authoritative and has no such command. §12.1 still lists
  `node paths`; the merged CLI has no such subcommand.
- `docs/design/v0.4/hardening.md` §9.5 — the `v4-local-4f-cli-surface` scope; the job is merged
  in source at `121f319`.
- `crates/vowp-cli/src/cli.rs` — merged command definitions for `surface pair|ls|revoke|rotate`,
  `agent enroll|paths|pair|run|status|service|doctor`, and
  `orchestrator new|ls|stop|resume`.
- `crates/vowp-cli/src/cmd/surface.rs` — the home surface lifecycle and the
  `surface pair`/`rotate` local-node handling.
- `crates/vowp-cli/src/cmd/agent.rs` — `vowp agent status` exit-3 not-enrolled path, the status
  document, `agent pair`, `agent run`, and the optional service/doctor commands.
- `crates/vowp-cli/src/cmd/orchestrator.rs` — the `orchestrator new` local-node after-help and
  the `keepalive` path.
- `crates/vowp-agent/src/status.rs` — `AgentStatus` fields and connection states, including the
  cache counters.
- `crates/vowp-protocol/src/agent.rs` — `NodeCard`, `NodeKind`, `NodeStatus`, `SurfacePair`, and
  `SurfaceIssued`/`SurfaceRevoked` frame shapes.
- `crates/vowp-protocol/src/surface.rs` — `OrchestratorCard` and the optional local `node` field.
- `crates/vowp-cli/src/cmd/node.rs` — `vowp node ls` uses the project store; the bare command
  needs a selected project on a multi-project home; `vowp node show` returns the `NodeCard`.

StackSpirit source used for the gate contract:

- `crates/spirit-core/src/engine/gate.rs` — `LocalOrchestratorGate`, `LocalAgentStatus`,
  `GateFeatureFlags`, the probe algorithm, the home `surface`/`orchestrator` help checks, and
  the Mac-side cache/pair/run/paths checks.
- `apps/spirit-desktop/src-tauri/src/state.rs` and `commands/gates.rs` — the D4/D5 feature flags
  and gate probing entry points.
- `packages/spirit-ui/src/domain/engine.ts` and `views/engine/GateCard.tsx` — the flag-off copy
  and the disabled-action rendering.
