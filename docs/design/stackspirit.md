# StackSpirit — first draft product and execution plan

Status: first draft, 2026-11-14. Execution is gated on VOWP Phase 3 (local opencode and
local orchestrator). This document is the product plan, not an implementation job.

## 1. Product

StackSpirit is the visual local control surface for VOWP.

It lets a user add a VPS once, connect or disconnect with one action, and see and control
the work running there: workers, jobs, sessions, storage, capacity, providers and harnesses.
The VOWP home remains the source of truth on the VPS. StackSpirit runs on the user’s local
machine and can also host local orchestrator sessions and local harnesses.

One sentence:

> StackSpirit is the desktop cockpit for VOWP: add a machine once, connect, see the whole
> fleet, and run local or remote orchestration without learning the internals.

## 2. Non-goals for v1

- Do not build a second VOWP scheduler or database.
- Do not fork a full agent harness.
- Do not require inbound ports, a VPN or a relay.
- Do not store provider keys in the VOWP home or copy them from the local machine to the VPS.
- Do not build a remote desktop in the first version.
- Do not claim exact continuation, lossless ASIF import or native harness import.

## 3. Architecture

```text
StackSpirit desktop / CLI
  ├─ Host manager: add once, connect/disconnect, host-key pinning
  ├─ VOWP local node agent: outbound SSH only
  ├─ StackSpirit Engine: bundled opencode sidecar (default)
  ├─ Optional external harnesses: installed opencode, Claude Code, Codex/ACP later
  ├─ Provider onboarding: included opencode models or BYOK, OS keychain only
  ├─ Visual fleet: workers, jobs, sessions, storage, capacity, providers
  └─ Local web UI served on 127.0.0.1 for optional phone/LAN use

VOWP home on the VPS
  ├─ database, jobs, workers, leases and sessions
  ├─ global and project orchestrators
  ├─ capacity, storage, provider names and merge queue
  └─ source of truth; never a desktop DB copy
```

### Desktop

- Tauri 2 + React/TypeScript is the recommended starting point.
- Electron is an acceptable fallback if terminal/PTY speed is more important than app size.
- Use React Flow for the fleet/job graph, Recharts for capacity, and xterm.js only for
  session terminals.
- The app should feel like a control room, not an IDE.

### CLI

A `spirit` command with the same core API:

```text
spirit host add user@vps
spirit host connect <name>
spirit host disconnect <name>
spirit status
spirit dashboard
spirit session attach <id>
spirit job pause|retry <id>
spirit capacity set <n>
```

The CLI is the test harness for the desktop; both use the same API client.

### VOWP API

Extend the VOWP daemon with a small, authenticated JSON/SSE API:

- host/fleet snapshot
- workers and slots
- jobs and transitions
- sessions and last activity
- capacity target, advised target and recommendations
- disk/storage usage
- provider and harness inventory, with names only
- action endpoints for pause, retry, capacity set and session attach

No desktop process opens the VOWP SQLite database. The VPS remains the only writer.

### Connection

- One-time host enrollment over SSH.
- Pin the host key.
- Store only host metadata in StackSpirit; use the OS keychain for local secrets.
- No inbound SSH from VPS to desktop.
- Reconnect automatically; disconnected local sessions are marked paused/degraded, not
  silently completed.
- This builds on VOWP’s local-node Phase 0/1 primitives rather than inventing a second transport.

## 4. StackSpirit Engine and providers

### Default

Bundle a pinned opencode binary as the **StackSpirit Engine**.

- Users install only StackSpirit.
- StackSpirit starts, supervises, updates and stops the engine.
- VOWP’s local node points its opencode adapter at the bundled binary.
- The UI calls it “StackSpirit Engine”; opencode is named in About/third-party licenses.
- Users can optionally point StackSpirit at their own opencode installation.

### Models

- Offer opencode’s included/free models where available, subject to opencode’s terms,
  quotas and rate limits.
- Never promise “free forever.”
- Always offer “Bring your own provider key.”
- Provider keys stay in the local OS keychain or the local engine’s auth store.
- The VPS sees provider names and usage, never key values.
- Claude Code and Codex are optional external harnesses where installed/authenticated;
  they are not bundled and are not required for the default experience.

