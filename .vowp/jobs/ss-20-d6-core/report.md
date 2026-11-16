---
job: ss-20-d6-core
status: done
branch: job/ss-20-d6-core
commits:
  - 55f588cbe9cdce371c3c4f8695f488565476bb27
  - a53e85af931930f1d71c17ea6e5f508eb9642922
  - 547c1eafcfb62a6e73fabb5842151e33599db73c
tests: "cargo fmt --all -- --check; cargo test -p spirit-core --features fixtures; cargo clippy -p spirit-core --all-targets -- -D warnings"
---

# ss-20-d6-core — D6 local-orchestrator backend

## Summary

Implemented the D6 local-orchestrator backend in `crates/spirit-core` without touching any
path outside `owns`.

### Gate and guard

- `features.local_orchestrator_actions` is derived by `GateSnapshot::enforce_local_orchestrator_flag`:
  it is true only for `LocalOrchestratorGate::Ready` with `LocalAgentState::Connected`, and
  false for Waiting, Incompatible, Unreachable, Unknown, Probing, NotInstalled, NotEnrolled,
  Connecting, Reconnecting, Revoked, Superseded, Stopped, or Incompatible agent states.
- `SpiritCore::set_gates` enforces that derivation, so a caller cannot publish a stale flag.
- `fixtures/vowp/gate-phase3-waiting.json` remains disabled; `gate-phase3-ready.json` now
  advertises the enabled D6 flag only because its gate is Ready and its agent is connected.
- Added `local_orchestrator_enabled(gate, connected)` plus `LocalOrchestratorGuard`, which
  re-checks the gate, agent connection, feature flag, protocol, target node, node enrollment
  and presence, and the project path map before every start action.
- Refusals use `SpiritErrorCode::Gate` and name the exact prerequisite
  (`local_orchestrator_gate`, `local_agent_connection`, `local_orchestrator_actions`,
  `agent_protocol`, `node_name`, `node_status`, `node_presence`, `local_path_map`).
  There is no force switch. Explicit cleanup (`surface revoke`, `orchestrator stop`) and
  read-only views remain available while the gate is Waiting so a broken node can be
  diagnosed and stopped.

### Typed DTOs and views

- Added `wire/local.rs`: `SurfaceEntry`, surface pair/rotate/revoke DTOs, `AgentStatusDocument`,
  `AgentSurfaceStatus`, `AgentPathMap`, `AgentPathAddRequest`, `AgentPairOutput`,
  `OrchestratorNewRequest`, `NodeCard`, and the `PathRef` type with `project:`, `worktree:`
  and `subdir:` validation.
- Added `view/local.rs`: `LocalAgentView`, `AgentPathView`, `LocalSurfaceView`,
  `LocalOrchestratorView`, and `LocalSessionState` with the honest failure states
  `disconnected`, `degraded`, `reconnecting`, `revoked`, `superseded`, `stopped`.
- Local surface views expose node, protocol, agent status, lease holder and session state.
  Revoked or expired surfaces are shown as such and are never writable from an old
  credential.
- `OrchestratorCard` now carries the merged optional `node`; the view displays it when
  present and never infers one when it is absent.

### Commands and service methods

- Home `VowpClient`: `surface pair`, `surface ls`, `surface rotate`, `surface revoke`,
  `orchestrator new|ls|stop|resume`, and `node show` (project-qualified where needed).
- Mac `LocalAgentClient`: `agent status`, `agent pair`, `agent paths ls|add|rm`, implemented
  over the injected local `CommandRunner`. Pairing codes travel on stdin with
  `--code-file -`; they never enter argv, a file, a log or shared state.
- `SpiritCore` (aliased as `SpiritService`) methods: `surface_pair`, `surface_list`,
  `surface_rotate`, `surface_revoke`, `agent_status`, `agent_pair`, `agent_paths`,
  `agent_path_add`, `agent_path_remove`, `orchestrator_new`, `orchestrator_list`,
  `orchestrator_stop`, `orchestrator_resume`, `local_surface_views`,
  `local_orchestrator_views`, plus node/path guard-state setters.

### Security boundaries

- No VOWP database is opened, no supervisor is started, and no parallel session writer exists.
- Surface pairing codes are held in `SecretString`, are absent from `Debug`, and have no
  `Serialize` path. `SurfacePairOutput` is `Deserialize`-only with a redacted `Debug`.
- Absolute Mac paths stay in Mac-local `AgentPathMap`/`AgentPathAddRequest` values; shared
  state uses `PathRef` only. `AgentPathMap` and `AgentPathAddRequest` debug output redacts
  local roots, and views expose only project names and `project:` refs.
- The agent status parser ignores the owner-only `paths` and key fingerprint, so neither can
  reach a view. Last errors are redacted and path-scrubbed before display.
- No retry logic exists for revoked nodes or expired surfaces.

## Tests added / updated

- New `tests/local_orchestrator.rs` covers:
  - flag off vs Ready + connected;
  - Waiting/Incompatible/non-connected/revoked-node/bad-path-map refusals;
  - no hidden retry after a refusal;
  - exact home/Mac command argv and JSON parsing;
  - pairing code on stdin and absent from argv/logs/Debug/serialization;
  - revoked/expired surface non-writability and honest view states;
  - `PathRef` rejection of absolute and traversing paths;
  - no secret or absolute-Mac-path leak in any serialized D6 view or guard state.
- Updated `tests/contract.rs` for the optional orchestrator `node`.
- Updated `tests/fixtures.rs` and `tests/gates.rs` for the derived D6 flag and the waiting
  fixture remaining disabled.

## Gates run

- `cargo fmt --all -- --check` — pass
- `cargo test -p spirit-core --features fixtures` — pass (all suites)
- `cargo clippy -p spirit-core --all-targets -- -D warnings` — pass

Only `crates/spirit-core/**` and this required job report were changed. No push was performed.
