# StackSpirit D6 readiness — local orchestrators (gated)

**Status: D6 remains gated and is not implemented or enabled.**
`features.local_orchestrator_actions` is `false` in this build. The Engine view can show the
local-orchestrator gate, its evidence, and the exact missing capabilities; it has no create or
attach action.

The installed VOWP CLI is **not D6-ready**: the Phase 3 `vowp agent surface start` and
`vowp agent surface attach` commands are absent, the home has no active local-agent node, and
the VOWP Phase 4 CLI job `v4-local-4f-cli-surface` is still pending. A VOWP version string is
**not** evidence: the probes in §1 are what count.

- **Evidence date:** 2026-11-16.
- **Job:** `ss-18-d6-readiness`; this document is the only tracked repository path changed.
- **VOWP CLI under test:** the installed `vowp` reports `vowp 0.3.0`.
- **Writes performed by this job:** none to VOWP, the local agent, or the VOWP home. No
  `vowp agent enroll` was run.
- **Redaction:** no real host, hostname, key, token, home path, session id, surface id, or
  one-time code value appears in this document; placeholders are used instead.

## 1. Current evidence

### 1.1 Probe scope

All commands below were run read-only on the job host against the **installed** `vowp` binary.
No real SSH host was contacted and no VOWP database was opened by hand. The desktop's real D6
probe runs the home half over SSH; these local commands exercise the same CLI surface and are
enough to disprove readiness, not to prove a live desktop probe.

`vowp agent status` is a **Mac-side** command. This job host is the home/control side, so its
`vowp agent status` result says only that this host has no agent enrollment. The owner must run
the same command on the Mac for the local-agent half. It is never valid home evidence.

Runtime claims below cite a command and exit code. Source claims cite a file in §7; a source
file is not installed evidence.

### 1.2 Command evidence (2026-11-16)

`<project>` and `<node>` are redaction placeholders. The actual names are deliberately omitted.

| # | Exact command | Exit | Parsed result (redacted) | What it proves |
|---:|---|---:|---|---|
| 1 | `vowp --version` | 0 | `vowp 0.3.0` | The CLI runs. A version string alone does not enable D6. |
| 2 | `vowp agent --help` | 0 | Subcommands `enroll`, `paths`, `pair`, `run`, `status`; no `surface` | The installed agent CLI has no local app-host command. |
| 3 | `vowp agent surface --help` | 2 | `error: unrecognized subcommand 'surface'` | The Phase 3 Mac surface commands are absent. |
| 4 | `vowp surface --help` | 0 | Subcommands `pair`, `ls`, `revoke`; no `start`, `attach`, or `rotate` | The top-level `surface` command is Phase 1 pairing, not the local app host. |
| 5 | `vowp surface ls --json` | 0 | `[]` | No paired surface exists. |
| 6 | `vowp agent status --json` | 3 | `kind: not_found`; “agent is not enrolled”; no status document | The local agent half is not enrolled. This is a Mac-side command; the result is from the job host. |
| 7 | `vowp agent paths ls --json` | 3 | Same `not_found` enrollment error | No local path map is available. |
| 8 | `vowp node ls --json` | 2 | `kind: invalid_input`; “no project selected; pass `--project <name>` or run `vowp init`” | A bare node inventory is not usable on this multi-project home. |
| 9 | `vowp --project <project> node ls --json` | 0 | 1 node; fields `name`, `ssh_target: null`, `is_local: true`, `capabilities`, `last_seen_at`, `created_at`; **no `kind` or `status`** | On this multi-project home the v0.3 `Node` shape appears only with a project selected. It is necessary but not sufficient for D6. |
| 10 | `vowp --project <project> node show <node> --json` | 0 | `kind: home`; `status: active`; `presence: offline`; `protocol: null`; `agent_version: null`; `runs: 0`; `mapped: 0 entries`; `capacity: null` | There is no active local-agent node; the only node is the home node, offline. |
| 11 | `vowp --project <project> node paths <node> --json` | 2 | `error: unrecognized subcommand 'paths'` | The home-side path-map read model used by the current probe is absent. |
| 12 | `vowp offload status --json` | 0 | 1 node; `status: active`; `presence: offline`; `mapped: 0 entries`; `assignments: 0`; `ready: 0` | The Phase 2 offload read model exists, but no local agent is online. |
| 13 | `vowp orchestrator --help` | 0 | Subcommands `new`, `ls`, `stop`, `resume` | Orchestrator commands exist before Phase 3; this is not sufficient evidence. |
| 14 | `vowp orchestrator new --help` | 0 | `--node` appears only under the global option text “Run the command on that node over ssh”; the `orchestrator new` local-node after-help is absent | The Phase 3 local-node meaning of `--node` is not proven. A substring check can false-positive on the global SSH flag. |
| 15 | `vowp orchestrator ls --json` | 0 | 4 cards: 2 `running`, 2 `stopped`; **0 of 4 have a `node` field** | The read model works, but installed cards do not carry a local node. The gate's node must come from `node show`, not from this list. |
| 16 | `vowp mcp --help` | 0 | Required `--surface <ID>`; `--agent-socket <PATH>` is advertised | The MCP proxy flag exists. The rest of the MCP proxy cannot be exercised without a local surface. |

