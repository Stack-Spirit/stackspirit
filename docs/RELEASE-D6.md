# StackSpirit D6 — local-orchestrator release and closeout note

**Release baseline:** `main` at `1f82340` (2026-11-16). This note covers the D6 local-orchestrator
core, the Tauri commands and bridge, and the Engine UI.

**Release status:** D6 is merged and worker-verified only for the gated paths described here.
**Live D6 is not enabled and was not smoke-tested.** The installed VOWP CLI reports
`vowp 0.3.0`; the recorded readiness evidence has no active local-agent node and no recorded
connected local agent. A synthetic `Ready` gate fixture and a MockBridge scenario are not a live
orchestrator. This job did not create, pair, start, stop, resume, rotate, or revoke any live
surface or orchestrator, did not start an agent, and did not run a live session turn.

## 1. Scope shipped

### 1.1 Shipped surfaces and evidence

| Surface | Scope in this release | Commit(s) |
|---|---|---|
| **D6a `spirit-core`** | Local-orchestrator wire types and fixtures, the home and Mac probes, `LocalOrchestratorGate` / `LocalAgentStatus`, the action guard, the derived feature flag, and honest surface/orchestrator/agent/path-map views. | `55f588c` (implementation), `a53e85a`, `547c1ea`, `6b54da3` (node scoping and path scrubbing); tests `ebf8603` |
| **D6b bridge and Tauri commands** | The `local_orchestrator_*` and `local_agent_*` Tauri commands, `SpiritBridge.localOrchestrators`, the `TauriBridge` routes, safe DTOs, MockBridge scenarios, and the read-only Web omission. | `133fd25` |
| **D6c Engine UI** | The **Local orchestrators** panel (gate, node, protocol, agent, lease, session state, path maps, surfaces, orchestrators), enabled/disabled action handling, prerequisite refusals, and the D6 Playwright specs. | `133fd25` (panel and specs), `1f82340` (e2e harness cleanup) |

The shipped D6 code/test range is `55f588c` through `1f82340` (7 commits; the range also contains
two job-report housekeeping commits, `f72ae2e` and `37ef034`, which changed no product code). The
gate/readiness contract landed in the preceding `ss-20-d6-contract-reconcile` commits
`064f80a` and `7c5e2ae`; the detailed contract is `docs/VOWP-CONTRACT.md` §11.3.

The core work is concentrated in `crates/spirit-core/src/wire/local.rs`,
`src/vowp/agent.rs`, `src/vowp/local.rs`, `src/vowp/client.rs`, `src/engine/gate.rs`,
`src/engine/local.rs`, `src/service.rs`, and `src/view/local.rs`, with fixture documents under
`crates/spirit-core/fixtures/vowp/`. The desktop side is
`apps/spirit-desktop/src-tauri/src/commands/local_orchestrator.rs` plus its handler registration
in `lib.rs`, and `packages/spirit-ui/src/views/engine/LocalOrchestratorPanel.tsx` with the bridge
types in `packages/spirit-ui/src/bridge/`.

### 1.2 Versions

| Item | Value | Source |
|---|---|---|
| Release baseline | `1f82340` | `git rev-parse HEAD` at the start of this job |
| `spirit-core` crate | `0.1.0`; workspace MSRV `1.85` | `crates/spirit-core/Cargo.toml`, root `Cargo.toml` |
| Tauri Rust crate | `2.12.5` | `Cargo.lock` |
| `@tauri-apps/api` | `2.12.4` (workspace range) | `package-lock.json` |
| Installed VOWP CLI | `vowp 0.3.0` | `vowp --version` |
| Merged VOWP CLI source | `v4-local-4f-cli-surface` at `121f319`, follow-up `d523314` | `docs/D6-READINESS.md` §1.3 |
| Host re-run toolchain | `cargo 1.99.1`, `node 24.21.0`, `npm 11.19.0` | Commands in §4 |

`vowp 0.3.0` is the CLI version, not a capability claim. The VOWP source still reports `0.3.0`,
so a build containing the merged Phase 4 surface can look identical to an older binary in
`vowp --version`. The gate uses command, flag, help, node-card, and agent-status evidence, never
the version string alone.

### 1.3 Evidence provenance

