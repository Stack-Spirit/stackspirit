# StackSpirit D6 readiness — local orchestrators (gated)

**Status: D6 is implemented behind the `Ready` + connected gate in this build.**
The D6 desktop build advertises `features.local_orchestrator_actions`, but the published
flag is derived by `spirit-core` and is true only when `LocalOrchestratorGate` is `Ready`,
the local agent is `connected`, and the build flag is on. The Engine view can pair,
create, inspect, stop, resume, rotate, and revoke a local orchestrator/surface; every
action re-checks the gate, agent, node, protocol and path map on the backend. **Live
execution is still owner-gated**: the installed `vowp 0.4.0` CLI advertises the full
Phase 3/4 surface, and the live home now reports an online `macbook` agent card, but this
Linux host cannot verify the Mac-side `vowp agent status` (`connection: connected`), so the
gate is not claimed `Ready` and no owner write was performed.

The **shipped `vowp 0.4.0` release** contains the Phase 3/4 local-orchestrator CLI surface:
`vowp surface pair|ls|revoke|rotate`, `vowp agent enroll|paths|pair|run|status|service|doctor|trust`,
`vowp orchestrator new|ls|stop|resume` with the local-node `--node` meaning, and
`vowp mcp --agent-socket`. The installed binary on this host reports `vowp 0.4.0` (schema 1)
and every read-only help probe used by the D6 gate in §1/§2 passes. A VOWP version string is
**still not** evidence: the probes in §1 and §2 are what count. The live home card in §1.2
proves the home-side agent node; the runtime gate still stays `Waiting` until the Mac-side
local `vowp agent status` is `connected` and the desktop re-probe passes both halves.

`vowp agent trust --fingerprint <sha256:hex>` is new in 0.4.0. It is a **local-only,
owner-confirmed** security action for a changed system-mode WSS leaf fingerprint: the owner
verifies the new value out of band, then records it. StackSpirit never runs it automatically,
never accepts a fingerprint from the same connection, and never treats trust help as a D6
`Ready` capability. The probe records only `vowp agent trust --help` as optional evidence.

There is **no `vowp agent surface start` / `attach` / `detach` command** in the shipped CLI.
That spelling was a design sketch. The local app host is created by the home-side
`vowp orchestrator new --node <node> --app opencode`; the Mac agent materializes it.

- **Evidence date:** 2026-11-18.
- **Jobs:** `ss-32-d6-v04-reconcile` (installed-CLI and temp-home/agent-home evidence) and
  `ss-37-d6-live-agent-evidence` (live read-only home node and orchestrator reads). This
  document and `docs/VOWP-CONTRACT.md` are the tracked docs changed. All command evidence is
  read-only CLI evidence from the installed `vowp 0.4.0` binary; §1.2 adds the live home read
  and keeps the hermetic probes. No owner write was performed.
- **VOWP release under test:** `v0.4.0` (tag `v0.4.0`, commit `828ada6`), installed binary
  reports `vowp 0.4.0`; earlier source checkpoints are superseded by the release.
- **VOWP CLI installed under test:** `vowp 0.4.0`.
- **Writes performed by these jobs:** none to the live VOWP home, the local agent, or the
  owner's node/surfaces. The live read was limited to `vowp --version`, `node ls`,
  `node show` and `orchestrator ls`. Hermetic `vowp init`/`node ls`/`node show`/
  `offload status`/`surface ls`/`orchestrator ls` probes ran under a disposable temp home;
  `agent status`/`agent paths ls`/`agent doctor` ran under a disposable `VOWP_AGENT_HOME`.
  No `vowp agent enroll`, `vowp agent trust`, `vowp surface pair`, `vowp surface rotate`, or
  `vowp orchestrator new` was run.
- **Redaction:** no real SSH host, hostname, key, token, home path, session id, surface id,
  or one-time code value appears in this document; placeholders are used instead. The
  owner-facing agent node name `macbook` is already recorded in
  `docs/HANDOFF-DESKTOP-UI.md`; the home node name and every runtime id stay redacted.

## 1. Current evidence

### 1.1 Probe scope

All commands below were run read-only against the **installed** `vowp` binary on the job
host. No SSH host was contacted by these jobs. Rows 1–28 are the `ss-32` hermetic probes:
the home-shape rows use a disposable `vowp init` temp home (`--home <temp-home>`), and the
agent-shape rows use a disposable `VOWP_AGENT_HOME`; both were deleted after the probe.
Rows 29–32 are the `ss-37` live home read: the installed CLI read the active project's node
and orchestrator models read-only on the job host. That is a live **home-store** read, not
the desktop's SSH probe and not a Mac-side `vowp agent status`. The desktop's real D6 probe
runs the home half over SSH; these local commands exercise the same CLI surface.
The VOWP release citation is tag `v0.4.0`; it is not installed evidence by itself.

`vowp agent status` is a **Mac-side** command. This job host is the home/control side, so its
hermetic `agent status` result says only that a fresh agent home is not enrolled. The owner
must run the same command on the Mac for the local-agent half. The live `macbook` home-store
card does not change that and is never Mac evidence.

Runtime claims below cite a command and exit code. Source claims cite the v0.4.0 release
and the protocol files in §7; a source file is not installed evidence.

### 1.2 Command evidence (2026-11-18)

`<project>`, `<node>`, `<home-node>`, `<agent-node>`, `<surface>`, `<temp-home>`, and
`<temp-agent-home>` are redaction placeholders. The actual names are deliberately omitted.
Rows 1–28 are the `ss-32` hermetic probes; rows 29–32 are the `ss-37` live home read on the
same date.