### 1.3 Installed binary versus the VOWP source checkout

The VOWP source checkout contains later Phase 4 work than the installed binary: source has a
`surface rotate` command, an optional `OrchestratorCard.node` field, WSS/service libraries, and
the local-agent orchestrator host. The installed `vowp 0.3.0` does not expose those command
surfaces (for example, `vowp surface rotate --help` exits 2 here). Source presence is **not**
installed capability.

The Phase 4 CLI release job is `v4-local-4f-cli-surface`. Until it is merged and that build is
installed on both the home and the Mac, the home's installed CLI is what the SSH probe sees and
the Mac's installed CLI is what the local-agent half sees; neither is D6-ready here.

### 1.4 VOWP Phase 4 job evidence

These are read-only VOWP supervisor reads, not capability probes. They show why the CLI release
is not available yet.

| Exact command | Exit | Parsed result | Meaning |
|---|---:|---|---|
| `vowp --project <vowp-project> job show v4-local-4f-cli-surface --json` | 0 | `state: pending`; `kind: build`; no report | The Phase 4 CLI surface job has not run. |
| `vowp --project <vowp-project> job show v4-local-4d-bundle-result --json` | 0 | `state: fixing` (state changed 2026-11-15T23:28:52Z; `v4-local-4d-bundle-result-fix-1` is `running`) | A dependency of `4f` is not merged yet; its first fix round is in progress. |
| `vowp --project <vowp-project> job show v4-local-4g-hardening-docs --json` | 0 | `state: pending`; depends on `4f` | The docs/release gate follows the CLI surface. |
| `vowp --project <vowp-project> job ls --all --json` | 0 | `v4-local-4b-node-runtime`, `v4-local-4c-rotation-ux`, and `v4-local-4e-metrics-history` are `done`; `v4-local-4d-bundle-result` is `fixing`; `v4-local-4f-cli-surface` and `v4-local-4g-hardening-docs` are `pending` | The Phase 4 chain is not complete. |

**Conclusion of §1:** the installed VOWP CLI is not D6-ready. The missing surface commands,
inactive/absent local-agent node, absent home path-map read model, and pending Phase 4 CLI job
are independent blockers. `vowp 0.3.0`, `vowp orchestrator ls`, `vowp node ls`, and an
`vowp offload status` exit 0 are necessary but not sufficient.

## 2. Gate contract: what `LocalOrchestratorGate=Ready` requires

`LocalOrchestratorGate=Ready` is a read-only capability result. The home half runs over SSH; the
local-agent half runs on the Mac with the local `CommandRunner`, never over SSH. A `Ready` home
does not enable an action by itself: `local_orchestrator_enabled()` also requires
`features.local_orchestrator_actions` and `LocalAgentStatus=connected`.