The dependency job reports named in this job's brief
(`.vowp/jobs/ss-21-d6-core-r3/report.md` and `.vowp/jobs/ss-21-d6-bridge-ui/report.md`) were not
present in the workspace when this note was written; only their briefs were available. This note
therefore does not quote gate results from them. Every **worker-verified** row in §4 is a command
re-run on this baseline in this job, and the merged commits in §1.1 are the product evidence.
**Owner-run/live** rows remain unexecuted and are marked as such.

## 2. Gate semantics

D6 does not use a version switch, a build constant alone, or a renderer flag. A start action is
enabled only when **all** of the following are true at the backend:

```text
features.local_orchestrator_actions
    && LocalOrchestratorGate == Ready
    && LocalAgentStatus.state == connected
```

`SpiritCore::set_gates` derives the published flag from that expression. The desktop command
boundary overlays the build's capability flags and then re-checks the same expression, so a stale
renderer cannot enable a D6 action by itself. There is no hidden force switch and no fallback to
the stale `vowp agent surface start|attach|detach` spelling (it exists in neither the merged CLI
nor the installed binary).

### 2.1 Readiness is a two-half capability probe

`LocalOrchestratorGate=Ready` requires both halves; neither half is sufficient alone.

- **Home half (over SSH).** A selected project; `vowp --project <project> node ls --json`;
  `vowp --project <project> node show <node> --json` with `kind: "agent"`, `status: "active"`,
  and `presence: "online"`; protocol `vowp-agent/1` and a non-empty agent version;
  `vowp offload status --json` with the documented `nodes`, `assignments`, and `ready` arrays;
  the `surface pair|ls|revoke|rotate` help surface (including `--replace`, `--wait`, `--grace`,
  and the command-specific local-node after-help); `vowp mcp --help` with `--surface` and
  `--agent-socket`; and the local-node `orchestrator new|ls|stop|resume` surface. Help alone is
  weak evidence and the global `--node` SSH selector is not accepted as proof of local-node
  behavior.
- **Mac half (local `CommandRunner`, never over SSH).** `vowp agent status --json` must report
  `connection: "connected"` and protocol `vowp-agent/1`, with `last_heartbeat`, `cache_entries`,
  and `cache_bytes`; the local `agent pair`, `agent run`, and `agent paths` help surfaces must be
  present. `vowp agent status` is a Mac-side command and is never run against the home.

`LocalAgentStatus` is displayed separately so the owner can see why the Mac half fails. A local
agent that is not installed, not enrolled, connecting, reconnecting, revoked, superseded,
stopped, or incompatible leaves the gate `Waiting`/`Incompatible`, never `Ready`; only
`connected` leaves it eligible. Missing or ambiguous help output stays `Waiting`/`Unknown`; a
malformed present document is `Incompatible`. `degraded` presence is honest but is not a
live-action state; `offline` is never `Ready`. The full probe table and state mapping are in
`docs/D6-READINESS.md` §2 and `docs/VOWP-CONTRACT.md` §11.3.4, not duplicated here.

### 2.2 The action guard

Actions that start or restart a local surface require `Ready`: `surface_pair`, `surface_rotate`,
`agent_pair`, `orchestrator_new`, and `orchestrator_resume`. `orchestrator_new` and
`orchestrator_resume` also require a valid path map for the bound project. Read-only actions
(`surface_list`, `orchestrator_list`, `agent_status`, `agent_paths`) and explicit cleanup
(`surface_revoke`, `orchestrator_stop`) remain available while the gate is waiting so a broken or
revoked local state can be inspected and stopped; they still go through the backend.

On every start action the guard re-checks, in order: the gate, the local agent connection, the
build feature flag, the protocol, the node, and (for project-bound actions) the path map. A
refusal names the exact prerequisite (`local_orchestrator_gate`, `local_agent_connection`,
`local_orchestrator_actions`, `agent_protocol`, `node_name`, `node_status`, `node_presence`, or
`local_path_map`). A card must match the **gate's validated node**: the stored node card is
evidence only when its name equals the gate node, and the requested target node must equal that
same node. A stale card from another connection does not authorize an action.

### 2.3 Node scoping: no borrowed agent state

