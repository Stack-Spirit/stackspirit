# StackSpirit ↔ VOWP Contract

Status: frozen read contract for the StackSpirit Desktop MVP (`ss-0-plan`),
2026-11-14.
Audience: `ss-1-core`, `ss-2-desktop-shell`, `ss-3-fleet-canvas`,
`ss-4-engine-onboarding`, and the VOWP owner.

This document specifies:

1. exactly which VOWP commands StackSpirit runs and how;
2. the exact JSON documents it parses, with field-level tables;
3. the mapping from those documents to StackSpirit view models;
4. the **cross-project dependency gates** for ASIF (`v4-asif-*`) and local
   orchestrators (`v4-local-3*`), including how the app probes and displays them
   honestly.

It is a **read contract**. StackSpirit does not open a VOWP database, does not write a
VOWP file, and does not edit `/home/dev/vowp`. All mutations are VOWP CLI commands
described in §9.

Sources pinned for this document:

- `/home/dev/vowp/docs/design/stackspirit.md` (product and desktop-first plan).
- `/home/dev/vowp/docs/design/v0.4/local-orchestrator.md` (local surfaces, offload,
  Phase 0–4, commands, credential model).
- `/home/dev/vowp/docs/design/v0.4/asif-integration.md` (ASIF 0.4 draft, adapter
  `vowp-asif` 0.1.1, pin `652d9e23ddbe5656b9ceea311df7e09e93c1cd2f`, milestone split).
- `/home/dev/vowp/crates/vowp-protocol/src/` (Rust wire types).
- `/home/dev/vowp/docs/design/cli.md` (CLI JSON wrappers and exit codes).

The VOWP repository is the source of truth. If this document and the VOWP source
disagree, the VOWP source wins; that is a contract-test failure to fix, not a reason to
guess at runtime.

### Pinned contract versions

| Item | Value |
|---|---|
| VOWP protocol wire version | `1` (`vowp_protocol::PROTOCOL_VERSION`) |
| Snapshot schema / version | `vowp-snapshot/1` / `1` |
| Agent protocol | `vowp-agent/1` |
| ASIF | `0.4` review draft, pin `652d9e23ddbe5656b9ceea311df7e09e93c1cd2f` |
| ASIF adapter | `vowp-asif` `0.1.1`, tag `v0.1.1`, commit `466ebb81f58646febb80996874b46c11288d5a97` |
| VOWP target | v0.4 local-orchestrator plan and v0.3 daemon API |
| StackSpirit contract | this document, 2026-11-14 |

---

## 1. Transport and invocation contract

### 1.1 Outbound SSH only

StackSpirit connects from the desktop to the VOWP home with the system OpenSSH client.
There is no externally reachable listener on the desktop; the local StackSpirit Engine
(if running) listens on loopback only. The app does not use the VOWP daemon HTTP API in
D1–D4 because it is loopback-only on the home.

```text
ssh
  -o BatchMode=yes
  -o StrictHostKeyChecking=yes
  -o UserKnownHostsFile=<app-data>/known_hosts
  -o ConnectTimeout=8
  -o ServerAliveInterval=15
  -o ServerAliveCountMax=3
  [-i <identity-file> -o IdentitiesOnly=yes]
  -p <port>
  -- <user>@<host>
  <remote-command>
```

- `<remote-command>` is built from a fixed VOWP argv array: every argument is POSIX
  single-quoted (`'` becomes `'\''`) and joined with spaces. SSH necessarily hands the
  command to the remote login shell; StackSpirit never interpolates unquoted user text
  and never uses a pipeline, `;`, `&&`, or redirection.
- `IdentitiesOnly=yes` is set only when `-i` is supplied; otherwise the user's SSH agent
  may offer its normal keys.
- `--` terminates local ssh options and is placed before the destination; it is not part
  of the remote command.
- `--home` is optional and comes from the host profile.
- `--project` is added only for project-scoped commands (ASIF export/import, job
  actions, inbox answers), from the selected job/session/item. Fleet-wide reads
  (`fleet status`, `capacity report`, `fleet queue`, `fleet inbox`) run without it.
- The initial connection probes `vowp --version` (plain text, one line) before it runs a
  JSON command.
- Read timeouts are 20 s; the initial snapshot may take 60 s; writes are 30 s.
- Output caps are 16 MiB per JSON document. A larger document is a typed error, never a
  silent truncation.
- Stderr is captured separately, redacted, bounded to 64 KiB, and shown only in details.

### 1.2 Primary commands

| Purpose | Command | Output |
|---|---|---|
| Snapshot + freshness | `vowp --json fleet status` | `FleetStatus` object (§4) |
| Capacity detail | `vowp --json capacity report` | `CapacityReport` + CLI extras (§5) |
| Queue rows | `vowp --json fleet queue` | array of `FleetQueueRow` (§6) |
| Inbox rows | `vowp --json fleet inbox --all` | array of `InboxItem` (§7) |
| Version | `vowp --version` | plain text |
| Host / VOWP presence | `vowp --version` success | `VowpMissing` if the binary is absent (remote shell exit 127) or not found; other failures are `VowpCommand` |
| Gate probes | see §11 | read-only capability evidence |

`vowp fleet status --json` is the primary poll. It returns the snapshot **and** the
wrapper's `source`, `fetched_at`, and `stale` fields. StackSpirit does not read
`~/.vowp/snapshot.json` directly in normal operation; the file is documented only because
it is the embedded snapshot document. The on-disk `snapshot.json` is exactly the
`snapshot` value in §4.1 and the document in §4.2; it has no wrapper.

`vowp capacity report --json` is the only source for `estimates`, `waiting`,
`provider_caps`, and `reload_pending`. The snapshot's capacity card is the fast path for
the gauges.

`vowp fleet inbox --json` without `--all` returns open items only; StackSpirit uses
`--all` so the inbox drawer can show answered items when requested.

### 1.3 What StackSpirit never does

- It never opens `~/.vowp`, a project database, a session log, a lease, or `snapshot.json`
  directly.
- It never writes `vowp.toml`, a job, a report, a session, an inbox item, a capacity row,
  or a lease.
- It never runs `vowp up`, `vowp down`, a merge, a promotion, a release, or a deploy.
- It never copies a provider key or a home secret to the VPS.
- It never uses `--cached` to hide a connection failure. If VOWP returns a cached
  snapshot, `source: "cache"` and `stale` are displayed.

---

## 2. Serialization conventions

| Convention | Rule |
|---|---|
| Field names | `snake_case` exactly as in the Rust types. |
| Enums | Lowercase `snake_case` strings; for example `running`, `claude-code`, `disk_low`. |
| Nullability | `Option<T>` is `null` when absent. Required fields are always present. |
| Timestamps | `Timestamp` is RFC 3339 UTC with a `Z` suffix and **exactly three** fractional digits: `2026-11-14T12:00:00.000Z`. |
| ULIDs | 26-character Crockford base32, uppercase; `CapsuleId` is `cap_` + ULID. |
| Slugs | `^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$` (1–64 chars, no trailing `-`); project names also accept `_node`. |
| Byte counts | Unsigned integers, bytes (not KiB/MiB). |
| Load / pressure | Floating-point numbers; `load1` is a one-minute load average; PSI `avg10` is a percentage (0–100) when present. |
| Unknown fields | Ignored by StackSpirit's parser. |
| Unknown enum values | Shown as **Unknown**; never coerced to a known state. |
| Schema marker | `snapshot.schema` must be exactly `"vowp-snapshot/1"` and `snapshot.version` exactly `1` for this contract. |
| Errors | `{"error":{"kind":"...","message":"...","hint":"..."}}` on stdout; exit code from the CLI table (§8.2). |

StackSpirit's Rust and TypeScript models keep the wire `snake_case` names at the bridge
boundary. The UI consumes `FleetView`/`CapacityView` from the core, not raw JSON, and does
its own formatting in dedicated helpers.

---

## 3. Common vocabulary

### 3.1 IDs

| Types | Shape | Example |
|---|---|---|
| `SessionId`, `EventId`, `RunId`, `InboxId`, `SwitchId`, `ReleaseId`, `ScheduleId` | 26-char ULID | `01JCF5WQ8ZK3M2V7N4P6R8T0AB` |
| `CapsuleId` | `cap_` + 26-char ULID | `cap_01JCF5WQ8ZK3M2V7N4P6R8T0AB` |
| `JobId`, `NodeName`, `ProjectName` | slug | `ss-3-fleet-canvas`, `vps`, `vowp` |

### 3.2 Enumerations used by the desktop

| Enum | Values |
|---|---|
| `HarnessKind` | `claude-code`, `opencode`, `cli`, `acp`, `vowp` |
| `Role` (executor role) | `planner`, `keeper`, `worker`, `verifier` |
| `RunRole` | `planner`, `keeper`, `worker`, `verifier`, `capsule`, `probe`, `merge`, `gate` |
| `JobKind` | `plan`, `spec`, `build`, `test`, `verify`, `fix`, `merge`, `docs`, `research`, `promote`, `release`, `decision` |
| `JobState` | `pending`, `ready`, `running`, `stalled`, `gating`, `reviewing`, `fixing`, `merging`, `done`, `failed`, `parked`, `replanned`, `cancelled` |
| `ProjectState` | `active`, `paused`, `archived` |
| `SessionState` | `idle`, `running`, `blocked`, `switching`, `closed` |
| `SessionRole` | `planner`, `keeper`, `worker`, `verifier`, `user`, `orchestrator` |
| `SurfaceKind` | `chat`, `opencode`, `claude_code`, `mcp`, `mirror`, `api`, `agent` |
| `OrchestratorApp` | `opencode`, `claude_code`, `chat` |
| `InboxKind` | `question`, `approval`, `decision`, `notice` |
| `InboxState` | `open`, `answered`, `dismissed` |
| `LimitRowKind` | `quota`, `credits`, `rate`, `context`, `auth`, `unavailable` |
| `TargetMode` | `auto`, `fixed` |
| `WaitCode` | `ram`, `cpu`, `disk`, `provider`, `brake`, `target` |
| `BrakeCode` | `memory_pressure`, `swap_growth`, `disk_low`, `provider` |
| `SampleSource` | `proc`, `fallback` |
| `BuildMode` | `cold`, `warm`, `incremental`, `unknown` |
| `EstimateSource` | `measured`, `pooled`, `default` |

`JobState::as_str()` defines the canonical state strings; `jobs_by_state` uses exactly
these keys. A missing key means zero.

### 3.3 Executor string

`harness:model[@effort][#variant]`

- `@effort` is valid only for `claude-code`.
- `#variant` is valid only for `opencode`.
- Provider mapping (matches VOWP):
  - `claude-code` → `claude`;
  - `opencode` → the segment before `/` in `model`;
  - `cli`, `acp`, `vowp` → the whole model string.

Examples:

| Executor string | Harness | Provider | Model | Effort | Variant |
|---|---|---|---|---|---|
| `opencode:deepseek/deepseek-flash#max` | `opencode` | `deepseek` | `deepseek/deepseek-flash` | null | `max` |
| `claude-code:model-a@medium` | `claude-code` | `claude` | `model-a` | `medium` | null |
| `cli:codex` | `cli` | `codex` | `codex` | null | null |
| `acp:claude-code-acp` | `acp` | `claude-code-acp` | `claude-code-acp` | null | null |

A malformed string is preserved raw and shown as **Unknown harness**; it is never
normalized into a different executor.

---

## 4. `vowp --json fleet status` and `snapshot.json`

### 4.1 Wrapper document (`FleetStatus`)

```json
{
  "snapshot": { "...": "the Snapshot object in §4.2" },
  "source": "local",
  "fetched_at": "2026-11-14T12:00:04.812Z",
  "stale": false
}
```

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `snapshot` | `Snapshot` | no | The full snapshot, exactly as written to `snapshot.json`. |
| `source` | string | no | `local`, `remote`, or `cache`. VOWP's perspective. |
| `fetched_at` | `Timestamp` | no | When VOWP produced or fetched this answer. |
| `stale` | bool | no | True when the answer is older than three ticks. |

StackSpirit treats `source: "cache"` or `stale: true` as stale even if the snapshot
`generated_at` is recent. A stale view is read-only until a refresh succeeds.

### 4.2 `Snapshot` document