| # | Requirement | Exact probe | Pass condition | Current result (2026-11-16) |
|---:|---|---|---|---|
| 1 | Home reachable | `vowp --version` over SSH | Exit 0 and a parseable version; **evidence only, never a gate by itself** | Exit 0, `vowp 0.3.0` |
| 2 | Node inventory | `vowp node ls --json` over SSH | Exit 0 and a parseable array with at least one node | Bare command exit 2 on this multi-project home; with `--project <project>`, exit 0 and 1 node |
| 3 | Active local-agent node | `vowp node show <node> --json` for each node (bounded, in order) | A `NodeCard` with `kind`, `status`, and `presence`; select one with `kind == "agent"` and `status == "active"` | Only `kind: home`, `status: active`, `presence: offline` |
| 4 | Agent protocol | The selected `NodeCard.protocol` | Exactly `vowp-agent/1`; another value is `Incompatible`; missing is `Waiting` | `protocol: null` |
| 5 | Phase 2 offload executor | `vowp offload status --json` | Exit 0 and a parseable document | Exit 0; 1 node, `presence: offline`, 0 assignments, 0 ready |
| 6 | Local opencode app host | `vowp agent surface start --help` | Exit 0 | Exit 2, unrecognized subcommand |
| 7 | Surface bridge | `vowp agent surface attach --help` | Exit 0 | Exit 2, unrecognized subcommand |
| 8 | MCP proxy | `vowp mcp --help` | Exit 0 and `--agent-socket` is advertised | Exit 0; `--agent-socket <PATH>` present |
| 9 | Orchestrator materialization | `vowp orchestrator new --help` | Exit 0 and the **local-node** meaning of `--node` is present, not merely the global SSH selector | Exit 0, but `--node` is only the global “over ssh” flag |
| 10 | Local cache (current StackSpirit probe) | `vowp node paths <node> --json` | Exit 0 and a parseable path map | Exit 2, unrecognized subcommand |
| 11 | Orchestrator read model | `vowp orchestrator ls --json` | Exit 0 | Exit 0; 4 cards, 0 with a `node` field |
| 12 | Local-agent half (Mac) | `vowp agent status --json` on the Mac | Exit 0; `connection: "connected"`; protocol `vowp-agent/1`; node captured | Exit 3; not enrolled on this host |

The node-row “current result” above was obtained with `--project <project>`. The shipped probe
does **not** add that flag, so its own `node ls`/`node show` reads fail before it can see the
card (§2.2, item 1).

### 2.1 Local-agent state mapping

`LocalAgentStatus` is displayed separately from the home gate. The home can be `Ready` while the
Mac agent is not installed, not enrolled, stopped, revoked, superseded, or reconnecting. In every
one of those states the action stays disabled.

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
`connected` enables an action.

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
   `--project <project>` exit 0. The current `spirit-core` probe builds both reads with the
   configured `--home` and `--json` but does not add `--project`. On a multi-project home this
   produces a false `Waiting` (“node model” or “no active local agent node”) even if the home is
   capable. This is a StackSpirit probe blocker to fix in a code job; it is not fixed by owner
   enrollment.
2. **`vowp orchestrator new --help` can false-positive on the global `--node`.** The installed
   help shows `--node` as the global SSH redirect. A substring probe sees the flag and passes
   even though Phase 3 local-node behavior is absent. Require the local-node after-help sentence
   (“For `orchestrator new`, --node <name> names the node that runs the app, not an SSH
   redirect.”) or a future capability document. Ambiguous output is `Waiting`, never `Ready`.
3. **`vowp node paths <node> --json` is a current probe requirement beyond this job's brief and
   `VOWP-CONTRACT.md` §11.3.4.** `crates/spirit-core/src/engine/gate.rs` uses it for the
   “local cache” capability. The installed CLI lacks the subcommand. Until VOWP adds it or the
   probe is reconciled with a documented home-side alternative, it is a hard blocker. Do not
   substitute the Mac-side `vowp agent paths ls --json` as home evidence.