| # | Exact command | Exit | Parsed result (redacted) | What it proves |
|---:|---|---:|---|---|
| 1 | `vowp --version` | 0 | `vowp 0.4.0`; `vowp --json version` also reports `version: "0.4.0"`, `schema: 1` | The CLI runs. A version string alone does not enable D6. |
| 2 | `vowp agent --help` | 0 | Subcommands `enroll`, `paths`, `pair`, `run`, `status`, `service`, `doctor`, `trust`; no `surface` | The shipped agent CLI includes the Phase 4/security surface. |
| 3 | `vowp agent surface --help` | 2 | `error: unrecognized subcommand 'surface'` | The stale design command is absent from the shipped binary. |
| 4 | `vowp surface --help` | 0 | Subcommands `pair`, `ls`, `revoke`, `rotate`; top-level `--node` after-help present | The full home surface lifecycle exists. |
| 5 | `vowp surface pair --help` | 0 | `--session`, `--kind`, `--app`, `--replace`; command-specific local-node after-help present | Pairing and one-command replacement exist. |
| 6 | `vowp surface rotate --help` | 0 | `--wait <DURATION>`, `--grace <DURATION>`; command-specific local-node after-help present | Surface token rotation exists. |
| 7 | `vowp surface ls --help` | 0 | List form advertised | The surface read model exists. |
| 8 | `vowp surface revoke --help` | 0 | `<ID>` and `--yes` advertised | The revocation form exists, but no live surface was revoked. |
| 9 | `vowp mcp --help` | 0 | `--surface <ID>` and `--agent-socket <PATH>` advertised (plus home-mode `--session`/`--token-file`) | The MCP proxy flag exists. |
| 10 | `vowp orchestrator --help` | 0 | Subcommands `new`, `ls`, `stop`, `resume` | The orchestrator lifecycle exists. |
| 11 | `vowp orchestrator new --help` | 0 | `--app`, `--force`; the `orchestrator new` local-node after-help is present | The local-node meaning of `--node` is proven, not the global SSH selector. |
| 12 | `vowp orchestrator stop --help` / `resume --help` | 0 | Both forms advertised | The orchestrator lifecycle forms exist. |
| 13 | `vowp agent pair --help` | 0 | `--code-file`, `--surface`, `--session` (and `--app`) advertised | The Mac-side pairing form exists. |
| 14 | `vowp agent run --help` | 0 | `--foreground`, `--role`, and `--service` advertised | The local agent run/bridge form exists. |
| 15 | `vowp agent paths --help` | 0 | Subcommands `add`, `rm`, `ls` | The local path map form exists. |
| 16 | `vowp agent service --help` | 0 | Subcommands `install`, `uninstall`, `status` | Optional service mode exists; not a Ready gate. |
| 17 | `vowp agent doctor --help` | 0 | Diagnostics form advertised; hermetic `agent doctor --json` exits 0 | Optional diagnostics exist; not a Ready gate. |
| 18 | `vowp agent trust --help` | 0 | `--fingerprint <SHA256:HEX>` advertised | Changed system-mode WSS fingerprint confirmation exists; owner-only, never auto-run. |
| 19 | `vowp node paths --help` | 2 | `error: unrecognized subcommand 'paths'` | The old design's home path read is absent. The shipped CLI also has no `node paths` subcommand. |
| 20 | `vowp --home <temp-home> --json node ls` | 0 | 1 node; fields `name`, `ssh_target`, `is_local`, `capabilities`, `last_seen_at`, `created_at`; **no `kind` or `status`** | The v0.3 `Node` inventory shape persists; necessary but not sufficient for D6. |
| 21 | `vowp --home <temp-home> --json node show <node>` | 0 | `kind: home`; `status: active`; `presence: online`; `protocol: null`; `agent_version: null`; `runs: 0`; `mapped: []`; plus `last_seen_at` | The fresh home's only node is the home node. Presence alone is not an agent. |
| 22 | `vowp --home <temp-home> --json offload status` | 0 | `nodes`, `assignments`, and `ready` arrays; 1 node, 0 assignments, 0 ready | The Phase 2 offload read model exists. |
| 23 | `vowp --home <temp-home> --json surface ls` | 0 | `[]` | The surface read model works; no surface exists in the temp home. |
| 24 | `vowp --home <temp-home> --json orchestrator ls` | 0 | `[]` | The orchestrator read model is a JSON array. |
| 25 | `VOWP_AGENT_HOME=<temp-agent-home> vowp --json agent status` | 3 | `not_found`; “agent is not enrolled”; no status document | The local-agent half needs enrollment. This is a Mac-side command; the result is from the job host. |
| 26 | `VOWP_AGENT_HOME=<temp-agent-home> vowp --json agent paths ls` | 3 | Same `not_found` enrollment error | No local path map is available without enrollment. |
| 27 | `vowp --project <project> session export 00000000000000000000000000 --format asif --out -` | 3 | Session not found; stdout empty | ASIF export is supported (the flag parsed; the nil session is absent). |
| 28 | `vowp session import --help` | 0 | `--format` lists `bundle` and `asif` | ASIF import capability is advertised. |
| 29 | `vowp --json --project <project> node ls` | 0 | 2 nodes. First: `is_local: false`, `capabilities.agent_version: "0.3.0"`, projects `[stackspirit, vowp]`. Second: `is_local: true`, `capabilities.role: "home"` | The live home lists both an agent node and the home node. `node ls` remains the v0.3 `Node` shape with **no `kind` or `status`**, so it is necessary but not sufficient. |
| 30 | `vowp --json --project <project> node show macbook` | 0 | `kind: agent`, `status: active`, `presence: online`, `protocol: "vowp-agent/1"`, `agent_version: "0.3.0"`, `runs: 0`, `mapped: [stackspirit, vowp]`; `last_error: null` | The live agent card. The home-side node/status/presence, protocol and version fields pass. The reported `0.3.0` agent version is an owner update item, not 0.4.0 agent evidence. |
| 31 | `vowp --json --project <project> node show <home-node>` | 0 | `kind: home`, `status: active`, `presence: offline`, `protocol: null`, `agent_version: null`, `runs: 0`, `mapped: []` | The home node is not an agent and can be offline while the agent is online. Node selection must keep scanning past it. |
| 32 | `vowp --json --project <project> orchestrator ls` | 0 | JSON array with 6 cards; 1 `running` card carries `node: <agent-node>`; 5 are `stopped` (2 of those carry a `node` field for a node not in the current inventory) | The live orchestrator read model exists. It is supporting evidence only, not proof of a live orchestrator, and its card ids are not recorded. |

