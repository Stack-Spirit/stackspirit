> **Status: historical origin design note (2026-11-14), migrated from the private `vowp` checkout.**
> This document is preserved for historical context only. For active implementation details use
> [`docs/PLAN.md`](PLAN.md) and [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md), which supersede this
> note wherever they differ.

# StackSpirit — Detailed Product, Architecture & Desktop-First Execution Plan

Status: updated 2026-11-14. Project started at `/home/dev/stackspirit` (`stackspirit` VOWP project).
All roles (`planner`, `worker`, `verifier`, `keeper`) run on
`opencode:deepseek/deepseek-flash#max` (**DeepSeek V4.1 Flash 1017** via the `deepseek` provider
with `max` reasoning).

---

## 1. Product Vision & Surface Order (Desktop First)

StackSpirit is the visual local control plane and desktop cockpit for VOWP.

**Surface build order (explicit owner decision):**

1. **Desktop App FIRST (`spirit-desktop` + `spirit-core` + `spirit-ui`)**
   - Native **Tauri 2** desktop application (Rust backend + React/TypeScript UI).
   - Focuses on the core UX first: add a VPS once, click Connect or Disconnect, and immediately
     visualize the VPS fleet, active workers, job lanes, VPS disk/storage limits, capacity
     advisor recommendations, active harnesses/providers, and sessions.
   - Bundles/manages `opencode` under the hood as the **StackSpirit Engine** so users do not need
     to install or know about a separate harness, while offering opencode's free/included models
     and Bring-Your-Own-Key (BYOK) via the OS keychain.
2. **Web App SECOND (`spirit-web`, after Desktop MVP)**
   - Reuses the exact same `spirit-ui` React/TypeScript components by swapping the `TauriTransport`
     bridge for an `HttpSseTransport` talking to `vowp-daemon`.
3. **CLI THIRD (`spirit-cli`, after Desktop MVP)**
   - Thin Rust CLI (`spirit`) built on top of the exact same `spirit-core` Rust crate created for
     the Tauri desktop backend.

Why this works cleanly without rework:
- The Tauri 2 desktop backend (`src-tauri`) delegates all non-UI logic to ` crates/spirit-core`
  (Rust). When it is time to build the CLI later, `spirit-cli` wraps `spirit-core`.
- The Tauri 2 frontend (`ui/`) talks to the backend strictly through a typed `SpiritBridge`
  interface. In Desktop, `TauriBridge` implements `SpiritBridge` over Tauri IPC. When it is time
  to build the Web App later, `WebBridge` implements `SpiritBridge` over HTTP/SSE.

---

## 2. How StackSpirit & VOWP Rely on Each Other (Cross-Project Dependency Matrix)

Although `stackspirit` (`/home/dev/stackspirit`) and `vowp` (`/home/dev/vowp`) are separate VOWP
projects and repositories, **StackSpirit relies on VOWP wire contracts and milestones**:

| StackSpirit Phase / Feature | Can start now? | VOWP Prerequisite in `/home/dev/vowp` | How StackSpirit handles the dependency |
|---|---|---|---|
| **D0: Detailed repo architecture & wire contract map** | **YES (Now)** | Existing VOWP v0.3/v0.4 `snapshot.json`, `vowp fleet status --json`, `vowp capacity report --json`, `vowp job`, `vowp session` | Reads `/home/dev/vowp/crates/vowp-protocol` and `docs/design/v0.4/local-orchestrator.md` read-only to mirror exact JSON schemas. |
| **D1: `spirit-core` Rust backend (Host store, SSH runner, VOWP snapshot client, mock fixtures)** | **YES (Now)** | Existing VOWP CLI JSON outputs over SSH (`vowp fleet status --json`, `vowp capacity report --json`, `vowp fleet queue --json`, `vowp fleet inbox --json`) | Uses outbound SSH + deterministic offline fixtures from real VOWP JSON schemas so UI work is 100% testable offline and against a live VPS. |
| **D2: `spirit-desktop` Tauri 2 shell, design system & "Add VPS / Connect / Disconnect" Host Manager** | **YES (Now)** | Existing SSH access + VOWP Phase 0/1 node enrollment (`v4-local-0-*` and `v4-local-1-*`, **already DONE** in `vowp`) | Connects to a VPS in one click, verifies host status, pins host metadata locally, and shows live connection state. |
| **D3: Visual Fleet Canvas, Worker Slots, Capacity & VPS Storage Dashboard** | **YES (Now)** | Existing VOWP `CapacityReport`, `HostSample` (disk free, RAM, CPU pressure, load), `Target` (fixed vs advised recommended workers), active sessions/providers | Renders the visual node graph, worker slot rings, running/waiting/blocked job lanes, VPS storage/disk gauges, recommended worker count, and active harness/provider badges. |
| **D4: StackSpirit Engine (`opencode` sidecar) & Provider/Model Onboarding UI** | **YES (Now — local engine & UI)** | Local `opencode` binary + OS keychain | Manages the local `opencode` binary as "StackSpirit Engine", configures included/free models or BYOK keys in the local OS keychain, and detects optional local `opencode` / `claude-code` installs. |
| **D5: Portable Session `.asif.zip` Download & Import in Desktop UI** | **GATED on VOWP ASIF jobs** | Requires VOWP `v4-asif-core`, `v4-asif-cli`, `v4-asif-web` (currently queued/ready in `vowp`) | Desktop UI builds the session drawer now; live one-click `.asif.zip` export/import wiring waits until `v4-asif-cli` / `v4-asif-web` merge in `/home/dev/vowp`. |
| **D6: Running Global & Project Orchestrators Locally on the Mac/Desktop** | **GATED on VOWP Phase 2 & 3** | Requires VOWP Phase 2 (`v4-local-2e/2f/2g`, currently finishing) **AND** VOWP Phase 3 (`v4-local-3*`: local opencode app host, bridge, MCP proxy, orchestrator materialize/backfill) | **Hard gate:** Do not claim or enable live local-orchestrator execution in StackSpirit until VOWP Phase 3 merges in `/home/dev/vowp`. Until then, orchestrator sessions attach to the VPS host or run in preview/mock mode with a clear gate badge. |
| **W1 / C1: Standalone Web App (`spirit-web`) & CLI (`spirit-cli`)** | **After D1–D4** | Reuses `spirit-ui` and `spirit-core` after the Desktop MVP is complete | Built after the desktop app UX and core crates are proven. |