4. **The installed `orchestrator ls` has no `node` field.** The source protocol now has an
   optional `OrchestratorCard.node`, but installed cards do not emit it. The gate's node comes
   from `vowp node show`; the UI must not infer a node from an absent field. This is a display
   gap, not a reason to mark the home gate `Ready`.
5. **Help probes are weaker than a capability document.** If a help output is ambiguous, the
   gate is `Waiting` (or `Unknown`), never `Ready`.
6. **A synthetic `gate-phase3-ready` fixture is not live evidence.** The fixture proves only
   that the disabled-action copy and capability list render. It must never be cited as a live
   orchestrator.

## 3. Owner setup steps (after the Phase 4 CLI build lands)

Do these only after VOWP `v4-local-4f-cli-surface` is merged and a build containing it is
installed on both the home and the Mac. Use VOWP's documented install/upgrade path for that
build; this document does not prescribe an installer because the release method is VOWP-owned.
Values in `<...>` are placeholders, not secrets. Commands marked “write” change local VOWP or
agent state; they are owner actions, not part of this documentation job.

### 3.1 Verify the installed build first

```sh
vowp --version
vowp agent surface start --help
vowp agent surface attach --help
vowp mcp --help | grep -F -- '--agent-socket'
vowp orchestrator new --help | grep -F -- 'not an SSH redirect'
vowp node paths --help
vowp --project <project> offload status --json >/dev/null
vowp --project <project> orchestrator ls --json >/dev/null
```

Expected after installing the Phase 4 build: every command exits 0. `vowp mcp --help` lists
`--agent-socket`. `vowp orchestrator new --help` must match `not an SSH redirect`; matching the
bare global `--node` line is not enough. On the installed `vowp 0.3.0` this grep exits 1, which
is the intended failure until a build with `v4-local-4f-cli-surface` is installed.
`vowp node paths --help` exits 0 only if the build provides the current probe's local-cache
read. Do **not** run `vowp agent surface start` or `vowp agent surface attach` for real in this
step; help only.