`vowp --version` was re-run during the live read: exit 0, `vowp 0.4.0`. A version string is
still not evidence by itself.

### 1.3 Installed `vowp 0.4.0` versus the D6 probe

The `v0.4.0` release ships every command the D6 gate probes. The installed binary on this
host matches that surface:

| Surface | Shipped `vowp 0.4.0` | Probe result |
|---|---|---|
| `vowp surface pair` | present with `--replace` and the local-node after-help | exit 0 |
| `vowp surface ls` | present | exit 0 |
| `vowp surface revoke` | present with `--yes` | exit 0 |
| `vowp surface rotate` | present with `--wait`/`--grace` and the local-node after-help | exit 0 |
| `vowp agent enroll` / `paths` / `pair` / `run` / `status` | present | pair/run/paths help exit 0; status exits 3 without enrollment |
| `vowp agent service` / `doctor` | present | exit 0 (optional) |
| `vowp agent trust` | present with `--fingerprint` | exit 0 help (optional, owner-only) |
| `vowp agent surface start` / `attach` / `detach` | **absent** (design sketch only) | exit 2 |
| `vowp node paths` | **absent** | exit 2 |
| `vowp orchestrator new` | `--node` has the local-node after-help | exit 0 |
| `vowp orchestrator ls` / `stop` / `resume` | present; `OrchestratorCard.node` is optional | exit 0; empty array in the temp home, 6 cards in the live home |
| `vowp mcp --agent-socket` | present | exit 0 |

The live home read on 2026-11-18 used the same installed binary. `node show macbook`
returned `kind: agent`, `status: active`, `presence: online`, `protocol: vowp-agent/1` and
`agent_version: 0.3.0`; `node show <home-node>` returned `kind: home`, `presence: offline`,
`protocol: null`; and `orchestrator ls` returned an array. That is live home-store evidence
for the node/protocol/version rows and the read models, not a Mac-side `agent status` read
and not a `Ready` claim.

The live agent reports `agent_version: "0.3.0"` while the home CLI is `0.4.0`. The gate
requires a present, non-empty agent version, not the 0.4.0 string, so this is valid
home-card evidence; it is not evidence that a 0.4.0 agent is running. Upgrading or
restarting the Mac agent, and then reading the Mac-side `vowp agent status`, is an owner
update item.

The 0.4.0 release also retains `vowp node enroll --replace` (rotate a pending node's code in one
transaction), `vowp node rm --yes` (remove a node and purge its enrollment codes), and
authenticated `PathMapUpdate` persistence: the home validates the reported map lexically and
stores it in `node_paths`, replying `Pong`, and an invalid map returns a typed `Error` without
closing the session. These are owner-setup and session-robustness details, not gate evidence;
the §2 gate rules are unchanged.

The release reports its own version as `0.4.0`, but a version string is still not evidence.
The gate uses command/flag/after-help probes so an older or newer binary cannot pass on a
version match alone; conversely, a 0.4.0 version string cannot make a live home `Ready`
without an active agent node and a connected Mac agent.

### 1.4 Historical Phase 4 job evidence

The earlier `v4-local-4f-cli-surface`/`v4-local-4g-hardening-docs` job reads recorded on
2026-11-16 are superseded by the `v0.4.0` release. They were not re-run by `ss-32` because a
live VOWP job read would touch the owner's home; they were never capability probes. Use §1.2
and §2 for the current CLI evidence.

**Conclusion of §1:** the **installed** `vowp 0.4.0` CLI advertises the full D6 command
surface. The live home now also has an active `agent` node with protocol `vowp-agent/1`, so
the remaining D6 blocker is the Mac half: the local `vowp agent status` must be
`connected`. A fresh 0.4.0 home's only node is `kind: home`, even when online, so
`vowp node show`, `vowp offload status`, and an `orchestrator ls` exit 0 are necessary but
not sufficient. The live home demonstrates why: its home node is `kind: home` and
`offline` while its agent card is online, and the gate must select the agent card without
treating either read as the Mac half.

## 2. Gate contract: what `LocalOrchestratorGate=Ready` requires

`LocalOrchestratorGate=Ready` is a read-only capability result. The home half runs over SSH; the
local-agent half runs on the Mac with the local `CommandRunner`, never over SSH. **Both halves
must pass.** `LocalAgentStatus` is displayed separately so the owner can see why the Mac half
fails, but a local agent that is not installed, not enrolled, or not `connected` leaves the gate
`Waiting`, never `Ready`. `local_orchestrator_enabled()` additionally requires
`features.local_orchestrator_actions`, which this D6 build advertises. The published flag is
still false unless the gate is `Ready` and the local agent is `connected`.