A home-hosted or other-node card must not borrow the Mac agent's state. `agent_describes_orchestrator`
applies the Mac agent status only when the card names a node and the local node row and/or the
agent's node names the same node. A `node: None` orchestrator is a v0.2 home-hosted card, not a
local one; a card for another node likewise does not inherit the Mac agent's connected, revoked,
lease, or session state. Its session state stays honestly unknown/disconnected. For surfaces,
`can_write` requires the agent to describe that surface's node, a connected agent, a paired
surface state, the current lease, and an online node. Revoked and expired surfaces, and offline
nodes, are non-writable even if a lease token is still present.

### 2.4 No version-only readiness, no sticky Ready

`vowp --version`, a `vowp node ls` exit 0, an `orchestrator ls` exit 0, a `gate-phase3-ready`
fixture, or a MockBridge `Ready` scenario cannot make the gate `Ready` by themselves. The gate
cache is per host connection and expires; a disconnect, a later failed probe, or a connection
change downgrades it immediately. Actions re-check the cached snapshot without a new probe, so a
cached `Ready` lasts until expiry or invalidation and refreshes on connect or **Probe gates**; the UI requires a successful probe.

## 3. Current live state (2026-11-16)

**Live D6 is not enabled and was not smoke-tested.**

| Observation | Result | Meaning |
|---|---|---|
| `vowp --version` | `vowp 0.3.0` | The CLI runs; version alone is not evidence. |
| Recorded readiness probe (`docs/D6-READINESS.md` §1.2) | Installed `0.3.0` lacked `surface rotate`, `agent service/doctor`, and the local-node `orchestrator new` after-help | The recorded installed build did not satisfy the Phase 4 surface. |
| Read-only re-check in this job | Help for `surface rotate`, `surface pair --replace`, `agent service/doctor`, and the local-node `orchestrator new` after-help is present on the job host | The recorded row and this re-check disagree; treat the installed-binary row as stale and re-record it. This is **probe drift, not readiness**; the remaining blockers are the node and agent rows below. |
| Home node card (`vowp --project <project> node show <node> --json`) | `kind: home`, `status: active`, `presence: offline`, `protocol: null`, `agent_version: null` | There is still no active local-agent node. |
| `vowp agent status --json` on the job host | Exit `3`, `not_found`: agent is not enrolled | No enrollment on this host. This is a Mac-side command, so this result is not home evidence and is not proof about the owner's Mac. |
| Live surface / orchestrator / agent action | None executed | No live action, no live session, no live MCP proxy, and no live orchestrator. |

The only product claims supported by this state are the honest gated paths: a `Waiting`,
`Incompatible`, or unreachable gate disables every start action and explains the missing
prerequisite. From this state the derived `features.local_orchestrator_actions` value is
`false`, even though the D6 build advertises the capability. A synthetic `Ready` fixture or a
MockBridge preview proves the enabled/disabled rendering, not a running local orchestrator. The
installed build is **not** a live orchestrator, and this note does not describe the merged source
as one.

No `vowp node enroll`, `vowp agent enroll`, `vowp surface pair`, `vowp orchestrator new`,
`vowp agent paths add`, or `vowp agent run` was executed by this job. No VOWP database was opened
by hand. No owner credential, pairing code, token, or session was read or written.

## 4. Tests and evidence

### 4.1 Re-run on this baseline

| Check | Exact command | Result | Status |
|---|---|---|---|
| Core format | `cargo fmt --all -- --check` | Exit `0`; no output. | **worker-verified** |
| Core full suite | `cargo test -p spirit-core --features fixtures` | `243 passed; 0 failed` across 13 suites plus doc-tests. | **worker-verified** |
| D6 core fixture/guard tests | `cargo test -p spirit-core --features fixtures --test local_orchestrator` | `29 passed; 0 failed`. | **worker-verified** |
| D6 gate/refusal tests | `cargo test -p spirit-core --features fixtures --test gates gates_phase3` | `21 passed; 0 failed`; `10` other gate tests filtered out. | **worker-verified** |
| Fixture secret scan | `cargo test -p spirit-core --features fixtures --test contract no_fixture_contains_a_secret_pattern` | `1 passed; 0 failed`. | **worker-verified** |
| D6 bridge tests | `npx vitest run src/bridge/localOrchestrator.test.ts` (in `packages/spirit-ui`) | `6 passed; 0 failed`. | **worker-verified** |
| D6 Engine panel tests | `npx vitest run src/test/engine/localOrchestrator.test.tsx` (in `packages/spirit-ui`) | `5 passed; 0 failed`, including one axe check. | **worker-verified** |