---

## 3. Language & Technical Stack

- **Systems / Desktop Backend / Future CLI:** **Rust** (2024 edition)
  - Shares exact mental model and serde JSON contracts with VOWP (`/home/dev/vowp/crates/vowp-protocol`).
  - Powers `crates/spirit-core` and `apps/spirit-desktop/src-tauri`.
- **Desktop Shell:** **Tauri 2**
  - Native lightweight windowing, small binary, low idle memory, native Rust command bridge for
    SSH, OS keychain, and supervising the bundled `opencode` sidecar process.
- **Desktop UI (and later Web UI):** **React 19 + TypeScript + Vite**
  - **Fleet & Worker Graph:** interactive node/lane canvas showing the VPS root node, worker slots
    (idle/busy/verifier), job cards by state, and provider/model badges.
  - **Capacity & Storage Telemetry:** visual gauges for VPS disk free (`disk_free_home_bytes`),
    minimum reserve floor (`min_disk_bytes`), RAM available/p90, CPU load/pressure, current worker
    target vs VOWP's **advised (recommended) workers**, and brake status.
  - **Terminal / Session Stream:** structured event/turn viewer plus terminal view for live session
    inspection.

---

## 4. Workspace Layout (`/home/dev/stackspirit`)

```text
stackspirit/
├── Cargo.toml                  # Rust workspace (spirit-core, src-tauri, later spirit-cli)
├── crates/
│   └── spirit-core/            # Rust core: VOWP snapshot/capacity/job types, SSH client,
│                               # host profile store, OS keychain trait, opencode sidecar manager,
│                               # and deterministic mock VOWP fixtures
├── apps/
│   └── spirit-desktop/         # Desktop-first application
│       ├── package.json        # React + TypeScript + Vite frontend (spirit-ui)
│       ├── src/                # UI components, SpiritBridge abstraction, views & tests
│       └── src-tauri/          # Tauri 2 Rust host wrapping spirit-core
└── docs/
    ├── PLAN.md                 # Detailed execution plan & VOWP cross-project gates
    └── VOWP-CONTRACT.md        # Exact mapping to /home/dev/vowp JSON outputs & Phase 3 gates
```

---

## 5. Desktop-First Phased Job Graph (`stackspirit` project)

Every worker in `stackspirit` runs on `opencode:deepseek/deepseek-flash#max` (DeepSeek V4.1 Flash
1017 on the `deepseek` provider with `max` reasoning).

### Phase D0 — `ss-0-plan` (Architecture, VOWP Contract Map & Desktop-First Spec)
- Reads `/home/dev/vowp/docs/design/stackspirit.md`, `/home/dev/vowp/docs/design/v0.4/local-orchestrator.md`,
  `/home/dev/vowp/docs/design/v0.4/asif-integration.md`, and `/home/dev/vowp/crates/vowp-protocol/src/`.
- Writes `docs/PLAN.md` and `docs/VOWP-CONTRACT.md` inside `/home/dev/stackspirit`.
- Freezes the exact Rust and TypeScript types for VOWP snapshots, capacity/storage metrics,
  recommended worker counts, host connection profiles, StackSpirit Engine (`opencode` sidecar)
  states, and the explicit VOWP Phase 3 / ASIF dependency gates.