| # | Requirement | Exact probe | Pass condition | Current result (2026-11-18) |
|---:|---|---|---|---|
| 1 | Home reachable | `vowp --version` over SSH | Exit 0 and a parseable version; **evidence only, never a gate by itself** | Exit 0, `vowp 0.4.0` |
| 2 | Selected project | Active project from the last snapshot | A safe project token; otherwise `Waiting` ("active project") and no bare node read | Live read passed `<project>`; the snapshot value remains owner state |
| 3 | Node inventory | `vowp --project <project> node ls --json` over SSH | Exit 0 and a JSON array with at least one node; malformed ⇒ `Incompatible` | Live home: exit 0, 2 nodes; temp home: exit 0, 1 node, v0.3 `Node` shape |
| 4 | Active local-agent node | `vowp --project <project> node show <node> --json` for each node (bounded, in order) | A `NodeCard` with `kind`, `status`, and `presence`; `kind == "agent"`, `status == "active"`, `presence == "online"` | Live home: `macbook` is `kind: agent`, `status: active`, `presence: online`; `<home-node>` is `kind: home`, `presence: offline` |
| 5 | Agent protocol | The selected `NodeCard.protocol` | Exactly `vowp-agent/1`; another value is `Incompatible`; missing is `Waiting` | Live `macbook` card: `vowp-agent/1`; temp home node: `null` |
| 6 | Agent version | The selected `NodeCard.agent_version` | Present and non-empty; missing is `Waiting` | Live `macbook` card: `0.3.0` present (owner update item); temp home node: `null` |
| 7 | Phase 2 offload executor | `vowp offload status --json` | Exit 0 with `nodes`, `assignments`, and `ready` arrays | Exit 0; 1 node, 0 assignments, 0 ready |
| 8 | Surface pairing | `vowp surface pair --help` | Exit 0; `--session`, `--kind`, `--app`, `--replace`, and the command-specific local-node after-help | Exit 0; all required flags and the after-help present |
| 9 | Surface read model | `vowp surface ls --help` | Exit 0 | Exit 0 |
| 10 | Surface revocation | `vowp surface revoke --help` | Exit 0 and `--yes` advertised | Exit 0; `--yes` present |
| 11 | Surface rotation | `vowp surface rotate --help` | Exit 0; `--wait`/`--grace` and the command-specific local-node after-help | Exit 0; all required flags and the after-help present |
| 12 | MCP proxy | `vowp mcp --help` | Exit 0; `--surface` and `--agent-socket` advertised | Exit 0; both present |
| 13 | Orchestrator materialization | `vowp orchestrator new --help` | Exit 0; `--app`, `--force`, and the local-node after-help | Exit 0; all required text present |
| 14 | Orchestrator lifecycle | `vowp orchestrator stop --help` and `resume --help` | Both exit 0 | Both exit 0 |
| 15 | Orchestrator read model | `vowp orchestrator ls --json` | Exit 0 with a JSON array | Live home: exit 0, 6 cards; temp home: `[]` |
| 16 | Local agent connection | `vowp agent status --json` on the Mac | Exit 0; `connection: "connected"`; protocol `vowp-agent/1`; node captured | Not verified from this Linux host; temp agent home exits 3, not enrolled |
| 17 | Local surface pairing | `vowp agent pair --help` on the Mac | Exit 0; `--code-file`, `--surface`, `--session` | Exit 0 help; no live pairing |
| 18 | Local agent run | `vowp agent run --help` on the Mac | Exit 0; `--foreground` and `--role`; this is also the bridge/backfill path | Exit 0 help; no live run |
| 19 | Local path map | `vowp agent paths --help` on the Mac | Exit 0; `add`, `rm`, `ls` | Exit 0 help; `paths ls` exits 3 in the temp agent home |
| 20 | Local cache | `vowp agent status --json` on the Mac | `cache_entries` and `cache_bytes` present | Not verified from this Linux host; exit 3 before a status document in the temp agent home |
| 21 | Optional service/diagnostics/trust | `vowp agent service --help` / `doctor --help` / `trust --help` | Exit 0, recorded as evidence only; not a Ready gate | All exit 0 on 0.4.0; `trust` advertises `--fingerprint`; never run without `--help` |

The live rows above come from the read-only home-store reads in §1.2; the negative cases come
from the disposable temp home. The shipped probe adds `--project` when the active project is
known; if it is not known, the gate is `Waiting` ("active project") and no bare global node
read runs (§2.2, item 1). The live home's offline home node and online agent card together
prove that presence alone is not accepted and that the probe must select the `kind: agent`
card; the Mac agent must still independently be `connected`.

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

`SpiritCore::set_gates` derives the published flag from this expression. The desktop
command boundary overlays the build's flags and then enforces the same expression, so a
stale renderer cannot enable a D6 action by itself.

### 2.2 Probe caveats that must stay visible

These are not optional footnotes; each one can make a naive probe report the wrong thing.

1. **Both node reads need a selected project on the owner's multi-project home.** The bare
   `vowp node ls --json` exits 2 there, and bare `vowp node show <node> --json` exits 2 with
   the same `invalid_input` “no project selected” message; the same commands with
   `--project <project>` exit 0. `spirit-core` now adds the active project to both reads. If no
   project is known, the gate is `Waiting` ("active project") and does not run a bare global node
   read. This is not fixed by owner enrollment.
2. **`--node` in help can false-positive on the global SSH selector.** `vowp orchestrator
   new --help` still shows `--node` as the global SSH redirect alongside the command's local
   meaning. The same trap exists on `vowp surface pair --help` and `vowp surface rotate --help`;
   a substring probe sees the flag and passes even if the local-node behavior is absent.
   Require the command-specific after-help sentence (“names the node that runs the app” /
   “names the node the surface runs on”, plus “not an SSH redirect”). The top-level `vowp
   surface --help` sentence is not enough for `surface pair`. Ambiguous output is `Waiting`,
   never `Ready`.
3. **`vowp node paths` is not a CLI command.** The old design sketch in
   `local-orchestrator.md` §12.1 lists it, but the shipped `vowp 0.4.0` `vowp-cli` has no such
   subcommand. The local cache is a **Mac-side** read from `vowp agent status --json`
   (`cache_entries`, `cache_bytes`, `cache_oldest_at`). Do not substitute
   `vowp agent paths ls --json` as home evidence and do not require `node paths`.
4. **An online home node is not an agent node.** A fresh 0.4.0 home's only node is
   `kind: home`, even when online: `status: active`, `presence: online`, `protocol: null`,
   `agent_version: null`. The live home adds an online agent card but still returns a
   `<home-node>` card that is `kind: home` and `offline`. Presence is computed for every
   node, so neither home card is an agent. The gate requires `kind == "agent"`, and the
   selected card must also carry `protocol: "vowp-agent/1"` and a non-empty
   `agent_version`. The scan must continue past the home card and select the agent card.
   The gate's node comes from `vowp node show`; the UI must not infer one from an absent
   `orchestrator ls` field.
5. **Help probes are weaker than a capability document.** If a help output is ambiguous, the
   gate is `Waiting` (or `Unknown`), never `Ready`.