```json
{
  "schema": "vowp-snapshot/1",
  "version": 1,
  "sequence": 4412,
  "generated_at": "2026-11-14T12:00:00.000Z",
  "home": "vps",
  "supervisor": {
    "running": true,
    "pid": 4812,
    "started_at": "2026-11-13T21:04:10.000Z",
    "last_tick": "2026-11-14T11:59:58.000Z",
    "tick_secs": 60,
    "paused": false,
    "fail_streak": 0,
    "version": "0.4.0"
  },
  "projects": [
    {
      "name": "vowp",
      "path": "/srv/vowp",
      "state": "active",
      "weight": 50,
      "paused_reason": "",
      "jobs_by_state": {
        "ready": 4,
        "running": 1,
        "reviewing": 1,
        "done": 37
      },
      "running": [
        {
          "job": "ss-2-desktop-shell",
          "kind": "build",
          "role": "worker",
          "executor": "opencode:deepseek/deepseek-flash#max",
          "node": "vps",
          "age_secs": 134,
          "last_event_age_secs": 3,
          "session_id": "01JCF5WQ8ZK3M2V7N4P6R8T0AB"
        }
      ],
      "blocked": [
        {
          "job": "ss-4-engine-onboarding",
          "state": "parked",
          "reason": "question",
          "detail": "Waiting for the owner to choose the included model set.",
          "since": "2026-11-14T11:42:00.000Z"
        }
      ],
      "queue": [
        {
          "job": "ss-3-fleet-canvas",
          "title": "Build the visual fleet canvas",
          "kind": "build",
          "role": "worker",
          "priority": 50,
          "state": "ready",
          "age_secs": 420,
          "waiting_reason": "RAM 1.2 GB short"
        }
      ],
      "last_merge": {
        "job": "ss-1-core",
        "at": "2026-11-14T10:15:00.000Z",
        "commit": "4f2c1a9b8e7d6c5b4a39281706f5e4d3c2b1a090",
        "branch": "job/ss-1-core"
      },
      "gate_failures": [
        {
          "job": "ss-3-fleet-canvas",
          "gate": "secret_scan",
          "at": "2026-11-14T11:20:00.000Z",
          "evidence": "example evidence line, redacted"
        }
      ],
      "cost_today_usd": 1.37
    }
  ],
  "capacity": {
    "measured": {
      "at": "2026-11-14T11:59:45.000Z",
      "interval_secs": 15,
      "mem_total_bytes": 16777216000,
      "mem_available_bytes": 13314392064,
      "swap_used_bytes": 0,
      "load1": 2.1,
      "load5": 1.8,
      "load15": 1.6,
      "cpu_count": 8,
      "cpu_quota_cores": null,
      "disk_free_home_bytes": 45206814720,
      "disk_free_worktrees_bytes": 41567838208,
      "pressure_cpu_avg10": 4.2,
      "pressure_mem_avg10": 1.1,
      "pressure_io_avg10": 0.8,
      "source": "proc"
    },
    "running": {
      "runs": 1,
      "ram_p90_bytes": 1181116006,
      "cpu_p90_cores": 2.4
    },
    "target": {
      "mode": "fixed",
      "target_workers": 8,
      "advised_workers": 8,
      "is_override": false,
      "set_by": "owner",
      "set_at": "2026-11-14T09:30:00.000Z",
      "reason": "steady state"
    },
    "advised_target": 8,
    "brake": {
      "active": false,
      "reasons": [],
      "since": null
    },
    "reserve_ram_bytes": 2147483648,
    "min_disk_bytes": 5368709120
  },
  "limits": [
    {
      "provider": "anthropic",
      "harness": "claude-code",
      "kind": "quota",
      "resets_at": "2026-11-14T15:00:00.000Z",
      "detail": "quota resets at 15:00",
      "since": "2026-11-14T11:10:00.000Z",
      "last_probe_at": null,
      "probe_fail_streak": 0,
      "concurrency_cap": null,
      "cap_until": null
    }
  ],
  "inbox": [
    {
      "id": "01JCF5WQ8ZK3M2V7N4P6R8T0AC",
      "project": "vowp",
      "job_id": "ss-4-engine-onboarding",
      "kind": "question",
      "title": "Choose the included model set",
      "created_at": "2026-11-14T11:42:00.000Z",
      "age_secs": 1080
    }
  ],
  "sessions": [
    {
      "id": "01JCF5WQ8ZK3M2V7N4P6R8T0AB",
      "project": "vowp",
      "job_id": "ss-2-desktop-shell",
      "role": "worker",
      "title": "Desktop shell and SpiritBridge",
      "state": "running",
      "executor": "opencode:deepseek/deepseek-flash#max",
      "node": "vps",
      "active_surface": {
        "surface_id": "01JCF5WQ8ZK3M2V7N4P6R8T0AD",
        "kind": "opencode",
        "holder": "vps opencode",
        "node": "vps",
        "acquired_at": "2026-11-14T11:30:00.000Z",
        "heartbeat_at": "2026-11-14T11:59:55.000Z"
      },
      "last_event_at": "2026-11-14T11:59:57.000Z"
    }
  ],
  "orchestrators": [
    {
      "name": "vowp-main",
      "session_id": "01JCF5WQ8ZK3M2V7N4P6R8T0AE",
      "project": "vowp",
      "app": "opencode",
      "state": "running",
      "join": {
        "desktop": "vowp orchestrator join vowp-main --surface desktop",
        "phone": "Open the VOWP board and join vowp-main",
        "chat": "vowp chat --session 01JCF5WQ8ZK3M2V7N4P6R8T0AE"
      }
    }
  ]
}
```

### 4.3 `Snapshot` fields

| Field | Type | Null? | StackSpirit use |
|---|---|---|---|
| `schema` | string | no | Must be `vowp-snapshot/1`; otherwise `vowp_incompatible`. |
| `version` | u32 | no | Must be `1` for this contract. |
| `sequence` | u64 | no | Changes every write; useful for diagnostics. |
| `generated_at` | `Timestamp` | no | Freshness and status strip. |
| `home` | `NodeName` | no | VPS root card. |
| `supervisor` | `SupervisorSnapshot` | no | Top bar and root card. |
| `projects` | array | no (may be empty) | Project switcher, lanes, running cards. |
| `capacity` | `CapacitySnapshot` | no | Capacity rail fast path. |
| `limits` | `Limit[]` | no (may be empty) | Limits panel and top-bar badge. |
| `inbox` | `InboxSummary[]` | no (may be empty) | Inbox badge; bodies come from §7. |
| `sessions` | `SessionSurfaceCard[]` | no (may be empty) | Sessions drawer. |
| `orchestrators` | `OrchestratorCard[]` | no (may be empty) | Engine / D6 view; no `node` field today (§11.3.3). |

### 4.4 `SupervisorSnapshot`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `running` | bool | no | A supervisor process is running. |
| `pid` | u32 | yes | Process id. |
| `started_at` | `Timestamp` | yes | Supervisor start. |
| `last_tick` | `Timestamp` | yes | Last completed tick. |
| `tick_secs` | u64 | no | Tick interval. |
| `paused` | bool | no | Node paused. |
| `fail_streak` | u32 | no | Consecutive failed L1 ticks. |
| `version` | string | no | VOWP binary version. |

UI rule: a running supervisor with `last_tick` older than `max(3 * tick_secs, 180s)` is
shown as amber **tick overdue**, even if `running` is true.

### 4.5 `ProjectSnapshot`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `name` | `ProjectName` | no | Project switcher and chips. |
| `path` | string | no | VOWP's checkout path; displayed only in details, never as a secret. |
| `state` | `ProjectState` | no | Active / paused / archived. |
| `weight` | u8 (1–100) | no | Fairness weight. |
| `paused_reason` | string | no | `owner` or `broken: <error>`; empty when active. |
| `jobs_by_state` | map<string,u32> | no | Authority for lane counts. |
| `running` | `RunningJobCard[]` | no | Running worker cards. |
| `blocked` | `BlockedJobCard[]` | no | Parked/blocked lane. |
| `queue` | `QueueJobCard[]` | no | Ready/review lanes; capped at 200. |
| `last_merge` | `MergeCard` | yes | Done lane / project summary. |
| `gate_failures` | `GateFailureCard[]` | no | Review & Fix lane details. |
| `cost_today_usd` | f64 | yes | Project cost today; null means unknown, never zero. |

#### `RunningJobCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `job` | `JobId` | no | Job id. |
| `kind` | `JobKind` | no | Kind badge. |
| `role` | `Role` | no | Worker/verifier/planner/keeper. |
| `executor` | string | yes | Canonical executor string; null for shell/gate/merge. |
| `node` | `NodeName` | no | Executor node. |
| `age_secs` | u64 | no | Run age. |
| `last_event_age_secs` | u64 | yes | Liveness. |
| `session_id` | `SessionId` | yes | Session drawer link. |

#### `BlockedJobCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `job` | `JobId` | no | Job id. |
| `state` | `JobState` | no | Parked/failed/etc. |
| `reason` | string | no | Parked or failure reason. |
| `detail` | string | no | Redacted, at most 200 characters. |
| `since` | `Timestamp` | no | Entered this state. |

There is **no title** in this card. StackSpirit shows the job id and reason; it never
invents a title.

#### `QueueJobCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `job` | `JobId` | no | Job id. |
| `title` | string | no | Human title. |
| `kind` | `JobKind` | no | Kind badge. |
| `role` | `Role` | no | Executor role. |
| `priority` | i32 | no | Dispatch priority; higher runs first. |
| `state` | `JobState` | no | Queue state. |
| `age_secs` | u64 | no | Age in this state. |
| `waiting_reason` | string | yes | Last admission/brake reason. |

VOWP caps `queue` at 200 entries. At 200, StackSpirit says "showing the first 200",
never "200 total". `jobs_by_state` remains the count authority.

#### `MergeCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `job` | `JobId` | no | Merged job. |
| `at` | `Timestamp` | no | Merge time. |
| `commit` | string | no | Commit sha. |
| `branch` | string | no | Merged branch. |

#### `GateFailureCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `job` | `JobId` | no | Job the gate ran for. |
| `gate` | string | no | Gate name. |
| `at` | `Timestamp` | no | Failure time. |
| `evidence` | string | no | Redacted, at most 200 characters. |

### 4.6 `CapacitySnapshot`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `measured` | `HostSample` | yes | Latest host sample. |
| `running` | `RunningCapacity` | no | `runs`, `ram_p90_bytes`, `cpu_p90_cores`. |
| `target` | `CapacityTarget` | no | Current target. |
| `advised_target` | u32 | yes | Supervisor advice. |
| `brake` | `BrakeState` | no | Active brake and reasons. |
| `reserve_ram_bytes` | u64 | no | RAM reserve. |
| `min_disk_bytes` | u64 | no | Disk floor. |

#### `HostSample`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `at` | `Timestamp` | no | Sample time. |
| `interval_secs` | u64 | no | Sampling interval. |
| `mem_total_bytes` | u64 | no | Total physical RAM. |
| `mem_available_bytes` | u64 | no | Available to new work. |
| `swap_used_bytes` | u64 | no | Swap in use. |
| `load1`, `load5`, `load15` | f64 | no | Load averages. |
| `cpu_count` | u32 | no | Logical CPUs. |
| `cpu_quota_cores` | f64 | yes | Cgroup v2 CPU quota in cores. |
| `disk_free_home_bytes` | u64 | no | Free bytes on the home filesystem. |
| `disk_free_worktrees_bytes` | u64 | yes | Free bytes for worktrees, when different. |
| `pressure_cpu_avg10` | f64 | yes | `/proc/pressure/cpu` `avg10`. |
| `pressure_mem_avg10` | f64 | yes | `/proc/pressure/memory` `avg10`. |
| `pressure_io_avg10` | f64 | yes | `/proc/pressure/io` `avg10`. |
| `source` | `SampleSource` | no | `proc` or `fallback`. |

#### `CapacityTarget`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `mode` | `TargetMode` | no | `auto` or `fixed`. |
| `target_workers` | u32 | no | Current allowed workers. |
| `advised_workers` | u32 | yes | The supervisor's advised number, when known. |
| `is_override` | bool | no | Owner raised the target above advice. |
| `set_by` | string | no | `owner`, `supervisor`, or `setup`. |
| `set_at` | `Timestamp` | no | When set. |
| `reason` | string | no | Free-form reason. |