If the commands or flags are still missing, the build did not include
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
vowp --project <project> offload status --json
vowp --project <project> node paths <node> --json
vowp --project <project> orchestrator ls --json
```

Expected parsed fields:

- `node show`: `kind: "agent"`, `status: "active"`, `presence: "online"` (degraded is honest but
  not a live-action state), `protocol: "vowp-agent/1"`, `agent_version` present, `mapped`
  includes the project, and `runs` is whatever it is.
- `offload status`: exit 0; the local node's `presence` is `online` or `degraded`; no offline
  node is used for a live action.
- `node paths`: exit 0 and a parseable path map. Do not record the absolute paths.
- `orchestrator ls`: exit 0. A local card should carry a `node` only if the build emits it; do
  not invent one.

Mac-side read:

```sh
vowp agent status --json
```

Expected: exit 0; `connection: "connected"`; `protocol: "vowp-agent/1"`; `last_heartbeat`
present; `last_error` null or a redacted safe message. `connecting`, `reconnecting`, `stopped`,
`revoked`, `superseded`, and `incompatible` all leave the D6 action disabled.

### 3.6 Re-probe StackSpirit and record the gate

1. Connect the desktop to the home through the normal host connect flow.
2. Open **Engine** and use **Re-probe** on the **Local orchestrator** gate. The gate is scoped
   to the current connection; do not trust a snapshot from before the connection.
3. Confirm `LocalAgentStatus` is `connected` and `LocalOrchestratorGate` is `Ready`.
4. Confirm the feature flag is still false. The expected post-owner-setup state is:

   - `local_orchestrator.state = "ready"`
   - `agent_protocol = "vowp-agent/1"`
   - `node` present (record as `<node>`)
   - all capabilities present: `node_card`, `offload_status`, `surface_start`,
     `surface_attach`, `mcp_agent_socket`, `orchestrator_new_node`, `orchestrator_read`
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
| Orchestrator list | Exit code, card count, state counts, count of cards with a `node` field | Orchestrator names, session ids, join strings, paths |
| Local agent status | Exit code, `connection`, `protocol`, whether `last_heartbeat` is present, redacted `last_error` | Key fingerprint, token, code, state-directory path, absolute paths |
| StackSpirit gate | `state`, `missing`, `capabilities`, `evidence` | Raw snapshot with node name, session ids, or paths |

The final committed evidence must contain no real host, hostname, key, token, one-time code,
home path, session id, or surface id. Use `<node>`, `<project>`, `<home-ssh-target>`, and
`<timestamp>` placeholders when a value is needed for shape only.

## 4. What StackSpirit will build when D6 is ready

D6 is the local-orchestrator phase from `PLAN.md` §9.8. It is not implemented in this build. When
the gate is `Ready` **and** `features.local_orchestrator_actions` is enabled, the planned scope is:

- **Create and attach.** The Engine view can create or attach a local orchestrator through
  VOWP's own commands: `vowp orchestrator new --node <node>`, `vowp agent surface start` /
  `attach` / `detach`, and `vowp mcp --agent-socket`. StackSpirit does not implement a parallel
  supervisor or a second session writer.
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
  themselves. The Phase 2 and Phase 3 capability reads in §2 are required. `vowp agent status`
  is never run against the home. Ambiguous help output remains `Waiting`/`Unknown`.
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
- **Do not treat Phase 1 commands as Phase 3.** `vowp surface pair`, `vowp surface ls`,
  `vowp node ls`, `vowp node show`, and `vowp orchestrator ls` exist before the local app host;
  they are necessary but not sufficient.
- **Do not treat a version string as evidence.** `vowp 0.3.0` (or a later version) is recorded
  only with the command evidence in §1 and §2.

## 6. Unblock checklist

Keep this table current. If the installed CLI changes, re-run §1 and update the evidence rows and
this table. An owner action is not “done” until the named probe has the stated result.

| Missing or unproven | Action (owner or StackSpirit) | Exact probe that proves it | Current (2026-11-16) |
|---|---|---|---|
| Phase 4 CLI release `v4-local-4f-cli-surface` | Wait for `4d` to merge and `4f` to land; install that VOWP build on the home and Mac. Do not substitute the source checkout. | `vowp --project <vowp-project> job show v4-local-4f-cli-surface --json` → `state: done`; record `vowp --version` | `pending`; its dependency `4d` is `fixing` |
| `vowp agent surface start` and `vowp agent surface attach` | Install the build above on both machines. | `vowp agent surface start --help` and `vowp agent surface attach --help` → exit 0 | Both exit 2, unrecognized subcommand |
| Active local-agent node with protocol `vowp-agent/1` | On the home, create the pending node; on the Mac, enroll with the private code file, map paths, and run one agent. | `vowp --project <project> node show <node> --json` → `kind: agent`, `status: active`, `presence: online`, `protocol: vowp-agent/1`, `agent_version` present | Only `kind: home`, `status: active`, `presence: offline`, `protocol: null` |
| Local agent state `connected` | Enroll and run the agent as above; keep it running. | `vowp agent status --json` → exit 0, `connection: connected`, `protocol: vowp-agent/1`, `last_heartbeat` present | Exit 3, not enrolled |
| Home-side `vowp node paths <node> --json` for the probe's “local cache” check | Use a build that provides it, or reconcile the probe with a documented home-side alternative. Do not use Mac-side `agent paths ls` as home evidence. | `vowp --project <project> node paths <node> --json` → exit 0 and a parseable path map | Exit 2, unrecognized subcommand |
| Phase 3 local-node meaning of `vowp orchestrator new --node` | Install the build; verify the help's local-node sentence. Do not create an orchestrator yet. | `vowp orchestrator new --help` contains “For `orchestrator new`, --node <name> names the node that runs the app, not an SSH redirect.” | `--node` is only the global “Run the command on that node over ssh” option |
| StackSpirit probe passes a project for `vowp node ls --json` and `vowp node show <node> --json` | Fix the probe in a code job (not this docs job): pass the active project or run from the project root; then re-run the gate on a multi-project home. | Gate evidence includes `vowp node ls --json: exit 0` and a `vowp node show --json` card (or the documented `--project`-qualified commands) | Both bare commands exit 2; with `--project`, `node ls` exits 0 with 1 node and `node show` exits 0 with the card |
| Orchestrator card carries the local `node` for display (display gap) | Only after a local orchestrator exists, verify the build's card. The gate node comes from `node show`; this is not a reason to fake `Ready`. | `vowp --project <project> orchestrator ls --json` → exit 0 and the local card has `node: <node>` | Exit 0; 4 cards, 0 with a `node` field |

A `Ready` gate alone is not the finish line. The D6 build must also prove that the flag-off copy
is unchanged, the local agent’s non-connected states disable the action, the path-map validation
refuses a bad root, and no home secret or absolute path appears in shared state.

## 7. References

StackSpirit:

- [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md) §11.3 — local-orchestrator gate, probe algorithm,
  capability labels, security invariants, and the known contract gaps.
- [`docs/PLAN.md`](PLAN.md) §9.8 — D6 local-orchestrator scope; §8.5.7 — gate cards and feature
  flags; §9.1 — D6 is gated on VOWP Phase 2/3.
- [`docs/RELEASE-D5-ASIF.md`](RELEASE-D5-ASIF.md) §7.1 — D6 remains gated after D5.
- [`docs/RELEASE-DESKTOP-MVP.md`](RELEASE-DESKTOP-MVP.md) §4 and §8 — D6 limitation and the
  D1–D4 feature-flag statement.
- [`docs/OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) — owner-run evidence template and sign-off
  rule; D6 is not an owner-verified behavior in this build.