6. **A non-connected local agent is not Ready.** `LocalAgentStatus` is displayed separately,
   but `LocalOrchestratorGate::Ready` is withheld until `vowp agent status --json` is
   `connected` and the local `agent pair|run|paths` helps and cache fields are present. A
   stopped, revoked, superseded, reconnecting, not-installed, or not-enrolled agent never
   produces a `Ready` gate.
7. **A synthetic `gate-phase3-ready` fixture is not live evidence.** The fixture proves that
   the gate card, capability list and disabled-action copy render. A separate D6 MockBridge
   scenario proves the enabled flow; neither is a live orchestrator.
8. **`vowp agent trust` is never a gate or an automatic action.** 0.4.0 adds
   `vowp agent trust --fingerprint <sha256:hex>` for a changed system-mode WSS leaf
   fingerprint. The owner must verify the new value out of band first; the command rewrites
   only the private `agent.json` and refuses SSH or pinned transports. StackSpirit probes only
   `vowp agent trust --help`, records it as optional evidence, never runs the command, and
   never copies a fingerprint from an error or status document. A trust refusal leaves the
   agent not `connected`, so the gate stays `Waiting` and the owner sees the redacted
   `last_error`.
9. **0.4.0 is the shipped baseline; older builds can still fail these probes.** A `0.3.0`
   binary lacks `surface rotate`, `agent service|doctor|trust`, and the `orchestrator new`
   local-node after-help. That is a missing capability, not a reason to relax a check or fall
   back to the stale `agent surface start|attach` spelling (which exists in neither build).
   The live agent card reports `agent_version: 0.3.0` while the home CLI is `0.4.0`; the
   gate accepts a present version, but the owner still needs to update the Mac agent and
   read the Mac-side `vowp agent status`. Do not call the card 0.4.0 agent evidence.

## 3. Owner setup steps (on `vowp 0.4.0+`)