The D6 core suite covers the feature-flag derivation, each exact prerequisite refusal, the absence
of a force switch, the private pairing-code path, surface revocation cleanup, path-map refusal,
revoked/expired surface non-writability, offline-node write refusal, node matching, the
home-hosted/other-node no-borrow rule, quoted-path scrubbing, and fixture parsing. The gate suite
covers the two-half probe, the command-specific local-node after-help checks, the project-qualified
node reads, and the never-version-only/never-home-only rules.

### 4.2 Committed but not re-run in this documentation job

| Check | Evidence | Status |
|---|---|---|
| D6 Playwright/axe, gated flows | `apps/spirit-desktop/e2e/local-orchestrator-gated.spec.ts` (Waiting and Incompatible; 2 axe checks) | **committed at `133fd25`/`1f82340`; not re-run here** |
| D6 Playwright/axe, Ready flow | `apps/spirit-desktop/e2e/local-orchestrator-ready.spec.ts` (honest Ready flow; 3 axe checks) | **committed at `133fd25`/`1f82340`; not re-run here** |
| Full desktop e2e and Web regression | `npm run test:e2e` | **not re-run here** |
| Tauri host tests | `cargo test -p spirit-desktop` / `scripts/desktop-build-container.sh check` | **not re-run here** |
| Live VOWP/agent/desktop smoke | Real home + Mac + enrolled agent + real surface | **owner-run/pending** |

### 4.3 What these tests do and do not prove

- The core tests use `FakeCommandRunner`, fixture documents, and synthetic node/agent cards. They
  prove argv construction, help parsing, state mapping, guard ordering, path-map refusal, node
  matching, and view scrubbing. They do not prove a real SSH host, a real `vowp` binary, or a real
  Mac agent.
- The bridge and Engine panel tests use MockBridge scenarios. They prove refusals, reads that stay
  available while waiting, cleanup, create/pair/rotate/revoke/stop/resume requests, and the
  disabled/enabled panel copy. They do not prove a native Tauri invoke, a native dialog, or a live
  orchestrator.
- The Playwright specs are MockBridge-only preview flows. They include axe checks, which complement
  but do not replace an owner GUI/assistive-technology pass.
- The fixture secret scan is pattern-based (PEM/key/token shapes). It is not a live secret audit
  and does not inspect an exported package or a running VOWP home.
- A `gate-phase3-ready` fixture or a MockBridge `Ready` scenario is synthetic evidence. Neither is
  a live local orchestrator.

## 5. Owner setup

The detailed commands, expected fields, and redaction rules are in
[`docs/D6-READINESS.md`](D6-READINESS.md) §3. The short sequence is:

1. **Install a VOWP build containing the merged Phase 4 CLI surface on the home and the Mac**,
   using VOWP's own install path. Verify the installed build with the help/flag probes in
   `D6-READINESS.md` §3.1 (`surface pair --replace`, `surface rotate`, `agent pair|run|paths`,
   `agent service|doctor` optional, `mcp --agent-socket`, and the local-node
   `orchestrator new` after-help). Do not use the source checkout as installed evidence.
2. **Enroll the local agent.** On the home, create the pending node and one-time code with
   `vowp --project <project> node enroll <node> --label <label>`; on the Mac, redeem it with
   `vowp agent enroll --target <home-ssh-target> --code-file <code-file>`. The code is a
   short-lived secret: keep it in a private file, never in argv, a shell variable, a log, a report,
   or this repository.
3. **Map project paths on the Mac** with `vowp agent paths add --project
   <absolute-local-checkout> --name <project> --worktree-root <worktree-root>`, then confirm
   `vowp agent paths ls --json`. Paths stay on the Mac and are never copied into a report or
   shared view.
4. **Run one local agent** with `vowp agent run --foreground --role both` (or a supported service
   mode instead, never both), and leave exactly one agent running. Do not start a second
   supervisor or a second `vowp agent run`.