## 5. Visual model

- A VPS node at the centre.
- Worker slots around it, with busy/idle state.
- Jobs in running, waiting, blocked and done lanes.
- Capacity rail: target, advised target, CPU, RAM, disk, pressure.
- Storage panel: sessions, worktrees, free disk, retention.
- Provider/harness lanes: which executor/model is running where.
- A recommended-worker number derived from VOWP’s capacity advisor.
- A clear connection state and last-seen time.
- Download/upload actions for `.asif.zip` once ASIF integration is available.

## 6. Execution gate

Do not start UI/build execution until the VOWP local orchestrator path is usable.

Required gate:

1. VOWP Phase 2 offload/agent work is done.
2. VOWP Phase 3 local opencode path is done:
   - local app host
   - bridge
   - MCP proxy
   - orchestrator materialize/backfill
   - local cache
   - takeover and path normalization
3. A local `vowp chat`/opencode orchestrator can write the same session locally.
4. The local node can be selected as the placement for global and project orchestrators.

Current status at 2026-11-14 13:21 UTC:

- Phase 0: done (protocol, store, agent client, gateway).
- Phase 1: done (surface pair, lease core, local chat).
- Phase 2: in progress.
  - `v4-local-2e-offload-dispatch` is fixing.
  - `v4-local-2f-agent-run-loop` is reviewing.
  - `v4-local-2g-agent-path-map` is pending.
- Phase 3: not started. The local orchestrator/opencode app host is the next product goal.
- ASIF integration is being built separately; it does not unblock local orchestrator.

## 7. Execution plan after the gate

### S0 — contract and API

- Define the StackSpirit API and host/connection model.
- Add read-only VOWP endpoints.
- No UI yet.

### S1 — CLI host and read-only dashboard

- `spirit host add/connect/disconnect`.
- SSH host-key pinning.
- Fetch one snapshot.
- Local web dashboard with workers, jobs, capacity, storage and providers.

### S2 — Tauri desktop

- Host manager.
- Fleet canvas.
- Capacity/storage panels.
- Reconnect and disconnected state.
- Package for macOS first.

### S3 — engine and provider onboarding

- Bundle/pin opencode as StackSpirit Engine.
- Included/free-model selection with quota display.
- BYOK fallback and keychain storage.
- Detect external opencode/Claude Code; Codex later.
- No secret values on the VPS.

### S4 — local orchestrators

- Start global/project orchestrators on the local machine.
- Local surface, lease and session sync.
- Local file/shell access only through the local node.
- Disconnect/reconnect semantics.
- Same session data remains on VOWP home.

### S5 — actions and automation

- Capacity set.
- Pause/retry jobs.
- Session attach.
- Download/upload `.asif.zip`.
- Audit and confirmations for destructive actions.

### S6 — team/fleet

- Multiple VPS hosts.
- Optional read-only hosted web view.
- AgentDock/AgentVPS-style live takeover later.

## 8. Security and privacy

- Host-key pinning; no credentials copied from local to VPS.
- Provider keys local only; VOWP sees names, not values.
- Scoped action tokens and an audit trail.
- No direct DB access from the desktop.
- No inbound ports.
- No custom cryptography; use SSH and existing VOWP leases.
- Explicit confirmation for publish, deploy and destructive actions.

## 9. Success measures

- Add a VPS and reach the dashboard in under two minutes.
- Connect/disconnect in one click.
- A disconnected local run never reports as completed.
- A user can use the bundled engine without knowing opencode exists.
- A user can choose BYOK or an included/free model.
- The dashboard always shows the real VOWP target and recommended workers.
- No provider key or VOWP secret is ever present in the desktop’s logs or the VPS.

## 10. Open decisions

- macOS-only first, or macOS + Linux?
- Is StackSpirit open source, source-available or proprietary?
- Bundle opencode by default or require an optional download on first run?
- Which included/free models are acceptable under their terms?
- Should the local web UI be desktop-only or LAN-accessible by default?
- Do we expose Claude Code/Codex as supported external harnesses in v1 or v2?