Do these on `vowp 0.4.0+` on both the home and the Mac. Use VOWP's documented
install/upgrade path; this document does not prescribe an installer because the release method
is VOWP-owned. Values in `<...>` are placeholders, not secrets. Commands marked “write” change
local VOWP or agent state; they are owner actions, not part of this documentation job.

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
vowp agent trust --help | grep -F -- '--fingerprint'
vowp mcp --help | grep -F -- '--agent-socket'
vowp orchestrator new --help | grep -F -- 'not an SSH redirect'
vowp orchestrator stop --help
vowp orchestrator resume --help
vowp --project <project> offload status --json >/dev/null
vowp --project <project> orchestrator ls --json >/dev/null
```

Expected on `vowp 0.4.0+`: every help command exits 0. `surface pair` lists `--replace` and its
local-node after-help; `surface rotate` lists `--wait` and `--grace`; `agent pair`,
`agent run`, and `agent paths` list their subcommands/flags; `agent trust` lists
`--fingerprint`; `mcp --help` lists `--agent-socket`; `orchestrator new --help` matches
`not an SSH redirect`; and `orchestrator stop|resume` exist. A pre-0.4.0 binary can still
fail some of these rows; that is a missing capability, not a reason to fall back to the stale
`agent surface start|attach` design.

`vowp agent service`, `vowp agent doctor`, and `vowp agent trust` are optional. A foreground
`vowp agent run` works without service mode; missing doctor only removes a diagnostic; trust is
only needed after an owner-verified changed system-mode WSS fingerprint. Record either result
and do not treat a missing optional command as D6-ready or D6-blocking by itself. Never run
`vowp agent trust` from this checklist; it is an owner decision after an out-of-band check.

`vowp node enroll --replace` and `vowp node rm --yes` are owner-setup forms, not gate probes.
They are not part of the §2 `Ready` table, and their presence or absence changes no gate rule;
they only make a stale pending node or an explicit removal recoverable without inventing a flag.

Do **not** run `vowp surface pair`, `vowp surface rotate`, `vowp orchestrator new`, a real
`vowp agent run`, or `vowp agent trust` in this step; help and read-only probes only.

If the required commands or flags are still missing, the installed build is older than 0.4.0.
Stop and record the evidence; do not work around it.

### 3.2 On the home: create the pending node and one-time code (write)

```sh
vowp --project <project> node enroll <node> --label <label>
```

This creates a `pending` local-agent node and prints a one-time code **once**. The code is a
secret with a short TTL. Do not put it in a command argument, a shell variable assignment, a
log, a report, or this repository; clear the terminal scrollback if needed. Record only the node
name placeholder and the command exit code.

If the node already exists as `pending` (for example the code expired or was lost), recover it in
one transaction with `--replace`; a stale code is purged before the new one is issued:

```sh
vowp --project <project> node enroll <node> --replace [--label <label>]...
```

`--replace` refuses an `active` node and a `revoked` node. Resolve those explicitly first; they
are never replaced silently:

```sh
vowp --project <project> node revoke <node> --yes   # active node: revoke its credentials
vowp --project <project> node rm <node> --yes       # active or revoked node: remove it
```

`node rm --yes` removes a node without a confirmation prompt and also purges its enrollment
codes. `--replace`, `--yes`, `node revoke`, and `node rm` are owner setup, not gate evidence.

`--project` is the global project selector; on a multi-project home it selects the right project
store. The shipped `vowp 0.4.0` `node enroll --help` lists `--label` and `--replace`; use the
release's documented flags and do not invent one.

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
  not a live-action state), `protocol: "vowp-agent/1"`, `agent_version` present (the live
  2026-11-18 card reported `0.3.0`; update the Mac agent, but the gate requires a present
  version, not the 0.4.0 string), `mapped` includes the project, and `runs` is whatever it is.
- `offload status`: exit 0 with `nodes`, `assignments`, and `ready`; the local node's `presence`
  is `online` or `degraded`; no offline node is used for a live action.
- `surface ls`: exit 0. A pending/paired surface row is enough after pairing; do not record
  tokens, prefixes, or surface ids.
- `orchestrator ls`: exit 0. A local card may carry a `node` if the 0.4.0 build emits it; do
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
4. Confirm the derived feature flag is on. The expected post-owner-setup state is:

   - `local_orchestrator.state = "ready"`
   - `agent_protocol = "vowp-agent/1"`
   - `node` present (record as `<node>`)
   - all capabilities present: `node_card`, `offload_status`, `surface_pair`,
     `surface_list`, `surface_revoke`, `surface_rotate`, `mcp_agent_socket`,
     `orchestrator_new_node`, `orchestrator_lifecycle`, `orchestrator_read`,
     `local_agent_connected`, `local_surface_pair`, `local_agent_run`, `backfill`,
     `local_path_map`, `local_cache`
   - `local_agent.state = "connected"`
   - `features.local_orchestrator_actions = true` (derived from the three-part rule)
   - the **Run locally** action is enabled and the Local orchestrators panel shows the node,
     protocol, agent, lease holder and honest session states.

   Do not create or stop an orchestrator merely to prove the UI during this read-only
   step. A live action is owner-initiated and must still be refused if the gate, agent,
   node, protocol or path map changes.

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

## 4. What StackSpirit ships when D6 is ready

D6 is the local-orchestrator phase from `PLAN.md` §9.8. It is implemented in this build
behind the `Ready` + `connected` gate. When the gate is `Ready` **and**
`features.local_orchestrator_actions` is enabled, the Engine view and bridge expose:

- **Create and pair.** The Engine view creates or attaches a local orchestrator through
  VOWP's own commands: home-side `vowp surface pair` (`local_surface_pair`), Mac-side
  `vowp agent pair` (`local_agent_pair`), and `vowp orchestrator new --node <node> --app
  opencode` (`local_orchestrator_new`). `surface ls` / `surface rotate` / `surface revoke`
  own the surface token lifecycle (`local_surface_list`, `local_surface_rotate`,
  `local_surface_revoke`); `orchestrator ls` / `stop` / `resume` own the read model and
  lifecycle (`local_orchestrator_list`, `local_orchestrator_stop`,
  `local_orchestrator_resume`). `local_orchestrator_overview` combines the safe Mac agent
  status, the validated node card, path maps, surfaces and orchestrators.
  `vowp mcp --agent-socket` is the MCP proxy, and `vowp chat --agent --session <id>` is the
  chat surface. StackSpirit does not implement a parallel supervisor or a second session
  writer. There is no `vowp agent surface start` / `attach` / `detach` call in this build.
- **Surface ownership.** The local surface uses the StackSpirit Engine or the selected external
  harness. VOWP owns the canonical orchestrator record, session log, lease, and backfill.
  StackSpirit displays VOWP's state; it does not invent a running state.
- **Honest display.** The Engine panel shows the node, protocol, agent status, lease holder,
  and session state. Disconnected, degraded, reconnecting, revoked, superseded, and stopped
  states are visible and explain what is unavailable. A surface that is revoked, expired or
  not paired is shown read-only; `can_write` also requires the lease and an online node.
- **Feature-flag behavior.** The D6 desktop build advertises
  `features.local_orchestrator_actions`; `SpiritCore::set_gates` derives the published value
  from the three-part `local_orchestrator_enabled()` expression in §2.1. An older build with
  the flag off still shows “VOWP is ready; local orchestrator execution is not implemented in
  this build.” The backend re-checks the gate, connection, agent and flag on every D6 action;
  a stale renderer is not authority. There is no hidden force switch.
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

### 4.2 Implementation status in this build

`ss-21-d6-bridge-ui` wires the `ss-20-d6-core` backend through Tauri, the shared bridge and
the Engine view. The implementation is deliberately a thin adapter: no VOWP rule, probe,
guard, secret or path handling moved out of `spirit-core`.

| Layer | What is implemented |
|---|---|
| Tauri commands | `local_orchestrator_overview`, `local_agent_status`, `local_agent_pair`, `local_agent_paths`, `local_agent_path_add`, `local_agent_path_remove`, `local_surface_pair`, `local_surface_list`, `local_surface_rotate`, `local_surface_revoke`, `local_orchestrator_new`, `local_orchestrator_list`, `local_orchestrator_stop`, `local_orchestrator_resume` are registered in `lib.rs` and call `SpiritCore`. |
| Bridge | `SpiritBridge.localOrchestrators` is optional. `TauriBridge` remains the only renderer file importing `@tauri-apps/api`; `MockBridge` provides deterministic preview flows; the read-only Web bridge omits the surface and gets no local-orchestrator mutations. |
| Engine view | A **Local orchestrators** panel shows the gate, node, protocol, agent, lease holder, honest session states, Mac path maps, surfaces and orchestrators. Create, pair, stop, resume, rotate and revoke are enabled only when the gate is `Ready`, the local agent is `connected` and the action flag is on. Revoke and stop remain available as explicit cleanup while the gate is waiting. |
| Refusals | Every action surfaces the backend's exact prerequisite (`local_orchestrator_gate`, `local_agent_connection`, `agent_protocol`, `node_status`, `node_presence`, `local_path_map`, `local_orchestrator_actions`) in the panel. The renderer never retries with an old credential and has no force switch. |
| Pairing secret | The one-time pairing code is returned only by the direct `local_surface_pair` result so the owner can copy it once or redeem it locally. It is not in a view, an event, or a log; the preview fixture uses a placeholder, never a real code. |
| Tests | Vitest covers MockBridge refusals, cleanup, create/pair/rotate/revoke/stop/resume and the gated-off/Ready Engine panel. Playwright covers `Waiting`, `Incompatible` and `Ready` MockBridge flows, including the honest session states and WCAG 2.2 AA. |

Live execution remains blocked on the Mac half of the owner setup in §3: the installed
`vowp 0.4.0` CLI advertises the full surface and the live home now has an online `macbook`
agent card, but this Linux host cannot run the Mac-side `vowp agent status` and did not
verify `connection: connected` or the local cache. No owner write was performed. The UI and
backend behave honestly in that state by keeping the gate `Waiting` and every start action
disabled.

## 5. What not to do now

- **Do not fake enablement.** Never set `features.local_orchestrator_actions` by hand, add
  a hidden force switch, or enable a **Run locally** control from a stale renderer. The
  published flag is derived by `SpiritCore::set_gates` from the gate and agent state.
- **Do not bypass the gate.** The D6 actions are implemented, but every one must still
  refuse on a `Waiting`/`Incompatible`/`Unreachable` gate, a non-connected local agent, an
  incompatible protocol, a revoked node, or an invalid path map. The refusal names the
  prerequisite and offers no force path.
- **Do not run `vowp agent enroll`, `vowp agent run`, `vowp agent trust`, `vowp node enroll`, or
  `vowp agent paths add` as part of this job.** Those are owner actions after the VOWP release
  is installed; they write local/agent state. `vowp agent trust` additionally requires an
  out-of-band fingerprint check.
- **Do not start a second supervisor.** Do not run a second `vowp up` or a second
  `vowp agent run` to make a probe pass.
- **Do not open a VOWP database by hand.** Read only through the public CLI. Do not copy
  `vowp.db`, project DBs, session logs, or home secrets.
- **Do not copy a home secret, token, one-time code, key, or TLS fingerprint.** The node key
  proves the node; the surface token scopes the surface; the lease token authorizes the writer.
  No value crosses.
- **Do not claim a live orchestrator from a synthetic fixture.** `gate-phase3-ready` proves the
  disabled-action copy, not a running local orchestrator.
- **Do not treat the live home card as the whole gate.** The 2026-11-18 `macbook` card proves
  only the home-side agent node, protocol and version field. `vowp agent status` must run on
  the Mac and report `connection: connected`; the desktop re-probe must pass both halves.
  D6 actions stay disabled until then.
- **Do not treat Phase 1 commands as the local surface.** `vowp surface pair`, `vowp surface ls`,
  `vowp surface revoke`, `vowp node ls`, `vowp node show`, and `vowp orchestrator ls` exist
  before the full local host; they are necessary but not sufficient. `vowp surface rotate`,
  `vowp agent pair|run|paths|service|doctor|trust`, the local-node `orchestrator new` surface,
  and the Mac `agent status` cache fields complete the 0.4.0 surface. `service`, `doctor`, and
  `trust` remain optional evidence, never `Ready` gates.
- **Do not require the stale `vowp agent surface start` / `attach` / `detach` spelling.** It does not
  exist in the shipped source or the installed binary. The local app host is
  `vowp orchestrator new --node <node> --app opencode`.
- **Do not treat a version string as evidence.** `vowp 0.4.0` is recorded only with the
  command evidence in §1 and §2.

## 6. Unblock checklist

Keep this table current. If the installed CLI changes, re-run §1 and update the evidence rows and
this table. An owner action is not “done” until the named probe has the stated result. The
StackSpirit D6 code is implemented in this build; these rows unblock **live owner execution**,
not the UI.

| Missing or unproven | Action (owner or StackSpirit) | Exact probe that proves it | Current (2026-11-18) |
|---|---|---|---|
| D6 Desktop actions and honest Engine panel | Implemented in `ss-21-d6-bridge-ui`; no owner action. | `npm test`, `npm run test:e2e`, `cargo test -p spirit-core --features fixtures`, `cargo clippy -p spirit-core --all-targets -- -D warnings` | Implemented behind the `Ready` + `connected` gate; gated-off and Ready MockBridge flows covered. |
| Install the `vowp 0.4.0+` CLI on the home and Mac | Use VOWP's documented 0.4.0 install/upgrade path on both machines. Do not substitute a source checkout. | `vowp --version` → `vowp 0.4.0`; every §3.1 help probe exits 0 | Job host: installed `vowp 0.4.0`; every help probe passes. The live home card reports agent `0.3.0`, so the Mac binary still needs an owner check/update. |
| Surface lifecycle `surface rotate` and `surface pair --replace` | No owner action on 0.4.0; confirm the Mac binary is also 0.4.0. | `vowp surface rotate --help` → exit 0 with `--wait`/`--grace`; `vowp surface pair --help` → `--replace` present | Both present on the job host. |
| Active local-agent node with protocol `vowp-agent/1` | Home half is now present; the remaining action is the Mac enrollment/run below. | `vowp --project <project> node show <node> --json` → `kind: agent`, `status: active`, `presence: online`, `protocol: vowp-agent/1`, `agent_version` present | Live home (2026-11-18): `macbook` is `kind: agent`, `status: active`, `presence: online`, `protocol: vowp-agent/1`, `agent_version: 0.3.0`; `<home-node>` is `kind: home`, `presence: offline`. |
| Local agent state `connected` | Enroll and run the agent as above; keep it running. | `vowp agent status --json` → exit 0, `connection: connected`, `protocol: vowp-agent/1`, `last_heartbeat`, `cache_entries`, `cache_bytes` present | Temp agent home: exit 3, not enrolled. Live Mac not verified from this Linux host. |
| Mac agent binary version matches the home CLI | On the Mac, update the agent to `vowp 0.4.0+` and restart one `vowp agent run` (owner action). | Home card `agent_version`; Mac `vowp agent status --json` | Home card reports `0.3.0` while the home CLI is `0.4.0`. Owner update item, not 0.4.0 agent evidence. |
| Mac command surface `agent pair` / `run` / `paths` | No owner action on 0.4.0; confirm the Mac binary is also 0.4.0. | `vowp agent pair --help`, `vowp agent run --help`, `vowp agent paths --help` → exit 0 with their documented flags/subcommands | All exit 0 on the job host; no live pairing/run. |
| Phase 3 local-node meaning of `vowp orchestrator new --node` | No owner action on 0.4.0. Do not create an orchestrator yet. | `vowp orchestrator new --help` contains “names the node that runs the app” and “not an SSH redirect” | Both phrases present. |
| StackSpirit probe passes a project for `vowp node ls --json` and `vowp node show <node> --json` | Fixed in `ss-19-d6-probe-fix`; re-run the gate on a multi-project home. | Gate evidence includes `vowp node ls --json: exit 0` and a `vowp node show --json` card (or the documented `--project`-qualified commands) | Probe passes `--project`; bare commands exit 2, with `--project` they exit 0. |
| Orchestrator card carries the local `node` for display (optional) | Only after a local orchestrator exists, verify the card. The gate node comes from `node show`; this is not a reason to fake `Ready`. | `vowp --project <project> orchestrator ls --json` → exit 0 and the local card has `node: <node>` | Live home: exit 0, 6 cards, 1 running with `node: <agent-node>`; temp home: `[]`. |
| Optional `agent service` / `agent doctor` / `agent trust` | No owner action unless service mode, diagnostics, or a changed WSS fingerprint is wanted. Foreground `agent run` does not need service mode. | `vowp agent service --help`, `vowp agent doctor --help`, `vowp agent trust --help` → exit 0 (`trust` lists `--fingerprint`) | All exit 0; `trust` is never run by the probe. |

A `Ready` gate alone is not the finish line. The D6 build proves that the flag-off copy
is unchanged, the local agent’s non-connected states leave the gate `Waiting`, the path-map
validation refuses a bad root, and no home secret or absolute path appears in shared state.
The Playwright suite covers the `Waiting`, `Incompatible` and `Ready` MockBridge flows; the
Vitest suite covers the backend-style refusals and the honest Engine panel.

## 7. References

StackSpirit:

- [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md) §11.3 — local-orchestrator gate, probe algorithm,
  capability labels, security invariants, and the known contract gaps.
- [`docs/PLAN.md`](PLAN.md) §9.8 — D6 local-orchestrator scope; §8.5.7 — gate cards and feature
  flags; §9.1 — D6 is gated on VOWP Phase 2/3/4.
- [`docs/RELEASE-D5-ASIF.md`](RELEASE-D5-ASIF.md) §7.1 — D6 remained gated after D5; the D6 build now implements it behind the gate.
- [`docs/RELEASE-DESKTOP-MVP.md`](RELEASE-DESKTOP-MVP.md) §4 and §8 — D6 limitation and the
  D1–D4 feature-flag statement.
- [`docs/OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) — owner-run evidence template and sign-off
  rule; D6 is not an owner-verified behavior in this build.