Advice precedence in the UI: `target.advised_workers` if non-null, else
`capacity.advised_target`, else **no advice**. `is_override` is always displayed when
true.

#### `BrakeState` and `BrakeReason`

`brake.active` is a bool; `brake.reasons` is an array of
`{"code": BrakeCode, "detail": string}`; `brake.since` is a `Timestamp` or null.

### 4.7 `Limit`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `provider` | string | no | Limits key: provider or CLI harness entry. |
| `harness` | `HarnessKind` | yes | Harness that reported the limit. |
| `kind` | `LimitRowKind` | no | `quota`, `credits`, `rate`, `context`, `auth`, `unavailable`. |
| `resets_at` | `Timestamp` | yes | When it lifts; null means a human must resolve it. |
| `detail` | string | no | Redacted human detail. |
| `since` | `Timestamp` | no | First observed. |
| `last_probe_at` | `Timestamp` | yes | Last recovery probe. |
| `probe_fail_streak` | u32 | no | Consecutive failed probes. |
| `concurrency_cap` | u32 | yes | Temporary cap, when reported. |
| `cap_until` | `Timestamp` | yes | Cap expiry. |

### 4.8 `InboxSummary`, `SessionSurfaceCard`, `SurfaceRef`, `OrchestratorCard`

#### `InboxSummary`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `id` | `InboxId` | no | Item id. |
| `project` | `ProjectName` | yes | Bound project. |
| `job_id` | `JobId` | yes | Bound job. |
| `kind` | `InboxKind` | no | Question/approval/decision/notice. |
| `title` | string | no | Short title. |
| `created_at` | `Timestamp` | no | Creation time. |
| `age_secs` | u64 | no | Age. |

The summary **omits `body`**; the inbox drawer uses §7 for the full item.

#### `SessionSurfaceCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `id` | `SessionId` | no | Session id. |
| `project` | `ProjectName` | yes | Bound project. |
| `job_id` | `JobId` | yes | Bound job. |
| `role` | `SessionRole` | yes | Session role. |
| `title` | string | no | Title. |
| `state` | `SessionState` | no | Session state. |
| `executor` | string | yes | Canonical executor string. |
| `node` | `NodeName` | no | Node owning the log. |
| `active_surface` | `SurfaceRef` | yes | Live lease holder. |
| `last_event_at` | `Timestamp` | no | Newest event. |

#### `SurfaceRef`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `surface_id` | `SurfaceId` (ULID) | no | Surface instance. |
| `kind` | `SurfaceKind` | no | Client kind. |
| `holder` | string | no | Human holder name. |
| `node` | `NodeName` | no | Holder node. |
| `acquired_at` | `Timestamp` | no | Lease acquisition. |
| `heartbeat_at` | `Timestamp` | no | Last heartbeat. |

#### `OrchestratorCard`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `name` | string | no | Orchestrator name. |
| `session_id` | `SessionId` | no | Session. |
| `project` | `ProjectName` | yes | Bound project. |
| `app` | `OrchestratorApp` | no | `opencode`, `claude_code`, `chat`. |
| `state` | string | no | `running`, `stopped`, or `degraded`. |
| `node` | `NodeName` | yes | Node that runs the app; absent/`null` means a v0.2 home-hosted orchestrator. |
| `join` | object | no | `desktop`, `phone`, `chat` strings. |

The merged `vowp-protocol::surface::OrchestratorCard` has an optional `node`. An installed
build may still omit it, so StackSpirit must not infer a local node from an absent field.
The D6 gate's node comes from `vowp node show`, not from `vowp orchestrator ls`; see
§11.3.3.

---

## 5. `vowp --json capacity report`

### 5.1 Invocation and CLI extras

```sh
vowp --json capacity report
```

`vowp capacity show --json` is the same document. The CLI adds two fields that are
**not** part of `vowp_protocol::capacity::CapacityReport`:

| Field | Type | Meaning |
|---|---|---|
| `provider_caps` | object `provider → u32` | Effective provider concurrency caps from `vowp.toml` / node defaults. |
| `reload_pending` | bool | A config reload is pending. |

StackSpirit parses both. It displays `provider_caps` in the limits panel and
`reload_pending` as a small **config reload pending** note. It never derives provider
caps from the snapshot alone.

### 5.2 Document

```json
{
  "generated_at": "2026-11-14T12:00:03.000Z",
  "host": {
    "at": "2026-11-14T11:59:45.000Z",
    "interval_secs": 15,
    "mem_total_bytes": 16777216000,
    "mem_available_bytes": 13314392064,
    "swap_used_bytes": 0,
    "load1": 2.1,
    "load5": 1.8,
    "load15": 1.6,
    "cpu_count": 8,
    "cpu_quota_cores": null,
    "disk_free_home_bytes": 45206814720,
    "disk_free_worktrees_bytes": 41567838208,
    "pressure_cpu_avg10": 4.2,
    "pressure_mem_avg10": 1.1,
    "pressure_io_avg10": 0.8,
    "source": "proc"
  },
  "target": {
    "mode": "fixed",
    "target_workers": 8,
    "advised_workers": 8,
    "is_override": false,
    "set_by": "owner",
    "set_at": "2026-11-14T09:30:00.000Z",
    "reason": "steady state"
  },
  "running": {
    "runs": 1,
    "ram_p90_bytes": 1181116006,
    "cpu_p90_cores": 2.4
  },
  "brakes": {
    "active": false,
    "reasons": [],
    "since": null
  },
  "reserve_ram_bytes": 2147483648,
  "min_disk_bytes": 5368709120,
  "estimates": [
    {
      "project": "vowp",
      "job_kind": "build",
      "harness": "opencode",
      "model": "deepseek/deepseek-flash",
      "build_mode": "warm",
      "samples": 12,
      "ram_p50_bytes": 734003200,
      "ram_p90_bytes": 1288490188,
      "cpu_p50_cores": 1.1,
      "cpu_p90_cores": 2.4,
      "duration_p50_ms": 420000,
      "duration_p90_ms": 910000,
      "source": "measured"
    },
    {
      "project": null,
      "job_kind": "build",
      "harness": null,
      "model": null,
      "build_mode": null,
      "samples": 40,
      "ram_p50_bytes": 838860800,
      "ram_p90_bytes": 1610612736,
      "cpu_p50_cores": 1.2,
      "cpu_p90_cores": 2.8,
      "duration_p50_ms": 510000,
      "duration_p90_ms": 1100000,
      "source": "pooled"
    }
  ],
  "waiting": [
    {
      "project": "vowp",
      "job": "ss-3-fleet-canvas",
      "reason": {
        "code": "ram",
        "detail": "RAM 1.2 GB short"
      },
      "since": "2026-11-14T11:57:00.000Z"
    }
  ],
  "limits": [],
  "provider_caps": {
    "deepseek": 8
  },
  "reload_pending": false
}
```

### 5.3 `CapacityReport` fields

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `generated_at` | `Timestamp` | no | Report generation. |
| `host` | `HostSample` | yes | Latest sample; absent before the sampler runs. |
| `target` | `CapacityTarget` | no | Current target and advice. |
| `running` | `RunningCapacity` | no | Open runs and p90. |
| `brakes` | `BrakeState` | no | **Plural** field name. |
| `reserve_ram_bytes` | u64 | no | RAM reserve. |
| `min_disk_bytes` | u64 | no | Disk floor. |
| `estimates` | `UsageEstimate[]` | no | Pooled estimates. |
| `waiting` | `WaitingJob[]` | no | Jobs waiting for capacity. |
| `limits` | `Limit[]` | no | Provider limits. |
| `provider_caps` | map<string,u32> | no (CLI extra) | Effective caps. |
| `reload_pending` | bool | no (CLI extra) | Config reload pending. |

`CapacitySnapshot.brake` is singular and `CapacityReport.brakes` is plural. This is a
real difference in the protocol types; the parser must not normalize one into the other
silently.

### 5.4 `UsageEstimate`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `project` | `ProjectName` | yes | Pooled key; null is the empty key. |
| `job_kind` | `JobKind` | yes | Pooled key. |
| `harness` | `HarnessKind` | yes | Pooled key. |
| `model` | string | yes | Pooled key. |
| `build_mode` | `BuildMode` | yes | Pooled key; null is `unknown`. |
| `samples` | u32 | no | Number of samples. |
| `ram_p50_bytes` | u64 | no | Median peak RSS. |
| `ram_p90_bytes` | u64 | no | p90 peak RSS. |
| `cpu_p50_cores` | f64 | no | Median CPU cores. |
| `cpu_p90_cores` | f64 | no | p90 CPU cores. |
| `duration_p50_ms` | u64 | no | Median duration. |
| `duration_p90_ms` | u64 | no | p90 duration. |
| `source` | `EstimateSource` | no | `measured`, `pooled`, `default`. |

The capacity rail's per-worker RAM estimate uses the selection order in
`PLAN.md` §5.4. It always shows the source and sample count; a `default` estimate is
labeled **default estimate**, never measured.

### 5.5 `WaitingJob` and `WaitReason`

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `project` | `ProjectName` | no | Project. |
| `job` | `JobId` | no | Waiting job. |
| `reason.code` | `WaitCode` | no | `ram`, `cpu`, `disk`, `provider`, `brake`, `target`. |
| `reason.detail` | string | no | One human sentence, e.g. "RAM 1.2 GB short". |
| `since` | `Timestamp` | no | Waiting since. |

The UI groups by `reason.code` and shows the detail verbatim.

---

## 6. `vowp --json fleet queue`

The output is a JSON **array** of `FleetQueueRow`:

```json
[
  {
    "project": "vowp",
    "job": "ss-3-fleet-canvas",
    "title": "Build the visual fleet canvas",
    "kind": "build",
    "role": "worker",
    "priority": 50,
    "state": "ready",
    "age_secs": 420,
    "waiting_reason": "RAM 1.2 GB short"
  },
  {
    "project": "vowp",
    "job": "ss-4-engine-onboarding",
    "title": "StackSpirit Engine onboarding",
    "kind": "build",
    "role": "worker",
    "priority": 50,
    "state": "pending",
    "age_secs": 60,
    "waiting_reason": null
  }
]
```

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `project` | `ProjectName` | no | Owning project. |
| `job` | `JobId` | no | Job. |
| `title` | string | no | Title; unlike `RunningJobCard`/`BlockedJobCard`, the queue has one. |
| `kind` | `JobKind` | no | Kind. |
| `role` | `Role` | no | Executor role. |
| `priority` | i32 | no | Dispatch priority. |
| `state` | `JobState` | no | Current state. |
| `age_secs` | u64 | no | Age in this state. |
| `waiting_reason` | string | yes | Last admission/brake reason. |

The command supports `--project <name>` and `--state <state>`; StackSpirit uses the
unfiltered form so the UI can filter locally. The array is not paginated in the current
contract. If VOWP later caps it, the response must say so; StackSpirit will label the
truncation rather than assume completeness.

---

## 7. `vowp --json fleet inbox --all`

The output is a JSON **array** of `InboxItem`:

```json
[
  {
    "id": "01JCF5WQ8ZK3M2V7N4P6R8T0AC",
    "project": "vowp",
    "job_id": "ss-4-engine-onboarding",
    "kind": "question",
    "title": "Choose the included model set",
    "body": "Which included models should the first release guarantee?",
    "options": ["Keep the generated catalog", "Pin a minimum set"],
    "state": "open",
    "answer": null,
    "answered_by": null,
    "created_at": "2026-11-14T11:42:00.000Z",
    "answered_at": null
  }
]
```

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `id` | `InboxId` | no | Item id. |
| `project` | `ProjectName` | yes | Bound project. |
| `job_id` | `JobId` | yes | Bound job. |
| `kind` | `InboxKind` | no | Question, approval, decision, notice. |
| `title` | string | no | Short title. |
| `body` | string | no | Full text; omitted from the snapshot summary. |
| `options` | string[] | no (may be empty) | Suggested answers. |
| `state` | `InboxState` | no | Open, answered, dismissed. |
| `answer` | string | yes | Answer text. |
| `answered_by` | string | yes | Answerer. |
| `created_at` | `Timestamp` | no | Created. |
| `answered_at` | `Timestamp` | yes | Answered/dismissed. |