### Phase D1 — `ss-1-core` (`crates/spirit-core`: VOWP Client, SSH Bridge, Host Store & Engine Manager)
- Implements `crates/spirit-core` in Rust:
  - Typed models matching VOWP's `fleet status --json`, `capacity report --json`, `fleet queue --json`,
    and `fleet inbox --json`.
  - Host connection store (`HostProfile`: label, SSH target, port, home path, last connected,
    pinned host key fingerprint—never storing secrets in plaintext files).
  - Outbound SSH command/tunnel runner (`ssh -o BatchMode=yes ... vowp fleet status --json`) plus a
    `MockVowpBackend` with realistic multi-project fixtures (`vowp`, `northnoir`, `vowp-asif`).
  - `EngineSupervisor` spec for discovering/launching the bundled `opencode` sidecar ("StackSpirit
    Engine") or an existing user `opencode` / `claude-code` binary, plus VOWP Phase 3 readiness
    probing (`LocalOrchestratorGate::Ready | WaitingForVowpPhase3`).
- Full `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

### Phase D2 — `ss-2-desktop-shell` (Tauri 2 Desktop App Shell, SpiritBridge & Host Manager UI)
- Scaffolds `apps/spirit-desktop` (React + TypeScript + Vite frontend + `src-tauri` Rust backend
  wired to `spirit-core`).
- Implements the `SpiritBridge` TypeScript interface with both `TauriBridge` (desktop IPC) and
  `MockBridge` (for hermetic UI tests and preview).
- Builds the **One-Click Host Manager**:
  - Add VPS modal (SSH host, user, port, VOWP home path, one-click test & save).
  - Topbar **Connect / Disconnect** toggle with live connection state, latency, supervisor PID, and
    last-tick age.
  - Project switcher (`All Projects`, `vowp`, `northnoir`, `vowp-asif`).

### Phase D3 — `ss-3-fleet-canvas` (Visual Worker Graph, Job Lanes, VPS Storage & Capacity Cockpit)
- Builds the signature visual views in `apps/spirit-desktop`:
  - **Visual Worker & Fleet Canvas:** VPS node card at center, live worker slot rings (showing busy
    vs idle slots up to the target, e.g. `3 / 8` active), running worker cards with live harness +
    provider/model badge (e.g., `DeepSeek V4.1 Flash 1017 #max`), and job lanes (`Running`, `Ready`,
    `Reviewing/Fixing`, `Parked/Blocked`, `Done`).
  - **Capacity & Storage Advisor Rail:**
    - Worker Target vs **Recommended Workers** (`advised_workers` from VOWP's capacity advisor) with
      one-click target adjustment (`capacity set`).
    - **VPS Storage & Memory Gauges:** free disk (`disk_free_home_bytes`) vs minimum safe floor
      (`min_disk_bytes`), RAM available vs reserve floor and p90 per-worker estimate, CPU load and
      pressure, and brake status.
  - **Interactive Job & Inbox Actions:** pause, retry, inspect gate failures, and answer inbox items.

### Phase D4 — `ss-4-engine-onboarding` (StackSpirit Engine / `opencode` Sidecar, Free Models, BYOK & VOWP Phase 3 Gate UI)
- Builds the **StackSpirit Engine & Local Orchestrator** view in `apps/spirit-desktop`:
  - Default **Zero-Harness UX**: presents the bundled `opencode` sidecar as **StackSpirit Engine**
    with one-click selection of included/free models or **Bring Your Own Key (BYOK)** stored in the
    local OS keychain (never sent to the VPS).
  - Advanced **External Harness Picker**: optional toggle to use an existing local `opencode`
    desktop/CLI install or `claude-code`.
  - **Cross-Project VOWP Gate Indicator**: checks whether the connected VOWP home supports Phase 3
    local orchestrators and `.asif.zip` session export/import; enables local orchestrator launch
    when VOWP Phase 3 is present, and shows the exact VOWP milestone gate status when connected to
    a pre-Phase-3 home.

### Phase D5 & Beyond (Gated on VOWP Phase 3 & ASIF completion, then Web & CLI)
- **D5 (after VOWP `v4-asif-*` merges):** wire live `.asif.zip` session download & import buttons in
  the Desktop Sessions drawer.
- **D6 (after VOWP Phase 3 `v4-local-3*` merges):** wire live local global/project orchestrator
  sessions running on the user's machine through the bundled StackSpirit Engine (`opencode`).
- **W1 (Web App):** package `spirit-ui` with `HttpSseBridge` for browser access.
- **C1 (CLI):** add `crates/spirit-cli` (`spirit`) wrapping `spirit-core`.