5. **Verify the two halves**: on the home, `vowp --project <project> node show <node> --json`
   should show an active agent node with `presence: online` and protocol `vowp-agent/1`; on the
   Mac, `vowp agent status --json` should show `connection: connected` with the cache fields and a
   heartbeat. Record only exit codes and the documented safe fields.
6. **Re-run the StackSpirit gate.** Connect the desktop to the home through the normal host
   connect flow, open **Engine**, and use **Re-probe** on the **Local orchestrator** card. Expect
   `LocalOrchestratorGate = Ready`, `LocalAgentStatus = connected`, and the derived
   `features.local_orchestrator_actions = true`. A still-waiting or incompatible card must be
   recorded with its exact `missing`/`detected`/`required`/`evidence` fields; do not edit code,
   force the flag, or start a hidden agent to make it green.
7. **Only then** perform an owner-initiated live action, and record its result. A successful
   re-probe alone does not prove a surface or turn is running.

No real host, token, code, secret, session id, or absolute path belongs in this note or in any
committed evidence. This job performed none of the write steps above.

## 6. Safety and limitations

- **One home.** The Mac never opens a VOWP database and never keeps a second session log. Pending
  local events are an outbox, not a log.
- **One lease.** At most one surface writes a session. A local-surface write needs a valid surface
  token **and** the current lease token; a surface without the lease is read-only.
- **No second supervisor.** Exactly one `vowp agent run` (foreground or service mode) and one
  supervisor. StackSpirit does not implement a parallel supervisor, session writer, or hidden
  retry with an old credential.
- **No credential copying.** The node key proves the node, the surface token scopes the surface,
  and the lease token authorizes the writer. No home secret, provider key, harness login, or SSH
  private key crosses the boundary. VOWP receives provider/model names only.
- **Pairing codes stay off argv and logs.** The one-time code is returned only by the direct
  pairing call, is not put in a view, event, or log, and is redeemed from a private code file (or
  stdin) on the Mac. The preview fixture uses a placeholder, never a real code.
- **No absolute local paths in shared state.** New events, run rows, and snapshot cards use
  `PathRef` (`project:`, `worktree:`, `subdir:`); absolute Mac paths stay on the Mac, and quoted
  paths in an agent error are scrubbed before display.
- **Revoked/expired is non-writable.** A revoked node fails the next handshake; revoked or expired
  surfaces and offline nodes are read-only even if an old token or lease is still present.
- **No root and no forced state.** The local agent never runs as root and the app never asks for
  root. Missing, ambiguous, offline, revoked, superseded, stopped, reconnecting, or incompatible
  evidence stays `Waiting`/`Incompatible`; there is no version-only readiness and no force switch.
- **Not live-verified.** The merged code, the fixtures, and the MockBridge preview are not a live
  orchestrator. Live D6 is blocked on the owner setup in §5 and has not been smoke-tested by the
  owner or by this job.

## 7. References and follow-ups

- [`docs/D6-READINESS.md`](D6-READINESS.md) §2–§3 — the detailed probe table, local-agent state
  mapping, owner setup commands, evidence fields, and the §6 unblock checklist. §1.2's
  installed-binary table predates this job's read-only re-check and should be refreshed by a
  follow-up docs change; that file is outside this job's write scope.
- [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md) §11.3 — local-orchestrator contract, capability
  labels, probe algorithm, and security invariants; §11.4–§11.6 — gate serialization, cache, and
  UI rules.
- [`docs/PLAN.md`](PLAN.md) §9.8 — D6 scope and its VOWP Phase 2/3/4 prerequisites.
- [`docs/RELEASE-D5-ASIF.md`](RELEASE-D5-ASIF.md) §7 item 1 — D5's statement that D6 remained gated;
  this note records the D6 code landing behind the same gate.
- [`docs/RELEASE-DESKTOP-MVP.md`](RELEASE-DESKTOP-MVP.md) — the D1–D4 release baseline and the
  original D6 limitation.

Open follow-ups: re-record the D6-READINESS installed-binary evidence after this job's re-check;
run the owner live smoke in §5; and keep the D6 release note from claiming `Ready`, live execution,
or owner verification until that smoke is recorded.