- [`../README.md`](../README.md) — capability gates and the cross-project VOWP relationship.

VOWP, read-only (these files live in the companion VOWP repository and are not modified here):

- `docs/design/v0.4/local-orchestrator.md` — §1.3 invariants, §4.2 node enrollment, §4.3 surface
  pairing, §5.7 local orchestrators, §6.2 path mapping, §10.2 no credential copying, §12 CLI
  surface, §14.1 phases.
- `docs/design/v0.4/hardening.md` §9.5 — the `v4-local-4f-cli-surface` scope and its dependency
  on `4b`/`4c`/`4d`/`4e`.
- `crates/vowp-cli/src/cli.rs` — installed/source command definitions for `agent`, `surface`,
  `orchestrator`, and the missing `agent surface` variant.
- `crates/vowp-cli/src/cmd/agent.rs` — `vowp agent status` exit-3 not-enrolled path and the
  status document.
- `crates/vowp-agent/src/status.rs` — `AgentStatus` fields and connection states.
- `crates/vowp-protocol/src/agent.rs` — `NodeCard`, `NodeKind`, and `NodeStatus` wire shapes.
- `crates/vowp-protocol/src/surface.rs` — `OrchestratorCard` and the optional local `node` field.
- `crates/vowp-cli/src/cmd/node.rs` — `vowp node ls` uses the project store; the bare command
  needs a selected project on a multi-project home.

StackSpirit source used for the gate contract:

- `crates/spirit-core/src/engine/gate.rs` — `LocalOrchestratorGate`, `LocalAgentStatus`,
  `GateFeatureFlags`, the probe algorithm, and the `node paths` local-cache check.
- `apps/spirit-desktop/src-tauri/src/state.rs` and `commands/gates.rs` — the D4/D5 feature flags
  and gate probing entry points.
- `packages/spirit-ui/src/domain/engine.ts` and `views/engine/GateCard.tsx` — the flag-off copy
  and the disabled-action rendering.