- [`../README.md`](../README.md) — capability gates and the cross-project VOWP relationship.

VOWP 0.4.0, read-only (these files live in the companion VOWP repository and are not
modified here):

- `docs/release/0.4.0.md` — the shipped release notes, local agent nodes, surface rotation,
  optional WSS ingress, and ASIF 0.4 review-draft scope.
- `docs/design/v0.4/local-orchestrator.md` — §1.3 invariants, §4.2 node enrollment, §4.3 surface
  pairing, §5.7 local orchestrators, §6.2 path mapping, §8.6 WSS fingerprint changes, §10.2 no
  credential copying, §12 CLI surface, §14.1 phases. Note: §12.2 still shows the design-sketch
  `agent surface start|attach` spelling; the shipped `cli.rs` is authoritative and has no such
  command. §12.1 still lists `node paths`; the shipped CLI has no such subcommand.
- `docs/design/v0.4/hardening.md` §9.5 — the `v4-local-4f-cli-surface` scope, shipped in
  `v0.4.0`.
- `docs/guides/agent-node.md` §“Changed certificate fingerprint” — a changed system-mode WSS
  leaf fingerprint is verified out of band and recorded with
  `vowp agent trust --fingerprint`; pinned TLS requires re-enrollment instead.
- `crates/vowp-cli/src/cli.rs` — shipped command definitions for `node add|ls|rm|enroll|revoke|show`,
  `surface pair|ls|revoke|rotate`, `agent enroll|paths|pair|run|status|service|doctor|trust`, and
  `orchestrator new|ls|stop|resume`; `NodeEnrollArgs` carries `--replace`, `NodeRmArgs` carries
  `--yes`, and `AgentTrustArgs` carries `--fingerprint`.