`vowp fleet inbox` without `--all` returns open items only. StackSpirit requests `--all`
so the **All** tab is honest; the top-bar badge counts `state == "open"`.

Inbox items are per project on the home, but `vowp fleet inbox` reads every project plus
the node inbox. StackSpirit does not filter by project unless the user selects one.

---

## 8. `--json` errors and exit codes

### 8.1 Error document

```json
{
  "error": {
    "kind": "not_found",
    "message": "session 01JCF5WQ8ZK3M2V7N4P6R8T0AB not found",
    "hint": "vowp session ls"
  }
}
```

| Field | Type | Null? | Meaning |
|---|---|---|---|
| `error.kind` | `ErrorKind` | no | Stable machine class (below). |
| `error.message` | string | no | Human message; no secret. |
| `error.hint` | string | yes | Optional next step. |

`ErrorKind` values: `invalid_input`, `not_found`, `conflict`, `io`, `db`, `parse`,
`config`, `adapter_unavailable`, `adapter_failed`, `limit`, `auth`, `timeout`, `guard`,
`refused`, `remote`, `internal`.

StackSpirit maps the VOWP kind to its own `SpiritErrorCode` and shows the message, not a
generic "command failed". It never parses a human message to make a control-flow
decision.

### 8.2 Exit codes

| Exit | Meaning | StackSpirit mapping |
|---|---|---|
| `0` | Success. | Apply result. |
| `1` | Internal/unexpected, I/O, DB, parse, config, adapter failure, timeout. | `vowp_command` / `internal`; retry only if the command is idempotent. |
| `2` | Invalid input/usage (bad id, bad flag, missing project selection). | `invalid_input`; show VOWP message. |
| `3` | Not found (job, session, node, project, secret, tunnel). | `not_found`; refresh. |
| `4` | Conflict (state does not allow, lease held, already answered, migration). | `conflict`; refresh; do not auto-retry. |
| `5` | Guard/refused (cwd outside allow-list, target above advice without `--override`, missing `--yes`). | `refused`; show the guard. |
| `6` | Provider limited / auth needed. | `limit` / `auth`; show the provider. |
| `7` | Remote failure (SSH, remote VOWP missing). | `remote`; connection banner. |

The CLI may return `75` for a lease/takeover stop in the `__exec` convention; StackSpirit
does not run `__exec` in D1–D4, so it treats `75` as a generic refused/remote failure if
it ever appears.

---

## 9. Mutations (commands, not read documents)

StackSpirit writes only through these VOWP commands. Their exact output is the affected
protocol object or the error document above; the app refreshes `fleet status` after a
success instead of trusting an optimistic local state.

### 9.1 Capacity target

```sh
vowp --json capacity set \
  --target <n> \
  --mode <auto|fixed> \
  [--override] \
  --reason "<text>" \
  --by stackspirit
```

- A target above the advised value requires `--override` (VOWP exits 5 otherwise).
- StackSpirit asks for explicit confirmation before adding `--override`.
- The input is validated as a non-negative integer; VOWP remains authoritative on the
  effective maximum. The `reason` is user text, trimmed, at most 200 characters, and
  never contains a secret.

### 9.2 Job actions

```sh
vowp --project <job-project> --json job retry <job-id>
vowp --project <job-project> --json job park <job-id> --reason "<text>"
vowp --project <job-project> --json job cancel <job-id>
```

- The UI enables an action only for states VOWP documents as legal.
- A conflict (exit 4) is displayed as "VOWP changed this job; refreshing" and triggers a
  snapshot refresh.
- `cancel` is danger-styled and confirmed.

### 9.3 Inbox answer

```sh
vowp --project <item-project> --json inbox answer <inbox-id> "<text>" --by stackspirit
```

Node-inbox items have no project; omit `--project` for those. `--approve`/`--reject` are
valid only for approval items; StackSpirit can use the literal answer or the shorthand,
but never both.