- `crates/vowp-core/src/agent/enroll.rs` — `create_pending`, `replace_pending` (pending-only
  code rotation with an active/revoked refusal), and `apply_path_map` (lexical validation and
  the home-only `node_paths` row).
- `crates/vowp-core/src/agent/gateway.rs` — `AgentFrame::PathMapUpdate` handling (`Pong` on
  persist, typed `Error` without closing the session) and the log-and-ignore path for
  unexpected post-handshake frames.
- `crates/vowp-cli/src/cmd/surface.rs` — the home surface lifecycle and the
  `surface pair`/`rotate` local-node handling.
- `crates/vowp-cli/src/cmd/agent.rs` — `vowp agent status` exit-3 not-enrolled path, the status
  document, `agent pair`, `agent run`, `agent trust` (local-only, refuses SSH/pin transports),
  and the optional service/doctor commands.
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
- `crates/spirit-core/src/engine/local.rs` and `crates/spirit-core/src/service.rs` — the D6
  action guard, the derived feature flag and the action facade.
- `apps/spirit-desktop/src-tauri/src/commands/local_orchestrator.rs` and `lib.rs` — the Tauri
  command adapters and their handler registration.
- `apps/spirit-desktop/src/bridge/TauriBridge.ts` — the only renderer file importing
  `@tauri-apps/api`; it routes every D6 call to a command.
- `packages/spirit-ui/src/bridge/SpiritBridge.ts`, `wire.ts`, `MockBridge.ts` — the optional D6
  bridge surface, safe DTOs and the preview fixtures.
- `packages/spirit-ui/src/views/engine/LocalOrchestratorPanel.tsx` — the honest Engine panel
  with create/pair/stop/resume/rotate/revoke actions and prerequisite refusals.
- `packages/spirit-ui/src/test/engine/localOrchestrator.test.tsx` and
  `apps/spirit-desktop/e2e/local-orchestrator-*.spec.ts` — the D6 Vitest and Playwright
  coverage.
- `apps/spirit-desktop/src-tauri/src/state.rs` and `commands/gates.rs` — the build feature
  flags and gate probing entry points.
- `packages/spirit-ui/src/domain/engine.ts` and `views/engine/GateCard.tsx` — the flag-off copy
  and the disabled-action rendering.