- Empty answers are rejected locally. StackSpirit bounds text answers to 10,000 bytes
  (matching the daemon's web limit) as a UI guard; VOWP remains authoritative.
- Answering a job-bound item can move a parked job to ready on the next tick; the app
  refreshes and does not claim the job moved immediately.

### 9.4 What is not a mutation in D1–D4

No `vowp up/down`, no supervisor start/stop, no session turn, no executor switch, no
merge, no promotion, no release, no deployment, no database migration, no `vowp.toml`
write, no provider key transfer.

---

## 10. Mapping VOWP → StackSpirit

| VOWP field | StackSpirit view | Notes |
|---|---|---|
| `FleetStatus.source` | freshness footer / status strip | `local`, `remote`, or `cache`; cache is always stale. |
| `FleetStatus.fetched_at` | freshness footer | Wrapper fetch time, not the snapshot's `generated_at`. |
| `FleetStatus.stale` | stale banner | Three-tick rule plus the max(3*tick, 180s) guard. |
| `snapshot.schema`, `snapshot.version` | compatibility gate | Must be `vowp-snapshot/1` / `1`; otherwise `vowp_incompatible`. |
| `snapshot.sequence` | diagnostics | Not shown in the cockpit; available in the raw-document drawer. |
| `snapshot.generated_at` | freshness footer | Age and "as of" label. |
| `snapshot.home` | VPS root card title | Node slug. |
| `snapshot.supervisor` | top bar + root card | Running, PID, tick age, pause, fail streak, version. |
| `snapshot.projects[].name` | project switcher | `All projects` is synthetic. |
| `snapshot.projects[].state` | project chip | `active`, `paused`, `archived`, or unknown. |
| `snapshot.projects[].paused_reason` | project detail tooltip | Empty when active. |
| `snapshot.projects[].path` | project details only | Never shown in the main canvas; not a secret. |
| `snapshot.projects[].jobs_by_state` | lane counts | Missing key = 0; canonical state strings. |
| `snapshot.projects[].running[]` | active worker cards | `executor` may be null. The card has no state and covers all lock-holding jobs; mark **Stalled** only from a matching queue row. |
| `snapshot.projects[].blocked[]` | parked/blocked lane | No title; show job id + reason. |
| `snapshot.projects[].queue[]` | ready/review lanes | Cap 200; label truncation. |
| `snapshot.projects[].last_merge` | done lane summary | Optional. |
| `snapshot.projects[].gate_failures[]` | review & fix lane | Evidence is redacted/truncated. |
| `snapshot.projects[].cost_today_usd` | project summary | Null = unknown, not zero. |
| `snapshot.capacity.measured` | disk/RAM/CPU gauges | Fast path. |
| `snapshot.capacity.running` | open-run total, running p90, other-runs chip | `runs` counts all open runs; the ring fills worker slots from running cards by role, not from `runs`. |
| `snapshot.capacity.target` | target advisor | Current + mode + override. |
| `snapshot.capacity.advised_target` | target advisor fallback | Used only if `target.advised_workers` is null. |
| `snapshot.capacity.brake` | brake banner | Reasons + since. |
| `snapshot.capacity.reserve_ram_bytes` | RAM reserve | Exact bytes. |
| `snapshot.capacity.min_disk_bytes` | disk floor | Exact bytes. |
| `snapshot.limits[]` | limits panel + top-bar count | Limit rows. |
| `snapshot.inbox[]` | inbox badge | Body is absent; fetch §7. |
| `snapshot.sessions[]` | sessions drawer | Read-only in D1–D4. |
| `snapshot.orchestrators[]` | engine/D6 view | No `node` field today. |
| `capacity report.estimates[]` | per-worker p90 | Show source/samples. |
| `capacity report.waiting[]` | waiting panel | Group by code. |
| `capacity report.provider_caps` | limits panel | CLI-only extra. |
| `capacity report.reload_pending` | config note | CLI-only extra. |
| `fleet queue[]` | optional lane detail | Titles available; snapshot is default. |
| `fleet inbox[]` | inbox drawer | Full body/options. |

The view layer must never read a field not listed here without updating this table and
the contract tests.

---

## 11. Cross-project dependency gates

This is the contract that keeps StackSpirit honest across two repositories. A feature is
either (a) available against the current VOWP JSON, (b) visibly gated on a named VOWP
milestone, or (c) not implemented. There is no fourth state where the UI pretends a
feature works.

### 11.1 Status matrix

| StackSpirit phase / feature | VOWP prerequisite | Status for the Desktop MVP | StackSpirit behavior |
|---|---|---|---|
| Host connect, snapshot, fleet canvas, capacity rail, inbox, job actions | Existing VOWP CLI JSON: `fleet status`, `capacity report`, `fleet queue`, `fleet inbox` | **Available now (D0–D4)** | Build and test against fixtures; live against a real home. |
| StackSpirit Engine, included models, BYOK, external harness | None from VOWP; local sidecar + OS keychain | **Available now (D4)** | Local only; provider/model names stay local until D6. |
| `.asif.zip` export/import (D5) | `v4-asif-core`, `v4-asif-cli`; `v4-asif-web` for the daemon route | **Gated on `v4-asif-*`** | Build the sessions drawer and gate card; actions disabled until the probe says `Ready`. |
| Local orchestrator execution (D6) | Phase 2 (`v4-local-2e/2f/2g`), Phase 3 (`v4-local-3*`: local app host, bridge, MCP proxy, orchestrator materialize, backfill, local cache), and Phase 4 `v4-local-4f-cli-surface` | **Gated on an installed Phase 2/3/4 build and an enrolled `connected` local agent** | Build the gate card and evidence UI; never start a local orchestrator or claim a local session. |

The merged VOWP source now contains the Phase 2 offload read model and the Phase 3/4
local-orchestrator CLI surface; the installed `vowp 0.3.0` build lags it and the local agent
is not enrolled. This contract does **not** treat source presence as installed capability.
The runtime probe is the authority, and D6 remains gated until the installed build and the
connected local agent satisfy §11.3.

### 11.2 ASIF gate (D5)

#### 11.2.1 Required milestones

The ASIF plan splits the work into five jobs with dependency order
A → B/C → D → E:

| Milestone | Scope | Needed by StackSpirit D5? |
|---|---|---|
| `v4-asif-core` | `vowp-asif` dependency, shared report types, core export/import facade | **Yes** — without it the CLI cannot export/import. |
| `v4-asif-cli` | `vowp session export/import --format asif`, remote paths, exit codes | **Yes** — the desktop uses the CLI over SSH. |
| `v4-asif-web` | `GET /api/v1/sessions/{id}/export/asif`, `POST /api/v1/sessions/import/asif` | Optional for D5; the desktop uses SSH + CLI. A future Web app may need it. |
| `v4-asif-docs` | User and design docs | Not a runtime gate; needed for support. |
| `v4-asif-verify` | Independent verification | Not a runtime gate; needed for release. |

The adapter is pinned as `vowp-asif` `0.1.1`, tag `v0.1.1`, commit
`466ebb81f58646febb80996874b46c11288d5a97`; ASIF `0.4` is a review draft pinned at
`652d9e23ddbe5656b9ceea311df7e09e93c1cd2f`. Nothing in StackSpirit may claim ASIF
conformance, lossless translation, exact request reconstruction, portable continuation,
or native-harness import.

#### 11.2.2 Commands StackSpirit will use

**Export (public CLI path, preferred for the desktop):**

```sh
# 1. Produce a private package on the VPS and a JSON summary.
vowp --project <project> --json session export <session-id> \
  --format asif \
  --out <remote-private-temp>/<session-id>.asif.zip \
  [--strict | --allow-loss] \
  [--report <remote-private-temp>/report.json]

# 2. Copy the package over SSH (sftp, or `ssh ... cat`) and verify its byte count.
# 3. Remove the remote private temp directory.
```

D5 adds a narrow remote-file helper to the SSH runner for step 2: create a private
directory (0700), read the file, and remove it. This helper is not part of the read
contract and is never exposed to the renderer.

Why not `--out -`? The public CLI says `--out -` writes only package bytes to stdout and
conflicts with `--json`; the summary is valuable for the loss display. The desktop
therefore uses the summary + file path, then a separate read. The hidden `--asif-stream`
mode is a CLI-to-CLI internal protocol (documented only for `session pull --full`), not a
stable desktop transport; StackSpirit does not depend on it.

**Import (public CLI path):**

```sh
vowp --project <project> --json session import - \
  --format asif \
  [--branch <branch-id>] \
  [--cwd <dir>] \
  [--strict | --allow-loss]
```

The local package is streamed to stdin. The remote performs validate → stage → rename →
reindex. The response is `AsifImportResponse`.

The 16 MiB JSON output cap in §1.1 applies to JSON documents only. The ASIF package
transfer uses the separate remote-file helper and is bounded by the adapter's 272 MiB
archive cap; the desktop checks the package size before it starts the transfer.

**Export summary shape** (from `asif-integration.md` §3.1):

```json
{
  "format": "asif",
  "path": "/home/user/project/01JCF5WQ8ZK3M2V7N4P6R8T0AB.asif.zip",
  "session": "01JCF5WQ8ZK3M2V7N4P6R8T0AB",
  "bytes": 18342,
  "report_path": null,
  "report": {
    "adapter_version": "0.1.1",
    "asif_version": "0.4",
    "asif_pin": "652d9e23ddbe5656b9ceea311df7e09e93c1cd2f",
    "source_session": "01JCF5WQ8ZK3M2V7N4P6R8T0AB",
    "target_session": null,
    "counts": {
      "events_in": 42,
      "events_out": 42,
      "resources": 3,
      "capsules": 1
    },
    "loss_count": 2,
    "losses": [
      {
        "kind": "cwd-omitted",
        "stage": "translation",
        "scope": "environment",
        "message": "path-free explanation"
      },
      {
        "kind": "contexts-reconstructed",
        "stage": "normalization",
        "scope": "contexts",
        "message": "path-free explanation"
      }
    ],
    "unsupported": ["jobs", "gates", "leases"]
  }
}
```

**Import response shape** (shared `AsifImportResponse`):

```json
{
  "session": {
    "id": "01JCF5WQ8ZK3M2V7N4P6R8T0AF",
    "project": "vowp",
    "job_id": null,
    "role": null,
    "title": "Imported session",
    "parent_session_id": null,
    "node": "vps",
    "cwd": "/srv/vowp",
    "state": "idle",
    "executor": null,
    "last_seq": 42,
    "last_capsule_id": "cap_01JCF5WQ8ZK3M2V7N4P6R8T0AG",
    "log_bytes": 38210,
    "created_at": "2026-11-14T12:05:00.000Z",
    "updated_at": "2026-11-14T12:05:00.000Z"
  },
  "report": {
    "adapter_version": "0.1.1",
    "asif_version": "0.4",
    "asif_pin": "652d9e23ddbe5656b9ceea311df7e09e93c1cd2f",
    "source_session": "01JCF5WQ8ZK3M2V7N4P6R8T0AB",
    "target_session": "01JCF5WQ8ZK3M2V7N4P6R8T0AF",
    "counts": {
      "events_in": 42,
      "events_out": 44,
      "resources": 3,
      "capsules": 1
    },
    "loss_count": 3,
    "losses": [
      {
        "kind": "capture-id-not-preserved",
        "stage": "translation",
        "scope": "native",
        "message": "path-free explanation"
      },
      {
        "kind": "cwd-rebound",
        "stage": "translation",
        "scope": "environment",
        "message": "path-free explanation"
      },
      {
        "kind": "session-id-changed",
        "stage": "translation",
        "scope": "native",
        "message": "path-free explanation"
      }
    ],
    "unsupported": []
  },
  "warnings": []
}
```

`Session` is the normal protocol row; not every field is repeated in the example's prose,
but the parser must accept the full `Session` shape from
`vowp_protocol::session::Session`.

#### 11.2.3 Loss, strict, and honesty rules

- Default export/import **succeeds with declared losses**. Every export has inherent
  losses (`cwd-omitted`, `no-causal-edges`, `contexts-reconstructed`,
  `configuration-unknown`, `tool-definition-unknown`, workspace/capsule limits). Every
  import has `session-id-changed`, `capture-id-not-preserved`, `cwd-rebound`.
- `--strict` refuses only `Blocking`/`Integrity` losses before the output/session becomes
  visible; inherent `Omission`/`Normalization` losses stay non-fatal.
- `--allow-loss` passes through to the adapter; it never makes unsupported semantics
  safe. `--allow-unsupported` is deliberately not exposed.
- A required unsupported feature (`continuation`, `streams`, `external_bindings`, an
  unknown required extension) exits 5 and names the feature. StackSpirit shows the
  refusal and does not offer a force switch.
- Import always creates a **new session**; there is no `--same-id` or `--executor`.
  Imported sessions are `idle`; StackSpirit never auto-runs a turn, writes a capsule,
  creates a lease, or dispatches a job after import.
- Warnings (`project ... left null`, `job ... left null`) are not losses; they are shown
  separately.
- Event identity is global: importing the same VOWP-origin package twice conflicts on
  event ids (exit 4) even under a new session id. StackSpirit explains the conflict; it
  does not retry with a mutated package.
- Limits: 16 MiB session/report JSON, 64 MiB single resource, 256 MiB total resources,
  272 MiB archive, 1,024 members, 4,096-character portable paths. Exceeding a limit is a
  typed error, never truncation.

#### 11.2.4 Exit codes

| Condition | Exit |
|---|---|
| Success; declared losses may exist and are reported | 0 |
| Internal adapter/write/I/O failure | 1 |
| Invalid package/JSON/schema/version/resource, unsafe path, torn source log, bad flag, oversized stdin | 2 |
| Unknown source session or remote node | 3 |
| Target session id/path exists, or imported event id belongs to another session | 4 |
| Strict refusal, required unsupported ASIF feature, other adapter refusal | 5 |
| Remote transport failure, truncated/corrupt export stream | 7 |

`TARGET_EXISTS` maps to 4, `BRANCH_AMBIGUOUS`/`BRANCH_UNKNOWN` to 2, every other adapter
`Refused` code to 5. StackSpirit maps these to its own codes and shows VOWP's message; it
does not remap a conflict to a retry.

#### 11.2.5 Probe algorithm and `AsifGate`

StackSpirit probes capabilities, not versions. The probe is read-only and safe:

1. **Connection required.** If the host is not connected, return `Unknown`.
2. **Version.** Run `vowp --version` (plain text). A failure is `Unreachable`.
3. **Export capability.** Run a harmless probe using a real project from the last
   snapshot (the first `active` project; if there is none, return `Unknown`):
   ```sh
   vowp --project <active-project> session export 00000000000000000000000000 \
     --format asif --out -
   ```
   on a non-TTY stdin/stdout over SSH.
   - exit 3 → the flag/value parsed and the session was simply not found ⇒ **export
     supported**.
   - exit 2 with a clap "invalid value" message ⇒ **export unsupported**.
   - any other exit or timeout ⇒ ambiguous; do not claim support.
   The nil ULID is a valid 26-character ULID that cannot collide in practice; the
   command writes nothing because no session exists.
4. **Import capability.** Run `vowp session import --help` and look for the exact
   `--format` value list that includes `asif`. This is a help-text probe and is therefore
   weaker; if the help is unavailable, changed, or ambiguous, set `import: false` and do
   not enable import. Do not infer import from export. When VOWP adds a
   `vowp capabilities --json` document (§13), use it and remove the help parse.
5. **Result.**
   ```rust
   AsifGate::Ready {
       export: true,
       import: false, // until independently proven
       detected_version: Some("0.4.0".into()),
       evidence: vec![
           "vowp --version: 0.4.0".into(),
           "session export --format asif: exit 3 (supported)".into(),
       ],
   }
   ```
   If neither capability is proven:
   ```rust
   AsifGate::WaitingForVowpAsif {
       missing: vec![
           "v4-asif-core".into(),
           "v4-asif-cli".into(),
       ],
       detected_version: Some("0.4.0".into()),
       evidence: vec!["session export --format asif: exit 2 (unsupported)".into()],
   }
   ```
   Agent/protocol mismatch or a parsed capability document that contradicts the probe is
   `Incompatible`, with both values named.

`AsifGate::Ready { export: true, import: false }` is a real, useful state. In D5 (when
`features.asif_actions` is true) the UI enables export and leaves import disabled with
"import capability not proven". It never labels the whole gate green if one action is
unavailable; the card shows the partial state.

#### 11.2.6 What StackSpirit builds before the gate is ready

- Sessions drawer fed by `snapshot.sessions`; read-only.
- **Export .asif.zip** and **Import .asif.zip** buttons rendered disabled with the gate
  reason and a **Details** drawer listing the milestones and probe evidence. Even with a
  synthetic `Ready` fixture, D4 keeps `features.asif_actions = false`, so the action says
  "not implemented in this build" rather than pretending to work.
- MockBridge scenarios for `gate-asif-waiting` and `gate-asif-ready` (synthetic, clearly
  labeled in preview) prove the gate rendering, not a live export.
- No production code path that opens a package, starts an import, or writes a session
  when the gate is not `Ready` or the feature flag is false.

#### 11.2.7 Security and honesty

- Exported packages may contain secrets; VOWP scrubs host paths by default but does not
  detect exact secret values. The desktop says so and points to the CLI `--redact-file`
  for a specific list. It never claims an export is secret-free or lossless.
- Packages and reports live in private temp directories (0700) and files (0600); the
  desktop deletes local temp files after use and asks VOWP to delete remote temp files.
- No uploaded/downloaded package is logged, indexed, or sent anywhere except the chosen
  VOWP home.
- The import response never echoes the uploaded filename or a staging path; StackSpirit
  shows the new session id, counts, losses, and warnings only.

### 11.3 Local orchestrator gate (D6)

#### 11.3.1 Required milestones

D6 requires the **Phase 2 offload executor** and the **Phase 3/4 local-orchestrator
surface** of the v0.4 plan:

| Phase | Scope | Why D6 needs it |
|---|---|---|
| Phase 2 (`v4-local-2e/2f/2g`) | offload executor: assignment, path map, worktree, events, summary, push/fetch, policy, capacity, waiting reasons | The local node transport, leases, spool, and run fences are prerequisites for a local surface. |
| Phase 3 (`v4-local-3*`) | local opencode app host, bridge, MCP proxy, orchestrator materialize, backfill, local cache | The actual local orchestrator session, lease, mirror, and backfill. |
| Phase 4 (`v4-local-4f-cli-surface`) | gateway `--listen`, agent service/doctor, `surface rotate`, `surface pair --replace`, and the CLI/JSON release surface | The commands and flags the gate probes. |

`v4-local-4f-cli-surface` is **merged in the VOWP source** (commit `121f319`, with the
follow-up `d523314`); its parent Phase 4 slices are merged or no longer block the CLI
surface. The installed `vowp 0.3.0` binary on this host is older: it has
`surface pair|ls|revoke` but no `surface rotate`, no `agent service|doctor`, and no
`orchestrator new` local-node after-help. Source presence is **not** installed capability.

D6 therefore remains gated until **all** of the following are true:

1. a build containing the merged Phase 3/4 surface is installed on the home **and** the Mac;
2. the home has an active local-agent node with protocol `vowp-agent/1`;
3. the Mac agent is enrolled and `connected`;
4. `features.local_orchestrator_actions` is enabled by a later D6 build.

The VOWP source still reports version `0.3.0`; a version string is never evidence. The
probes in §11.3.4 are what count.


#### 11.3.2 VOWP command surface and the agent frames D6 uses

The merged CLI has **no `vowp agent surface start|attach|detach` command**. That spelling in
`local-orchestrator.md` §12.2 is a design sketch; the merged local app host is created by
the home's `vowp orchestrator new --node <name> --app opencode`, which sends an
`AgentFrame::OrchestratorCommand(Start)` to the enrolled Mac agent. StackSpirit must never
require, probe, or display the stale spelling as a capability.

Home-side commands:

```sh
vowp --version
vowp --project <p> node ls --json
vowp --project <p> node show <node> --json
vowp offload status --json
vowp surface pair --session <id> --node <node> --kind opencode [--app <app>] [--replace <surface-id>]
vowp surface ls [--node <node>] [--json]
vowp surface revoke <surface-id> --yes
vowp surface rotate <surface-id> [--wait <duration>] [--grace <duration>]
vowp orchestrator new --node <node> --app opencode [--project <p>] [--force]
vowp orchestrator ls --json
vowp orchestrator stop <name>
vowp orchestrator resume <name>
```

Owner-only setup commands (never gate evidence, never run by the probe):

```sh
vowp node enroll <node> [--label <label>]...
vowp node revoke <node> --yes
```

Mac-side commands:

```sh
vowp agent enroll --target <ssh-target> --code-file <path|-> [--project <path>]... \
  [--worktree-root <path>] [--wss <url>] [--tls-fingerprint <sha256:hex>]
vowp agent paths add --project <absolute-checkout> [--name <project>] \
  [--worktree-root <path>] [--allow <rel>]...
vowp agent paths rm <project>
vowp agent paths ls [--json]
vowp agent pair --code-file <path|-> --surface <surface-id> --session <id> [--app <app>]
vowp agent run [--foreground] [--role surface|executor|both] [--service]
vowp agent status --json
vowp agent service install|uninstall|status
vowp agent doctor [--json]
vowp chat --agent --session <id>
vowp mcp --agent-socket <path> --surface <surface-id>
```

Which command proves each D6 capability:

| Capability | Exact probe | Side | Kind |
|---|---|---|---|
| Node model | `vowp --project <p> node ls --json` exits 0 with a node array | home | gate |
| Active local agent | `vowp --project <p> node show <node> --json` has `kind: "agent"`, `status: "active"`, `presence: "online"` | home | gate |
| Agent protocol | selected `NodeCard.protocol == "vowp-agent/1"` | home | gate |
| Agent version | selected `NodeCard.agent_version` is present | home | gate |
| Phase 2 offload executor | `vowp offload status --json` exits 0 with `nodes`, `assignments`, and `ready` arrays | home | gate |
| Surface pairing | `vowp surface pair --help` exits 0 and advertises `--session`, `--kind`, `--app`, `--replace`, and the local-node after-help | home | gate |
| Surface read model | `vowp surface ls --help` exits 0 | home | gate |
| Surface revocation | `vowp surface revoke --help` exits 0 and advertises `--yes` | home | gate |
| Surface rotation | `vowp surface rotate --help` exits 0 and advertises `--wait` and `--grace` | home | gate |
| MCP proxy | `vowp mcp --help` exits 0 and advertises `--surface` and `--agent-socket` | home | gate |
| Orchestrator materialization | `vowp orchestrator new --help` exits 0 and advertises `--app`, `--force`, and the local-node after-help | home | gate |
| Orchestrator lifecycle | `vowp orchestrator stop --help` and `vowp orchestrator resume --help` exit 0 | home | gate |
| Orchestrator read model | `vowp orchestrator ls --json` exits 0 with a JSON array | home | gate |
| Local agent connection | `vowp agent status --json` exits 0 with `connection: "connected"` and protocol `vowp-agent/1` | Mac | gate |
| Local surface pairing | `vowp agent pair --help` exits 0 and advertises `--code-file`, `--surface`, `--session` | Mac | gate |
| Local agent run | `vowp agent run --help` exits 0 and advertises `--foreground` and `--role` | Mac | gate |
| Local path map | `vowp agent paths --help` exits 0 and advertises `add`, `rm`, `ls` | Mac | gate |
| Local cache | `vowp agent status --json` reports `cache_entries` and `cache_bytes` | Mac | gate |
| Backfill | no separate command; it is the `vowp agent run` bridge/cursor behavior, covered by the run help plus a connected agent | Mac | implementation behavior |
| Service mode | `vowp agent service --help` exits 0 | Mac | optional local prerequisite |
| Local diagnostics | `vowp agent doctor --help` exits 0 | Mac | optional local prerequisite |
| Enrollment | `vowp agent enroll --help` exits 0; `vowp agent status --json` exit 3 before enrollment | Mac | owner setup |
| Node enrollment code | `vowp node enroll` succeeds on the home | home | owner setup (write) |
| Path map setup | `vowp agent paths add` / `rm` / `ls` | Mac | owner setup |

`vowp node paths <node> --json` **does not exist in the merged CLI** and is not a D6
probe. Local cache is a Mac-side read from `vowp agent status --json`; the home never reads
a Mac path map. `vowp agent service` and `vowp agent doctor` are optional: service mode is
an alternative to a foreground `vowp agent run`, and doctor is diagnostic. Neither makes a
gate `Ready`.

Agent frames used by the commands:

| Command | Frames |
|---|---|
| `vowp surface pair` (home) | Creates a `pending` surface and a one-time code; no agent frame until the Mac redeems it. |
| `vowp surface ls` (home) | Read-only `SurfaceInfo` list; no agent frame. |
| `vowp agent pair` (Mac) | `AgentFrame::SurfacePair { code, surface_id, session, app }`; the home replies `SurfaceIssued { surface_id, token, scopes, expires_at }`. |
| `vowp surface rotate` (home) | Queues an `AgentFrame::SurfaceIssued` with the replacement token; the old token stays valid for the ten-minute overlap. |
| `vowp surface revoke` (home) | Queues `AgentFrame::SurfaceRevoked { surface_id, reason }`. |
| `vowp agent run` (Mac) | `SurfaceAttach`, `SurfaceEvent`, `SurfaceTurn`, `SurfaceDetach`, `LeaseAcquire`, `LeaseHeartbeat`, `LeaseRelease`. |
| `vowp mcp --agent-socket` (Mac) | `AgentFrame::ToolCall` to the home and `ToolResult` back. |
| `vowp orchestrator new --node` / `vowp orchestrator stop` / `vowp orchestrator resume` (home) | `vowp orchestrator new --node` sends `AgentFrame::OrchestratorCommand(Start)`; `stop`/`resume` send `Stop`/`Resume`; the agent replies `OrchestratorResult`. `--node` has the local-node meaning only on `new`; on `stop`/`resume` it remains the global SSH selector. |

Protocol facts that are frozen enough to check:

- Agent protocol name: `vowp-agent/1` (`vowp_protocol::agent::AGENT_PROTOCOL`).
- Node presence is computed: online under 30 s since the last heartbeat, degraded
  30–90 s, offline after 90 s. D6 requires `online` for a live action; `degraded` is
  honest but keeps the gate `Waiting`.
- Node kinds: `home`, `ssh`, `agent`; enrollment status: `pending`, `active`, `revoked`.
- A local surface needs a valid surface token **and** the current lease token; neither
  alone is enough.
- The home owns the canonical orchestrator record, session log, lease, and snapshot. The
  Mac materializes a local config under `~/.vowp-agent/orchestrators/<name>/` and starts
  the local opencode service. The home never starts a host on the home for this flow.
- No absolute Mac path appears in shared state; `PathRef` (`project:`, `worktree:`,
  `subdir:`) is the wire shape.

`vowp node ls --json` returns the unchanged v0.3 `Node` shape:
`name`, `ssh_target`, `is_local`, `capabilities`, `last_seen_at`, `created_at`. It has
**no `kind` or `status`**. `vowp node show <name> --json` returns the redacted `NodeCard`
from `vowp-protocol::agent`; it may contain local paths and is owner-only, so its raw body
is never copied into shared state or gate evidence:

| `NodeCard` field | Type | Null? | Meaning |
|---|---|---|---|
| `name` | `NodeName` | no | Node name. |
| `kind` | `NodeKind` (`home`, `ssh`, `agent`) | no | How the node is reached. |
| `status` | `NodeStatus` (`pending`, `active`, `revoked`) | no | Enrollment state. |
| `presence` | `NodePresence` (`online`, `degraded`, `offline`, `revoked`) | no | Computed presence. |
| `labels` | string[] | no | Owner labels. |
| `capacity` | `AgentCapacity` | yes | Latest capacity, when one arrived. |
| `runs` | u32 | no | Active runs. |
| `mapped` | `ProjectName[]` | no | Mapped projects. |
| `agent_version` | string | yes | Agent binary version, when known. |
| `protocol` | string | yes | Agent protocol, when known. |
| `last_error` | string | yes | Redacted last error. |

`vowp orchestrator ls --json` now returns `OrchestratorCard` with an optional `node`
(`vowp-protocol::surface`); `None` means a v0.2 home-hosted orchestrator. An installed build
may still omit it, so the gate's node comes from `node show`, never from this list.

`vowp agent status --json` is a **Mac-side** command, not a home command. It reads
`~/.vowp-agent/status.json` and prints a CLI-local `AgentStatus` document plus `surfaces`
and `paths` arrays (the `paths` array is owner-only and never evidence):

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
  "surface_token_expires_at": null,
  "cache_entries": 12,
  "cache_bytes": 4096,
  "cache_oldest_at": "2026-11-14T09:00:00.000Z",
  "surfaces": [],
  "paths": []
}
```

`connection` is one of `unknown`, `connecting`, `connected`, `reconnecting`, `revoked`,
`superseded`, `stopped`. Without an enrollment it exits 3 with the `vowp agent enroll`
hint; it also exits 0 when the last state is stopped/offline.

`SurfacePair`, `SurfaceInfo`, and `AgentFrame` are frozen in `vowp-protocol::agent`;
StackSpirit parses only the fields D6 uses and tolerates additions. It does not need
`AgentStatus` over SSH and never runs a Mac-side command against the home.


#### 11.3.3 Contract gaps that keep D6 gated

1. **`OrchestratorCard.node` is optional.** The merged protocol has
   `Option<NodeName>` (`vowp-protocol::surface`), but an installed build may omit it and a
   v0.2 card legitimately has `None`. StackSpirit must not infer a local node from an
   absent field. The gate's node comes from `node show`, not from `orchestrator ls`.
2. **`vowp fleet status --json` stays the v0.2 snapshot in the MVP.** The plan says the
   human output adds a node summary, but the JSON snapshot does not gain a `nodes` array
   in the MVP. StackSpirit uses `vowp node ls --json` / `node show --json` for nodes and
   does not expect a node section in the snapshot. The exact `node ls --json` envelope is
   not frozen for D6; the probe accepts the documented machine-readable node model and
   never guesses if the shape is different.
3. **`CapacityReport` has no node section yet.** StackSpirit does not fabricate a
   local-node capacity panel; it shows home capacity and, only when a documented node
   section exists, a separate local section.
4. **No capability document.** Without `vowp capabilities --json`, the gate must use
   command/exit-code probes and conservative help parsing. This is the main reason the
   gate can stay `Waiting` or `Unknown` rather than guess.
5. **`vowp node paths` is not a CLI command.** The design sketch in
   `local-orchestrator.md` §12.1 lists `vowp node paths <name> --json`, but the merged
   `vowp-cli` has no such subcommand. D6 must not probe it. The local cache is a Mac-side
   read from `vowp agent status --json` (`cache_entries`, `cache_bytes`,
   `cache_oldest_at`), and the home never reads a Mac path map.
6. **Phase 0/1 commands look like progress.** `vowp node ls`, `vowp node show`,
   `vowp surface ls`, and `vowp orchestrator ls` exist before the complete local surface.
   They are necessary but not sufficient evidence; the probe also requires the Phase 2
   offload document, the `surface pair|ls|revoke|rotate` lifecycle, `mcp --agent-socket`,
   and the local-node `orchestrator` surface.
7. **Help text can false-positive on the global `--node` selector.** `vowp surface pair
   --help`, `vowp surface rotate --help`, and `vowp orchestrator new --help` all show
   `--node` in the global options as "Run the command on that node over ssh". That line is
   not proof of the local-node meaning. The probe requires the command-specific after-help
   sentence ("names the node the surface runs on" / "names the node that runs the app",
   plus "not an SSH redirect"). The top-level `vowp surface --help` sentence is not
   accepted for `surface pair`.
8. **The installed binary can lag the merged source.** The installed `vowp 0.3.0` lacks
   `surface rotate`, `agent service`, `agent doctor`, and the `orchestrator new` local-node
   after-help. That is a missing capability, not a false negative. D6 stays `Waiting`
   until a build containing the merged surface is installed on both machines.

#### 11.3.4 Probe algorithm and `LocalOrchestratorGate`

The gate has two halves, and **both must pass before `LocalOrchestratorGate::Ready`**:

- **Home capability** (over SSH): does this VOWP home support the local-orchestrator
  command surface?
- **Local agent** (on the Mac): is a local `vowp agent` installed, enrolled, and
  `connected`, with the required local commands and cache read?

`LocalAgentStatus` is still displayed separately so the owner can see why the Mac half
fails, but a non-connected local agent is never reported as `Ready`. The probe must
distinguish **Phase 0/1** (agent enrollment, presence, node model, surface pairing) from
**the complete local surface** (app host, bridge, MCP proxy, orchestrator materialize,
backfill, local cache). `vowp agent status` and `vowp node ls` exist before the complete
surface and are therefore **not** sufficient evidence on their own.

1. **Connection required** for the home half; otherwise `Unknown`.
2. **Version.** `vowp --version` over SSH. Failure ⇒ `Unreachable`. A parsed version is
   evidence only, never a gate.
3. **Node inventory.** `vowp --project <p> node ls --json` over SSH. This is the v0.3
   `Node` shape (names and SSH targets, no kind/status). A malformed array is
   `Incompatible`; a failed command or no nodes is `WaitingForVowpPhase3` ("node model").
   On a multi-project home the `--project` selector is required; without a usable active
   project the gate is `Waiting` ("active project") and no bare node read is run.
4. **Node card.** For each node name (bounded to 16, in the order returned), run
   `vowp --project <p> node show <name> --json` and parse the `NodeCard`. Require at least
   one card with `kind == "agent"`, `status == "active"`, and `presence == "online"`.
   `degraded` is honest but keeps the gate `Waiting`; an offline agent card is never
   `Ready`. If none, ⇒ `WaitingForVowpPhase3` ("no active local agent node"). If a card is
   missing `kind`/`status`/`presence`, or is not a `NodeCard`, ⇒ `Incompatible`.
5. **Protocol and version.** From the selected `NodeCard.protocol`:
   - `vowp-agent/1` ⇒ compatible so far;
   - any other value ⇒ `Incompatible { detected, required: "vowp-agent/1" }`;
   - missing ⇒ `WaitingForVowpPhase3` ("agent protocol not reported"), never `Ready`.
   A missing `agent_version` adds "agent version not reported".
6. **Phase 2 offload executor.** `vowp offload status --json` exits 0 with the documented
   `nodes`, `assignments`, and `ready` arrays. A malformed document adds "offload executor
   (Phase 2)" and stays `Waiting`; it is not a transport failure.
7. **Home surface lifecycle.** These are the discriminating probes:
   - `vowp surface pair --help` exits 0 and advertises `--session`, `--kind`, `--app`,
     `--replace`, and the command-specific local-node after-help;
   - `vowp surface ls --help` exits 0;
   - `vowp surface revoke --help` exits 0 and advertises `--yes`;
   - `vowp surface rotate --help` exits 0 and advertises `--wait`, `--grace`, and the
     command-specific local-node after-help.
   Any missing command or flag ⇒ `WaitingForVowpPhase3` with the exact labels "surface
   pairing", "surface read model", "surface revocation", or "surface rotation".
8. **MCP proxy.** `vowp mcp --help` exits 0 and advertises `--surface` and
   `--agent-socket`. Missing ⇒ `Waiting` ("MCP proxy").
9. **Orchestrator surface.**
   - `vowp orchestrator new --help` exits 0 and advertises `--app`, `--force`, and the
     command-specific local-node after-help; missing/ambiguous ⇒ `Waiting` ("orchestrator
     materialization").
   - `vowp orchestrator stop --help` and `vowp orchestrator resume --help` exit 0; missing
     ⇒ `Waiting` ("orchestrator lifecycle").
   - `vowp orchestrator ls --json` exits 0 with a JSON array; missing/malformed ⇒
     `Waiting` ("orchestrator read model"). This read is supporting evidence, not proof of
     the local host.
   The global `--node` line in any of these helps is never accepted as proof; only the
   command-specific after-help sentence counts. A help probe is weaker than a JSON
   capability document; if the output is ambiguous, return `Waiting`, never `Ready`.
10. **Local agent half.** Run `vowp agent status --json` on the **Mac**, not over SSH,
    via the local `CommandRunner`.
    - local `vowp` missing ⇒ `LocalAgentState::NotInstalled`.
    - exit 3 (no enrollment) ⇒ `LocalAgentState::NotEnrolled`.
    - exit 0 ⇒ map `connection` exactly: `unknown`, `connecting`, `connected`,
      `reconnecting`, `revoked`, `superseded`, `stopped`. Carry `node`, `protocol`,
      `last_heartbeat`, and the redacted `last_error` into `LocalAgentStatus`.
    - protocol mismatch ⇒ `LocalAgentState::Incompatible`.
    In addition, require the local commands that make the surface usable:
    - `vowp agent pair --help` advertises `--code-file`, `--surface`, `--session`;
    - `vowp agent run --help` advertises `--foreground` and `--role` (this is also the
      bridge/backfill path; there is no separate backfill command);
    - `vowp agent paths --help` advertises `add`, `rm`, `ls`;
    - `vowp agent status --json` reports `cache_entries` and `cache_bytes`.
    `vowp agent service` and `vowp agent doctor` are optional diagnostics, recorded as
    evidence but not required. A non-`connected` state, a missing local command, or a
    missing cache field keeps the gate `Waiting` with the exact missing label; it is never
    `Ready`.
11. **Result.**
    ```rust
    LocalOrchestratorGate::Ready {
        agent_protocol: "vowp-agent/1".into(),
        node: Some("mac-studio".into()),
        capabilities: vec![
            "node_card".into(),
            "offload_status".into(),
            "surface_pair".into(),
            "surface_list".into(),
            "surface_revoke".into(),
            "surface_rotate".into(),
            "mcp_agent_socket".into(),
            "orchestrator_new_node".into(),
            "orchestrator_lifecycle".into(),
            "orchestrator_read".into(),
            "local_agent_connected".into(),
            "local_surface_pair".into(),
            "local_agent_run".into(),
            "backfill".into(),
            "local_path_map".into(),
            "local_cache".into(),
        ],
        evidence: vec![
            "vowp node show --json: kind=agent status=active presence=online".into(),
            "vowp offload status --json: exit 0".into(),
            "vowp surface pair --help: exit 0".into(),
            "vowp surface rotate --help: exit 0".into(),
            "vowp mcp --help: exit 0".into(),
            "vowp orchestrator new --help: exit 0".into(),
            "vowp orchestrator ls --json: exit 0".into(),
            "vowp agent status --json: cache stats reported".into(),
        ],
    }
    ```
    Missing commands produce:
    ```rust
    LocalOrchestratorGate::WaitingForVowpPhase3 {
        missing: vec![
            "no active local agent node".into(),
            "surface rotation".into(),
            "orchestrator materialization".into(),
            "local agent connection".into(),
        ],
        detected_version: Some("0.3.0".into()),
        evidence: vec![
            "vowp node show --json: kind=home status=active presence=offline".into(),
            "vowp surface rotate --help: exit 2".into(),
            "vowp orchestrator new --help: required flags/text not advertised".into(),
            "vowp agent status --json: not_enrolled".into(),
        ],
    }
    ```
12. **Never version-only and never home-only.** A VOWP version that "looks like" Phase 3
    is not evidence. A Phase 0/1 `agent status` success is not evidence, and `agent status`
    is never run against the home. A `Ready` home with a not-installed, not-enrolled, or
    stopped local agent is not `Ready` for D6. Only the home and Mac capability reads in
    this section count, and `features.local_orchestrator_actions` must still be enabled by
    a later D6 build for any action.


#### 11.3.5 What StackSpirit builds before the gate is ready

- The Engine view gate card with `WaitingForVowpPhase3`, the capability list, a
  **Details** drawer, and a disabled **Run locally** action. In D4
  `features.local_orchestrator_actions = false`, so a synthetic `Ready` fixture shows
  "VOWP is ready; local orchestrator execution is not implemented in this build."
- `LocalAgentStatus` card with the actual connection states: unknown, not installed,
  not enrolled, connecting, connected, reconnecting, revoked, superseded, stopped, or
  incompatible; no secret and no absolute path in the shared view.
- Fixtures `gate-phase3-waiting`, `gate-phase3-ready`, and preview mode clearly labeled;
  `gate-phase3-ready` proves the disabled-action copy, not a live orchestrator.
- The local engine can still be configured and health-checked in D4; it simply does not
  attach to a VOWP session until the gate is `Ready` and the local agent is online.

#### 11.3.6 What D6 will do when ready

- Home-side pairing and lifecycle: `vowp surface pair` / `surface ls` /
  `surface rotate` / `surface revoke`.
- Mac-side pairing and run: `vowp agent pair` with a private code file, then
  `vowp agent run` (foreground or service mode). Never a second supervisor or a second
  `agent run`.
- Local app host: `vowp orchestrator new --node <local> --app opencode`, then
  `vowp orchestrator stop|resume`; the home sends `OrchestratorCommand` frames and the
  agent materializes the app. There is no `agent surface start/attach/detach` call.
- MCP: `vowp mcp --agent-socket <path> --surface <id>`; a chat surface uses
  `vowp chat --agent --session <id>`.
- Display the node, protocol, lease holder, and session state from VOWP documents.
- Surface takeover/backfill/degraded states exactly; no optimistic "running" claim.
- Keep provider keys local; VOWP receives provider/model names and environment names, not
  values.
- Refuse to start when the gate is not `Ready`, the local agent is offline, the path map
  is invalid, or the protocol is incompatible. The refusal names the missing
  prerequisite.

#### 11.3.7 Security invariants for D6

- One home. The Mac never opens a VOWP database and never keeps a second session log.
- One lease. The local surface is read-only without the lease.
- No credential copying: the node key proves the node, the surface token scopes the
  surface, the lease token authorizes the writer; no home secret value crosses.
- No absolute local paths in shared state; `PathRef` only.
- The local agent never runs as root; the app never asks for root.
- A revoked node fails the next handshake; StackSpirit stops and explains.

### 11.4 Gate serialization

The bridge sends the gate states as tagged unions (see `PLAN.md` §5.2). Example
`GateSnapshot`:

```json
{
  "probed_at": "2026-11-14T12:06:00.000Z",
  "local_orchestrator": {
    "state": "waiting_for_vowp_phase3",
    "missing": [
      "no active local agent node",
      "surface rotation",
      "orchestrator materialization",
      "local agent connection"
    ],
    "detected_version": "0.3.0",
    "evidence": [
      "vowp node show --json: kind=home status=active presence=offline",
      "vowp surface rotate --help: exit 2",
      "vowp orchestrator new --help: required flags/text not advertised",
      "vowp agent status --json: not_enrolled"
    ]
  },
  "asif": {
    "state": "waiting_for_vowp_asif",
    "missing": ["v4-asif-core", "v4-asif-cli"],
    "detected_version": "0.4.0",
    "evidence": ["session export --format asif: exit 2 (unsupported)"]
  },
  "local_agent": {
    "state": "not_installed",
    "node": null,
    "protocol": null,
    "last_heartbeat": null,
    "last_error": null
  },
  "features": {
    "asif_actions": false,
    "local_orchestrator_actions": false
  }
}
```

`evidence` strings contain commands, exit codes, versions, and capability names. They
never contain a token, a key, a session body, an absolute local path, or a home path.

### 11.5 Probe lifecycle and failure handling

- **When:** on connect, after a VOWP version change is observed, after a manual
  **Re-probe**, and no more than once per 10 minutes automatically.
- **Cache:** in memory, per host connection. A cached `Ready` older than 10 minutes is
  shown with "last probed 12m ago" and a **Re-probe** link. A stale `Ready` does not
  silently enable a destructive action; the action re-probes first.
- **Failure:** a transport failure is `Unreachable`, not `Waiting`. A malformed but
  present capability document is `Incompatible`, not `Waiting`. A timeout is
  `Unreachable` with a retry hint.
- **Downgrade:** if a later probe fails, the gate moves back to `Waiting`/`Unreachable`
  immediately and the UI disables the action. There is no sticky `Ready`.
- **No network:** gate probes run over the same SSH connection or the local
  `CommandRunner`; they do not call a public API, GitHub, or a version service.

### 11.6 UI rules for both gates

- The gate badge in the top bar is one of: **Unknown**, **Probing**, **Waiting**,
  **Incompatible**, **Ready**, **Unreachable**. It is never a bare green dot without a
  successful probe.
- The gate card names the exact missing milestones/capabilities. It does not say
  "coming soon" without them.
- The action button is disabled unless the relevant capability is ready **and** the
  corresponding StackSpirit feature flag is enabled (`features.asif_actions` /
  `features.local_orchestrator_actions`). In D4 both flags are false, so a VOWP-ready
  gate still shows "not implemented in this build", never a dead control.
- The disabled reason is visible next to the button, not only in a tooltip.
- A **Details** drawer shows the probe evidence: command, exit code, parsed version,
  timestamp. It never shows a raw response body, a path, or a secret.
- Preview fixtures that simulate `Ready` are labeled **Preview data** in every view and
  are not shipped in release builds.
- The gate does not block the rest of the app. Fleet, capacity, inbox, and engine
  onboarding work while both gates are waiting.

### 11.7 Milestone ledger and change management

| Gate | Milestone family | What must be true for `Ready` | Contract evidence |
|---|---|---|---|
| ASIF | `v4-asif-core`, `v4-asif-cli` | Export is proven by the `session export --format asif` probe; import is proven independently by the import help/capability probe. `AsifGate::Ready` may be partial (`export: true, import: false`); full D5 requires both. | Probe result + `AsifGate` evidence |
| ASIF (web, optional) | `v4-asif-web` | Daemon routes exist for a future Web app | Not required by the desktop |
| Local orchestrator | Phase 2 `v4-local-2e/2f/2g` + Phase 3 `v4-local-3*` + Phase 4 `v4-local-4f-cli-surface` | `vowp node show <name> --json` has an active agent node with protocol `vowp-agent/1`; `vowp offload status --json` succeeds; `vowp surface pair` / `ls` / `revoke` / `rotate`, `vowp mcp --agent-socket`, and the local-node `vowp orchestrator new` / `ls` / `stop` / `resume` surface are present; the Mac agent is `connected` and advertises `agent pair` / `run` / `paths` plus cache stats | Probe result + `LocalOrchestratorGate` evidence |

Change procedure:

1. VOWP merges a milestone or changes a document.
2. Update `docs/VOWP-CONTRACT.md` (this file) and `docs/PLAN.md` if the UI changes.
3. Add/update golden fixtures in `crates/spirit-core/fixtures/vowp/`.
4. Update the contract tests and the gate probe tests in the same change.
5. Run both the positive (`Ready`) and negative (`Waiting`/`Incompatible`) fixtures.
6. Record the exact VOWP commit/version in the job report.

No StackSpirit job may edit `/home/dev/vowp`. No StackSpirit test may depend on a live
VOWP checkout, a real home, or the owner's SSH keys.

---

## 12. Contract tests and fixtures

### 12.1 Golden fixture set

Fixtures live in `crates/spirit-core/fixtures/vowp/` and are copied from this document,
not from a live home. They contain documentation addresses and fake values only.

| Fixture | Document | Key assertions |
|---|---|---|
| `fleet-status-fresh.json` | `FleetStatus` + `Snapshot` | Every snapshot field, one project, running/queue/blocked cards, measured sample, target, brake, limits, inbox, sessions, orchestrators. |
| `fleet-status-stale-cache.json` | `FleetStatus` | `source: "cache"`, `stale: true`, UI disables writes. |
| `fleet-status-supervisor-down.json` | `FleetStatus` | `supervisor.running: false`, old `last_tick`, amber state. |
| `capacity-report-full.json` | `CapacityReport` + extras | `brakes` plural, estimates, waiting, limits, `provider_caps`, `reload_pending`. |
| `capacity-report-empty.json` | `CapacityReport` | `host: null`, no estimates, no waiting, no limits. |
| `fleet-queue.json` | `FleetQueueRow[]` | Array shape, title, waiting reason, priority. |
| `fleet-queue-cap.json` | `FleetQueueRow[]` | 200 rows; UI labels truncation. |
| `fleet-inbox.json` | `InboxItem[]` | Full body, options, open/answered rows. |
| `error-not-found.json` | error document | `kind`, `message`, `hint`, exit 3 mapping. |
| `error-guard.json` | error document | `kind: "guard"`, exit 5 mapping for target-above-advice. |
| `snapshot-unknown-enums.json` | `FleetStatus` | A new state/harness becomes **Unknown**, not a known state. |
| `snapshot-multi-project.json` | `FleetStatus` | Project filtering and per-project counts. |

### 12.2 Required tests

`crates/spirit-core/tests/contract.rs`:

- `fleet_status_fixture_round_trips_documented_fields`
- `snapshot_schema_and_version_are_exact`
- `snapshot_capacity_card_parses_every_host_sample_field`
- `snapshot_project_cards_parse_running_blocked_queue_and_gates`
- `snapshot_inbox_summary_has_no_body_field`
- `snapshot_session_surface_card_parses_surface_ref`
- `snapshot_orchestrator_card_does_not_assume_node`
- `capacity_report_parses_brakes_plural_and_cli_extras`
- `capacity_report_parses_estimates_and_waiting`
- `fleet_queue_is_an_array_with_waiting_reason`
- `fleet_inbox_is_an_array_with_full_body`
- `error_document_maps_every_exit_code`
- `timestamp_regex_and_canonical_round_trip`
- `unknown_enum_value_is_unknown_not_ready`
- `unknown_object_field_is_ignored`
- `missing_required_field_fails_with_document_name`
- `no_fixture_contains_a_secret_pattern`

`crates/spirit-core/tests/gates.rs`:

- `asif_probe_waiting_when_export_flag_is_unsupported`
- `asif_probe_ready_export_only_when_import_help_is_ambiguous`
- `asif_probe_ready_both_when_export_and_import_are_proven`
- `asif_probe_never_ready_from_version_alone`
- `phase3_probe_waiting_when_surface_command_is_absent`
- `phase3_probe_incompatible_on_wrong_agent_protocol`
- `phase3_probe_does_not_accept_agent_status_and_node_model_alone`
- `phase3_probe_ready_only_with_offload_and_surface_capabilities`
- `phase3_probe_requires_orchestrator_new_node`
- `phase3_probe_separates_home_capability_from_local_agent_status`
- `gate_evidence_has_no_paths_or_secrets`
- `gate_cache_expires_and_downgrades`

`crates/spirit-core/tests/ssh.rs`:

- `ssh_local_process_uses_an_argv_array`
- `ssh_remote_command_quotes_dynamic_arguments`
- `ssh_remote_command_has_no_pipeline_or_operators`
- `ssh_identities_only_is_set_only_with_an_identity_file`
- `ssh_argv_uses_strict_host_key_checking`
- `ssh_read_command_uses_the_expected_vowp_json_forms`
- `ssh_output_cap_is_a_typed_error`
- `ssh_timeout_is_retryable_remote_timeout`

### 12.3 Drift policy

| Drift | Behavior |
|---|---|
| New object field | Ignored; no failure. Add a fixture and a test if the UI needs it. |
| New enum value | Parsed as **Unknown**; UI shows an explicit chip. Add the value to the contract before mapping it to a known lane. |
| Removed required field | Parse error for that document; keep the last good view; show `parse` with the document name. If the schema/version also changed, show `vowp_incompatible` instead. |
| Changed `snapshot.schema`/`version` | `vowp_incompatible`; do not parse heuristically. |
| Changed command flag | Gate probe fails or returns `Incompatible`; the app does not guess a replacement. |
| Changed ASIF report shape | Contract test fails; D5 remains gated until the fixture and doc are updated. |
| Changed Phase 3 protocol | `Incompatible` with both versions named; D6 remains disabled. |

### 12.4 Live smoke (manual, never CI)

The optional live check is a local developer action:

```sh
# Local shell only. Do not commit these values or put them in a .env file.
SPIRIT_LIVE_HOST=<owner-provided-host> cargo test -p spirit-core --features live-smoke -- --ignored
```

It must use the owner's SSH agent, a host key already pinned by the app, and a temporary
home where possible. It never prints a key, a token, a session body, or a host path.

---

## 13. Open contract asks and risks

These are requests to the VOWP project, not assumptions. StackSpirit works without them
where noted.

### 13.1 Proposed `vowp capabilities --json`

A stable capability document would remove the brittle help/exit-code probes:

```json
{
  "protocol": 1,
  "snapshot_schema": "vowp-snapshot/1",
  "agent_protocol": "vowp-agent/1",
  "features": {
    "asif_export": true,
    "asif_import": true,
    "local_orchestrator": true,
    "node_model": true,
    "node_capacity": false
  },
  "milestones": ["v4-asif-core", "v4-asif-cli", "v4-local-3-app-host"]
}
```

If VOWP adds it, StackSpirit prefers it and keeps the probes as a fallback. The document
must not contain secrets or host paths. Until then, §11.2.5 and §11.3.4 are the contract.

### 13.2 Orchestrator node field

`OrchestratorCard.node` (or `vowp orchestrator show <name> --json` with a `node` field)
is required to display a local orchestrator honestly. The local-orchestrator design
already describes `node=mac-studio` in the card; the protocol type does not have it.
This is the top D6 contract gap.

### 13.3 Node capacity and node section

If VOWP adds a node section to `capacity report` / `fleet status`, StackSpirit will show
a separate **this Mac** capacity panel. Until then, the capacity rail is home-only and
says so. A future `vowp node capacity --json` is also acceptable.

### 13.4 Job titles for running/blocked cards

`RunningJobCard` and `BlockedJobCard` omit titles. StackSpirit shows job ids and fetches a
title only when the user opens a drawer (when a stable job-detail document exists). A
future `title` field or a `vowp fleet jobs --json` document would improve the canvas.
This is a UX limitation, not a correctness bug.

### 13.5 Stable ASIF export stream for desktop clients

The public CLI supports either a JSON summary (`--json`) or raw bytes (`--out -`), not
both. The hidden `--asif-stream` is an internal CLI-to-CLI protocol. A stable
desktop-facing export that returns a summary plus the package (or a documented hash)
would remove the two-step remote-temp dance and let StackSpirit verify integrity. Until
then, D5 uses the public two-step path and verifies the byte count.

### 13.6 Queue/inbox array stability

`vowp fleet queue --json` and `vowp fleet inbox --json` are arrays today. If either gains
a wrapper object or a cap, the contract must document it; StackSpirit will show a
truncation label rather than assume completeness.

### 13.7 Risks

| Risk | Impact | Mitigation |
|---|---|---|
| VOWP snapshot grows beyond the parse budget | Slow canvas, memory | Off-thread parse, cap, last-good view, profile with a large fixture. |
| VOWP changes `brake`/`brakes` naming | Silent empty panel | Contract test asserts both fields explicitly. |
| `source: "cache"` is missed | Stale data looks live | Treat cache as stale regardless of `generated_at`. |
| ASIF 0.4 is a review draft | Claims become false | No conformance/lossless wording; loss list always shown. |
| Phase 3 contract changes | D6 rework | Gate on probes; no Phase 3 types in D1–D4. |
| Help-text probe changes | Gate stuck at Waiting | Conservative by design; capabilities document ask. |
| Pinned ASIF adapter version changes | Report fields drift | `adapter_version`/`asif_pin` are checked and displayed. |
| Free/included model availability changes | First-run dead end | Runtime verification and a BYOK empty state. |
| Windows OpenSSH missing | Cannot connect | Detect and explain; future `russh` backend. |

---

## 14. Reference command summary

Reads:

```sh
vowp --version
vowp --json fleet status
vowp --json capacity report
vowp --json fleet queue
vowp --json fleet inbox --all
```

Writes (D1–D4):

```sh
vowp --json capacity set --target <n> --mode <auto|fixed> [--override] --reason "<text>" --by stackspirit
vowp --project <job-project> --json job retry|park|cancel <job-id>
vowp --project <item-project> --json inbox answer <inbox-id> "<text>" --by stackspirit
```

Gate probes (read-only; see §11):

```sh
vowp --project <active-project> session export 00000000000000000000000000 --format asif --out -
vowp session import --help
vowp agent status --json                # locally only (Mac-side)
vowp node ls --json
vowp orchestrator ls --json
```

D5/D6 commands are listed in §11.2.2 and §11.3.2 and are not run while the gate is not
`Ready`.

---

## 15. See also

- [`PLAN.md`](PLAN.md) — build order, module layout, visual specification, test strategy.
- `/home/dev/vowp/docs/design/stackspirit.md` — product and desktop-first execution plan.
- `/home/dev/vowp/docs/design/v0.4/local-orchestrator.md` — Phase 0–4, agent protocol,
  leases, offload, commands.
- `/home/dev/vowp/docs/design/v0.4/asif-integration.md` — ASIF contract, losses, limits,
  milestone split.
- `/home/dev/vowp/crates/vowp-protocol/src/` — exact Rust types.
