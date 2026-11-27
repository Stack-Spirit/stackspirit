# Web Surface Contract — W1

> **Historical document.** This is the frozen W1 **pre-build** contract from 2026-11-15,
> kept for the transport, data-honesty and bridge rules that still govern the daemon
> cockpit. Its “read-only Web shell with no browser token” statements describe the
> no-account `/` cockpit; the shipped app also has the optional account/Always-On Homes
> route at `/cloud`. The current app is described in
> [Current Web/PWA state (0.6.1+)](#11-current-webpwa-state-061) at the end of this file.

Status: frozen pre-build contract, 2026-11-15. The W1a read-only implementation is
recorded in §9.
Audience: the W1 Web job, `packages/spirit-ui`, and the VOWP daemon owner.

This document is the contract for `apps/spirit-web` **before any Web app code exists**. It
maps the current `vowp-daemon` HTTP/SSE API to the `SpiritBridge` groups (`hosts`,
`connection`, `fleet`, `engine`, `gates`), names every gap, and fixes the honest read-only
fallback so a Web build cannot invent capacity, supervisor, or host data.

The desktop app is the reference implementation. This document does not change the desktop
behaviour; it names the exact bridge additions the Web app needs.

Sources pinned for this document:

- VOWP `docs/design/v0.3/http-api.md`, `docs/design/v0.3/web-window.md`.
- VOWP `crates/vowp-daemon/src/app.rs`, `stream.rs`, `auth.rs`, `main.rs`
  (read-only).
- VOWP `crates/vowp-protocol/src/api.rs`, `job.rs`, `session.rs`, `inbox.rs`.
- VOWP `crates/vowp-core/src/ledger/board.rs` (the exact `Board` calculation).
- StackSpirit `docs/PLAN.md` §9.9 and §10.4, `docs/VOWP-CONTRACT.md`,
  `packages/spirit-ui/src/bridge/SpiritBridge.ts`, `wire.ts`, `state/fleetStore.ts`.

If the daemon and this document disagree, the daemon wins; that is a contract failure to fix
in both places, not a reason to guess at runtime.

---

## 0. Decision summary

| # | Decision | Consequence |
|---|---|---|
| 1 | W1 is **read-only first** (W1a). | The Web shell makes no `POST` in W1a. Job actions and inbox answering stay hidden until W1c. |
| 2 | The Web app talks to **one daemon, one project**. | No project switcher, no multi-host rail, no `All projects`. The daemon serves one project selected by `vowp up`. |
| 3 | **Capacity detail is unavailable.** There is no capacity/storage read route. | The UI shows the exact copy in §3.7, never zeroes, never a guessed worker target. |
| 4 | **Host management and engine lifecycle are desktop-only.** | The Web shell hides those surfaces or shows the exact desktop-only notice. No bridge call is attempted. |
| 5 | Reads are **token-free**; writes are bearer-token gated. | The browser never holds `VOWP_API_TOKEN` or a provider key. W1c uses a narrow local write proxy. |
| 6 | The daemon sends **no CORS headers**. | The Web app and `/api/v1` must be same-origin behind an explicit reverse proxy (or the Vite dev proxy). A separate-origin SPA cannot call the daemon. |
| 7 | The daemon binds `127.0.0.1` only. | Never expose it directly. A public deployment is a reverse proxy that authenticates users itself. |
| 8 | The capacity/fleet routes are a **VOWP v0.3 API gap**, not an existing `v4-*` milestone. | Do not claim they exist and do not block on an invented milestone. |

---

## 1. Transport

### 1.1 Daemon address discovery

`vowp up` starts `vowp-daemon` as a separate child process:

- The daemon binds `127.0.0.1` and `--port 0` by default, so the OS chooses an ephemeral
  port. It prints exactly one line:
  `vowp-daemon: listening on http://127.0.0.1:<port>`.
- The supervisor prints `web window: http://127.0.0.1:<port>/` and, for a human on another
  machine, the exact
  `vowp tunnel open window --remote-port <port>` hint.
- `vowp up --json` keeps the optional address in the `web_window` field; the supervisor also
  writes the address to the home's `web-window` file for a detached `up`.

There is no fixed port, no port file the Web app should parse, and no default fallback. The
Web app learns the base URL from configuration, not from a guessed port.

### 1.2 Base URL resolution

`HttpSseBridge` resolves the base URL in this order:

1. The `daemon` query parameter on the page URL, for example
   `/app?daemon=http%3A%2F%2F127.0.0.1%3A<port>`.
2. The build-time `VITE_VOWP_DAEMON_URL` value.
3. Otherwise there is **no base URL**: the shell shows a setup panel that asks for the
   address. It never falls back to a port, a previous origin, or mock data.

This value is the base the **browser** calls, not necessarily the daemon's loopback
address. Behind a reverse proxy it is the proxy origin; in development it is the Vite
dev-server origin. The proxy or dev server holds the actual loopback daemon address. It is
always an absolute URL; there is no implicit empty base. A direct `http://127.0.0.1:<port>`
value is valid only when the Web app is served from that same origin; the daemon does not
serve the StackSpirit SPA, so in practice the value is the proxy or dev-server origin.

The resolved value MUST be an absolute `http` or `https` URL with:

- a host (`127.0.0.1`, `[::1]`, `localhost`, a DNS name, or a reverse-proxy host);
- an optional explicit port;
- an optional path prefix, with no trailing slash (for a proxy mounted under `/vowp`);
- **no** userinfo (`user:password@`), **no** query string, **no** fragment.

Any other value is `invalid_input` and the setup panel is shown. The bridge strips the
trailing slash and builds routes as `<base>/api/v1/...`.

When the `daemon` query parameter is used, the app validates it, stores it in
`sessionStorage`, and calls `history.replaceState` to remove it from the address bar. A
shared link therefore does not carry the daemon address. The address is a base URL, never a
home path, SSH key, or token.

### 1.3 Same-origin requirement

The daemon has no CORS layer and answers no `OPTIONS` preflight. The browser therefore
requires the page origin and the API origin to be the same. Two supported shapes:

| Shape | How it works | Status |
|---|---|---|
| **Reverse proxy** (production) | The proxy serves the built SPA and forwards `<base>/api/v1/*` to the loopback daemon on the same origin. | Required for a browser deployment; the proxy itself is out of scope for W1a. |
| **Vite dev proxy** | `apps/spirit-web/vite.config.ts` proxies `/api` to the server-side `VOWP_DAEMON_URL` (the actual loopback daemon). `VITE_VOWP_DAEMON_URL` is the dev-server origin, so the browser base and the page origin match. | Development and Playwright only. |

A deployment that serves the SPA from a different origin than the API does not work with the
current daemon. The bridge detects this at startup (base origin differs from
`window.location.origin`) and fails fast with `daemon_incompatible` and the same-origin copy
in §3.7, instead of showing a browser CORS error.

`vowp tunnel open window` forwards a loopback port to the daemon. That is a supported way
for a human to reach VOWP's own page, but it does **not** serve the StackSpirit SPA. A remote
StackSpirit Web deployment still needs one origin that serves the SPA and `/api/v1`.

### 1.4 What the Web app never does

- It never changes the daemon bind address, adds `0.0.0.0`, or asks the daemon to listen on
  a public interface.
- It never puts `VOWP_API_TOKEN`, a provider key, an SSH key, or a home path in a URL,
  `localStorage`, a log line, or an error message.
- It never reuses the daemon's own static pages at `/`, `/board`, `/jobs`, `/sessions`,
  `/cost`, `/inbox`, `/nodes`, or `/import`. Those are VOWP's embedded pages, not
  StackSpirit.

---

## 2. `SpiritBridge` route map

### 2.1 Availability classes

Every bridge method gets exactly one class:

| Class | Meaning in this document |
|---|---|
| **available** | A current daemon route can implement the method with honest data. |
| **unavailable** | The method exists in the bridge, but no daemon route exists. The Web implementation returns a typed unavailable result; the UI hides the surface or shows the exact reason. |
| **desktop-only** | The capability depends on the desktop process (local host profiles, SSH, keychain, engine supervision, gate probes). The Web implementation rejects with `desktop_only` and performs no network request. |

“Token-gated” is a property, not a fourth class: a token-gated write is still **available**
if its route exists, but it is hidden until the W1c write proxy is present.

### 2.2 `hosts`

| Method | Daemon route | Class | W1 behaviour |
|---|---|---|---|
| `hosts.list()` | no route | desktop-only | The daemon serves one home; it has no host-profile collection. `GET /api/v1/nodes` is not a `HostProfile` (no pinned host key, no identity file, no label). |
| `hosts.add()` | no route | desktop-only | Reject with `desktop_only`. |
| `hosts.update()` | no route | desktop-only | Reject with `desktop_only`. |
| `hosts.remove()` | no route | desktop-only | Reject with `desktop_only`. |
| `hosts.scanFingerprint()` | no route | desktop-only | SSH host-key scanning is a desktop action. |
| `hosts.trustFingerprint()` | no route | desktop-only | The app-private `known_hosts` entry lives on the desktop. |
| `hosts.probe()` | no route | desktop-only | SSH reachability, auth, host key, and remote `vowp --version` are desktop actions. |

The Web shell does not render the Hosts rail. If a `/hosts` URL is opened directly, it
renders the desktop-only panel with the exact copy in §3.7 and makes no bridge call.

### 2.3 `connection`

| Method | Daemon route | Class | W1 behaviour |
|---|---|---|---|
| `connection.connect(hostId)` | no route | desktop-only | There is no SSH connection to establish; the page already speaks HTTP to one daemon. Reject with `desktop_only`. |
| `connection.disconnect(hostId)` | no route | desktop-only | There is no daemon connect/disconnect control. Reject with `desktop_only`. |
| `connection.status()` | `GET /api/v1/health` (base) | available | Synthesized connection state: `connected` after a successful health read, `reconnecting` after a stream failure, `stale` after three failed polls, `failed` when the base is unreachable. `host_id` is the constant `"daemon"`; it is not a `HostProfile` id. `latency` is HTTP round-trip to the daemon, not SSH/VOWP latency. |
| `connection.subscribe(listener)` | `GET /api/v1/board/stream` + local liveness | available | Emits `connection`, `fleet`, and `error` events. Never emits `engine` or `gates`. Returns an unsubscribe that closes the `EventSource`, clears timers, and aborts in-flight requests. |

### 2.4 `fleet`

| Method | Daemon route(s) | Class | W1 behaviour |
|---|---|---|---|
| `fleet.get()` | `GET /api/v1/health`, `GET /api/v1/board`, `GET /api/v1/jobs`, `GET /api/v1/sessions`, `GET /api/v1/inbox?state=open` | available | Builds a partial `FleetView` and attaches the `unavailable` notices from §3. Missing fields are `null` + a notice, never zeroes. |
| `fleet.refresh()` | same reads as `get()` | available | Fetches one complete snapshot, applies it if it is the newest generation, and returns the retained view. |
| `fleet.capacity()` | no route | unavailable | Resolves `null` with `capacity_unavailable = { code: "no_route", route: "/api/v1/capacity", ... }`. No `CapacityView` is fabricated. |
| `fleet.job(id)` | `GET /api/v1/jobs/{id}`, `GET /api/v1/jobs/{id}/runs`, `GET /api/v1/jobs/{id}/gates` | available | Reads the full `Job` fields the drawer needs (no worktree or home path), its runs oldest first, and the gate results of its newest attempt. Returns `JobDetailView` with safe evidence summaries. A 404 on the job is typed `not_found` ("job not found"), a 404 on `/runs` or `/gates` is a missing route, a 500 is `remote`, and a network failure is `daemon_unreachable`. |
| `fleet.jobAction(jobId, action, reason?)` | `POST /api/v1/jobs/{id}/retry`, `POST /api/v1/jobs/{id}/park`, `POST /api/v1/jobs/{id}/cancel` | available (token-gated write) | Hidden in W1a. W1c proxies only these three. `retry` and `cancel` send no body; `park` sends `{"reason": "..."}`. A success is `202` and means **queued**, not applied. |
| `fleet.inbox(all)` | `GET /api/v1/inbox?state=open`, `?state=answered`, `?state=dismissed` | available | `all: false` fetches only `open`. `all: true` fetches the three states and merges them, sorted by `created_at` ascending then `id` ascending (the daemon's own order). `state=all` is a `400`; never send it. |
| `fleet.answerInbox(id, answer)` | `POST /api/v1/inbox/{id}/answer` | available (token-gated write) | Hidden in W1a. W1c only. Success is `202` with the answered `InboxItem` and `answered_by: "web"`. The supervisor applies the request on its next tick. |
| `fleet.setTarget(input)` | no route | unavailable | Rejects before any fetch with `no_route` (`/api/v1/capacity/target`). The target editor is hidden; no guessed target is shown. |
| `fleet.rawDocument()` | no route | unavailable | Resolves `null` with a notice. The raw-document drawer is hidden; the daemon does not serve the fleet-status or capacity-report documents. |

The Web bridge implements `fleet.job(id)` for the job drawer. The review/fix lane reads
the same method for jobs in `gating`, `reviewing`, and `fixing`, so a lane card carries its
real failed gate kind and safe summary instead of the old `GATE_FAILURES_UNAVAILABLE`
placeholder. If a job-detail route is absent, the lane shows the exact route-missing copy;
if a present route fails in transport, the notice code is `read_failed`. Neither case is
rendered as an empty “no failures” state.

### 2.5 `engine`

All engine methods are **desktop-only**. The daemon has no engine, model, provider-key, or
external-harness route. The browser never sends, receives, or stores a provider key.

| Method | Daemon route | Class |
|---|---|---|
| `engine.status()` | no route | desktop-only |
| `engine.models()` | no route | desktop-only |
| `engine.selectModel()` | no route | desktop-only |
| `engine.storeKey()` | no route | desktop-only |
| `engine.verifyKey()` | no route | desktop-only |
| `engine.deleteKey()` | no route | desktop-only |
| `engine.keyStatus()` | no route | desktop-only |
| `engine.scanExternal()` | no route | desktop-only |
| `engine.selectExternal()` | no route | desktop-only |
| `engine.useBundled()` | no route | desktop-only |
| `engine.restart()` | no route | desktop-only |

### 2.6 `gates`

| Method | Daemon route | Class | W1 behaviour |
|---|---|---|---|
| `gates.get()` | no route | desktop-only | `GateSnapshot` contains local engine and local-agent state that the daemon does not expose. Reject with `desktop_only`. |
| `gates.probe()` | no route | desktop-only | Gate probes run over SSH and against the local engine from the desktop. Reject with `desktop_only`. |

The daemon's ASIF export/import routes exist, but they do not satisfy `AsifGate` or
`LocalOrchestratorGate`: those gates also cover the local engine, local-agent protocol, and
feature flags that the daemon does not report. A working ASIF route is not permission to
claim a Ready gate.

### 2.7 Routes with no bridge method

These routes exist today but have no `SpiritBridge` method. W1a does not call them.

The three job-detail routes (`GET /api/v1/jobs/{id}`, `/runs`, and `/gates`) are no longer
in this table: they now back `fleet.job(id)` in §2.4.

| Route | Auth | Why it is not mapped | W1 |
|---|---|---|---|
| `GET /api/v1/sessions/{id}` | none | The bridge has no session-detail method; it returns `SessionDetail` (session, latest capsule, lineage). | Not used. A later session drawer adds `fleet.session(id)`. |
| `GET /api/v1/sessions/{id}/capsules/{capsuleId}` | none | No capsule method. | Not used. |
| `GET /api/v1/sessions/{id}/events?after_seq=&limit=` | none | No event-timeline method. | Not used. |
| `GET /api/v1/sessions/{id}/events/stream?after_seq=` | none | No event-stream method. | Not used. |
| `GET /api/v1/sessions/{id}/export/asif?report=` | none | ASIF actions are desktop D5 and gated; the bridge has no ASIF method. | Not used. |
| `POST /api/v1/sessions/import/asif?branch=` | Bearer token | Token-gated, desktop D5, gated on `v4-asif-*`. | Not used. |
| `POST /api/v1/sessions/{id}/switch` | Bearer token | No executor-switch method; local-node only. | Not used. |
| `GET /api/v1/cost?by=&since=&until=` | none | No cost method. `FleetView` carries only `Board.cost_today_usd`. | Not used. A later `fleet.cost(by)` can map it. |
| `GET /api/v1/nodes` | none | `Node` is not a `HostProfile`; it carries `ssh_target` and no pinned key. Exposing it in W1 adds host data for no user value. | Not used. |

The daemon also serves static pages and PWA assets (`/`, `/board`, `/jobs`, `/sessions`,
`/cost`, `/inbox`, `/nodes`, `/import`, `/manifest.webmanifest`, `/sw.js`, `/icon.svg`).
They are VOWP's own window and are not part of the StackSpirit Web contract.

W1a does not need the job-detail reads because `GET /api/v1/jobs` returns every job. They
now power the read-only job drawer and the review/fix lane (§6.2, Ask 3).

### 2.8 Token-gated writes

Every route in this table is a token-gated write; every other route named in §2 is a
token-free read. The daemon reads `VOWP_API_TOKEN` at startup. A missing or empty value
leaves every `GET` working and disables every write with `503`. A missing or wrong bearer
value is `401` with `WWW-Authenticate: Bearer`, checked before the id or body is read.

| Route | Body | Success | Documented failures | W1 |
|---|---|---|---|---|
| `POST /api/v1/jobs/{id}/retry` | none | `202 JobActionResponse` | `400` invalid id, `401`, `404`, `409` state guard, `500`, `503` | W1c only |
| `POST /api/v1/jobs/{id}/park` | `{"reason": "<text>"}` optional | `202 JobActionResponse` | as retry; an empty/blank reason uses `"parked from the web"`; over 10,000 bytes is `400` | W1c only |
| `POST /api/v1/jobs/{id}/cancel` | none | `202 JobActionResponse` | as retry | W1c only |
| `POST /api/v1/inbox/{id}/answer` | `{"answer": "<text>"}` | `202 InboxItem` | `400` empty/oversized/malformed, `401`, `404`, `409` not open, `500`, `503` | W1c only |
| `POST /api/v1/sessions/{id}/switch` | `{"to", "reason", "force"}` | `200 Switch` | `400`, `401`, `404`, `409`, `500`, `503` | Not in W1 |
| `POST /api/v1/sessions/import/asif` | raw zip | `201 AsifImportResponse` | `400`, `401`, `409`, `413`, `415`, `500`, `503` | Not in W1 |

`JobActionResponse` is `{ job, action, request }`, where `request` is the queued request id
(`req:<project>:<ULID>`). It never means the job already moved.

Exact state guards the UI must mirror:

| Action | Legal current states | Queued request |
|---|---|---|
| retry | `failed`, `stalled`, `parked` only | `failed` → `retry_job`; `stalled` → `retry_job` via `KeeperRestart`; `parked` → `resolve_job` |
| park | any non-terminal state | `park_job` with the trimmed reason |
| cancel | any non-terminal state | `cancel_job` |

Every other state returns `409` and queues nothing. For an inbox answer, a job-bound item
queues one `answer_inbox` request that the supervisor applies on its next tick; a node-inbox
item is answered in place with no job request. The browser never claims the job moved.

### 2.9 Method classification summary

| Group | available | unavailable | desktop-only | Total |
|---|---|---|---|---|
| `hosts` | 0 | 0 | 7 | 7 |
| `connection` | 2 | 0 | 2 | 4 |
| `fleet` | 6 | 3 | 0 | 9 |
| `engine` | 0 | 0 | 11 | 11 |
| `gates` | 0 | 0 | 2 | 2 |
| **Total** | **8** | **3** | **22** | **33** |

`fleet.jobAction` and `fleet.answerInbox` are counted as available because the routes exist;
they are hidden in W1a and enabled only in W1c.

---

## 3. Data honesty

### 3.1 Wire sources used by W1a

| Source | Document | Fields W1a uses |
|---|---|---|
| `GET /api/v1/health` | `Health` | `version`, `node`, `project`, `schema_version`, `heartbeat` |
| `GET /api/v1/board` | `Board` | `at`, `project`, `jobs`, `running`, `slots_busy`, `slots_total`, `planner`, `keeper`, `limits`, `cost_today_usd`, `inbox_open`, `last_checkpoint` |
| `GET /api/v1/jobs` | `JobPage` (`Page<Job>`) | every `Job` row; W1a uses `id`, `project`, `title`, `kind`, `role`, `state`, `priority`, `parked_reason`, `state_changed_at`, `updated_at` |
| `GET /api/v1/sessions` | `SessionPage` (`Page<Session>`) | `id`, `job_id`, `node`, `state`, `executor`, `updated_at`; used only to join a running job to a session |
| `GET /api/v1/inbox?state=open` | `InboxPage` (`Page<InboxItem>`) | the full `InboxItem` fields |

`JobPage.total` is the number of rows in this response, not a server-side total. The list is
unpaginated in this slice and `next` is `null`; if VOWP adds a cap, the bridge must label the
truncation rather than present the page as complete.

### 3.2 `FleetView` field map

`FleetView` is defined in `packages/spirit-ui/src/bridge/wire.ts`. The Web bridge fills it as
follows. “Unavailable” means `null` plus a visible notice, or a documented constant for a
non-nullable field; it never means `0`, `false`, `"active"`, or an empty array presented as
“none”.

| `FleetView` field | Source | W1 value and honesty rule |
|---|---|---|
| `revision` | local counter | Monotonic per applied snapshot. Starts at `0`; increments once per applied board frame or poll result. Never derived from the server; `Board` has no sequence. |
| `host_id` | none | Constant `"daemon"`. It is an opaque client key for the one daemon, not a `HostProfile` id. The Hosts rail is not rendered. |
| `fetched_at` | client clock | The moment the complete snapshot was received. RFC 3339 UTC with milliseconds. |
| `generated_at` | `Board.at` | The time the **changed** board snapshot was collected. It is not a freshness signal: the daemon resends a frame only when the board changes apart from `at`, so a stable board keeps an old `at`. Label it “snapshot changed at”, never “data as of”. |
| `source` | none | The new documented value `"daemon"`, meaning “computed live by the daemon from the project store”. Never claim `local`, `remote`, or `cache`; the daemon exposes no `FleetStatus.source`. |
| `stale` | local liveness | `true` only after three consecutive failed polls (or a `stale` connection event). It is not VOWP's `stale` flag; label it “no data from the daemon”. |
| `home` | `Health.node` | Exact node name. Exactly one. |
| `supervisor` | none | `null`. `Health.heartbeat` is only the last heartbeat timestamp; it does not tell us `running`, `pid`, `paused`, `last_tick_at`, `last_tick_age_secs`, `tick_secs`, `fail_streak`, or the supervisor version. `Health.version` is the daemon version and must not be copied into `SupervisorView.version`. Show “supervisor state unavailable” plus the heartbeat age as a separate fact. |
| `projects` | `Health.project`, `Board` | One entry. `name` = `Health.project` (must equal `Board.project`; otherwise `daemon_incompatible`). `state` = `"unknown"`; `paused_reason` = `""` only because the field is non-nullable — consumers must gate on `state == "unknown"` and must not render the empty reason as “active”. `jobs_by_state` = `Board.jobs` (exact). `cost_today_usd` = `Board.cost_today_usd` (`null` stays unknown). |
| `capacity` | none | `null`. See §3.3. |
| `running` | `Board.running` + `Job` + `Session` | One card per board run, joined to the matching `Job` by id and to the `Session` with that `job_id` and the newest `updated_at`. `kind`, `role`, and `project` come from the job; `state` is `"stalled"` when the job is stalled and `"active"` otherwise, with `stalled` mirroring that state, exactly as the desktop card; `executor`, `age_secs`, `last_event_age_secs` from the board run; `session_id` from the joined session; `node` from the joined session or `null`. `WorkerCardView` has no title; the shell may show the title from the matching lane card as the desktop does, or the job id, never an invented title. A run with no matching job keeps its board fields and says “No matching job row for this board run.” rather than disappearing. |
| `lanes.counts` | `Board.jobs` | Exact. Map states to lanes with the same table as `crates/spirit-core/src/view/lanes.rs`: running/stalled/merging → `running`; pending/ready → `ready`; gating/reviewing/fixing → `review_fix`; parked/failed/cancelled/unknown → `parked_blocked`; done/replanned → `done`. Missing key = 0. |
| `lanes.running` | `Board.running` + `Job` | Cards built like `FleetView.running`: `title`, `priority` from the job; `executor`, `age_secs` from the board run; `session_id` from the joined session; `reason`, `detail`, and `gate*` are `null`. No fabricated title, gate, or waiting reason. |
| `lanes.ready` | `GET /api/v1/jobs` | Jobs in `pending`/`ready`. `title`, `kind`, `role`, `priority`, `state` come from the job; `age_secs` is client `now - state_changed_at`; `reason`, `detail`, `gate*`, `executor`, and `session_id` are `null`. |
| `lanes.review_fix` | `GET /api/v1/jobs` + `fleet.job(id)` | Cards for jobs in `gating`/`reviewing`/`fixing`. `title`, `kind`, `role`, `priority`, and `state` come from the job. The first failed gate of the newest attempt supplies `gate`, `gate_at`, and `gate_evidence` (a safe summary); a passed-only attempt says so, and an empty gate page says “No gate results recorded for the newest attempt”. Missing route detail shows `GATE_FAILURES_UNAVAILABLE`; a transport failure shows a `read_failed` notice. Never an empty lane that implies no failures. |
| `lanes.parked_blocked` | `GET /api/v1/jobs` | Jobs in `parked`/`failed`/`cancelled` and unknown states. `title`, `kind`, `role`, `priority`, `state` come from the job; `age_secs` is client `now - state_changed_at`; `reason` = `parked_reason` when present, otherwise the state string; `detail`, `gate*`, `executor`, and `session_id` are `null`. Do not copy `Job.note`, which is not the redacted `BlockedJobCard.detail`. |
| `lanes.done` | `Board.jobs` | Count only, exactly as the desktop does. There is no `last_merge` in `Board`; do not invent a merge card. |
| `lanes.queue_truncated` | none | `false`. The current list is unpaginated. If a future daemon caps it, the bridge must set this from the response, not assume. |
| `lanes.queue_shown` | `GET /api/v1/jobs` | Number of job rows that feed the ready/review/parked lanes: every state except `running`, `stalled`, `merging`, `done`, and `replanned`. These are the rows the list lanes consume. |
| `lanes.queue_cap` | none | `null` after the contract change in §3.6; the snapshot's 200-row cap does not exist on this route. |
| `inbox` | `GET /api/v1/inbox?state=open` | Full `InboxItem` rows, including `body`, `options`, `answer`, `answered_by`, and `answered_at`. `age_secs` is derived from `created_at` and labelled as an age. The badge uses `Board.inbox_open`; if it differs from the loaded list, show both (“N open, M loaded”) rather than picking one. |
| `sessions` | `GET /api/v1/sessions` partially | `[]` plus the `active_surfaces` notice. `Session` has no `active_surface` and no `last_event_at`; setting `active_surface: null` would claim “no lease”, which is false. The W1 shell does not render a sessions drawer. |
| `orchestrators` | no route | `[]` plus the `orchestrators` notice. Never render an empty state that implies “no orchestrators”. |
| `latency` | local HTTP timings | HTTP round-trip to the daemon for the board reads (last 20 samples). Label “daemon RTT”, never “SSH latency” or “VOWP latency”. |

### 3.3 `CapacityView` field map

There is **no** capacity or storage read route in `vowp-daemon`. `Board` contains only job
counts, worker-slot counts, limits, cost, inbox count, and checkpoint time. It does not
contain a host sample, a target, a brake, or estimates. Therefore W1a does not construct a
`CapacityView` at all: every row below is “not constructed”, not a zeroed field. The “W1
value” column records the honest state to display if a future route is absent.

| `CapacityView` field | Source in current daemon | W1 value |
|---|---|---|
| `generated_at` | none | unavailable |
| `measured_at` | none | unavailable |
| `report_loaded` | none | not constructed (there is no report) |
| `host` | none | `null`; never a zeroed `HostSampleView` |
| `running.runs` | none | unavailable. `Board.slots_busy` counts busy worker slots, not open runs; do not map it. |
| `running.ram_p90_bytes` | none | unavailable |
| `running.cpu_p90_cores` | none | unavailable |
| `target.mode` | none | unavailable |
| `target.target_workers` | none | unavailable. `Board.slots_total` is `max_workers + max_verifiers`, the configured slot count, not the target. |
| `target.advised_workers` | none | unavailable |
| `target.is_override` | none | unavailable |
| `target.set_by` | none | unavailable |
| `target.set_at` | none | unavailable |
| `target.reason` | none | unavailable |
| `advised_workers` | none | unavailable; never guessed from `slots_total` |
| `brake.active` / `brake.reasons` / `brake.since` | none | unavailable |
| `reserve_ram_bytes` | none | unavailable |
| `min_disk_bytes` | none | unavailable |
| `disk.free_home_bytes` | none | unavailable |
| `disk.free_worktrees_bytes` | none | unavailable |
| `disk.floor_bytes` | none | unavailable |
| `disk.source` / `disk.state` | none | unavailable |
| `ram.available_bytes` | none | unavailable |
| `ram.reserve_bytes` | none | unavailable |
| `ram.running_p90_bytes` | none | unavailable |
| `ram.per_worker_p90_bytes` | none | unavailable |
| `ram.per_worker_source` | none | unavailable |
| `ram.per_worker_samples` | none | unavailable |
| `ram.estimated_extra_slots` | none | unavailable |
| `ram.state` | none | unavailable |
| `cpu.load1` / `cpu.cpu_count` / `cpu.load_ratio` | none | unavailable |
| `cpu.pressure_avg10` | none | unavailable |
| `cpu.state` | none | unavailable |
| `estimates` | none | unavailable. An empty list would mean “no estimates”, which is false. |
| `waiting` | none | unavailable. `Job.parked_reason` is not a `WaitCode` or a capacity reason. |
| `limits` | `Board.limits` (partial) | `Board.limits` has `provider`, `kind`, `resets_at` only. `LimitView` also needs `detail`, `since`, `last_probe_at`, `probe_fail_streak`, `concurrency_cap`, `cap_until`. Render a separate read-only “Active limits” strip from `Board.limits`; do not put it in a `CapacityView`. |
| `provider_caps` | none | unavailable |
| `reload_pending` | none | unavailable; do not show `false` as “no reload pending” |

The capacity rail in W1a is only the `CAPACITY_DETAIL_UNAVAILABLE` notice; it contains no
gauges, no target editor, no estimates, and no waiting list. A separate “Board facts” strip
may show the real board values that do exist: `slots_busy / slots_total` (labelled
“configured slots”, not “target”), active limits from `Board.limits`, `cost_today_usd`
(unknown stays unknown), `inbox_open`, and `last_checkpoint`. That strip is not a
`CapacityView` and must not be styled as one.

### 3.4 Derived values and their labels

| Derived value | Rule | Required label |
|---|---|---|
| `FleetView.revision` | local apply counter | none (internal) |
| `FleetView.fetched_at` | client clock at snapshot receipt | “received” |
| `FleetView.source` | the constant `"daemon"` | “live from daemon” |
| `FleetView.stale` | three failed polls or a `stale` connection event | “no data from the daemon” |
| `FleetView.latency` | HTTP round-trip to the daemon | “daemon RTT” |
| `JobCardView.age_secs` | client `now - state_changed_at` | “in this state” |
| `InboxItemView.age_secs` | client `now - created_at` | “age” |
| `WorkerCardView.session_id` | `Session` with the same `job_id` and newest `updated_at` | “matched by job id” |
| `WorkerCardView.node` | joined session's `node` | “from session”; `null` otherwise |
| `CapacityView` | not derived in W1a | “Capacity detail unavailable” |

### 3.5 Forbidden derivations

These mappings must not appear in a Web build, a test fixture, or a snapshot. Each one
produces plausible-looking data that is not true.

| Forbidden mapping | Why it is wrong |
|---|---|
| `Board.slots_total` → `target.target_workers` or `advised_workers` | Slots are the configured maximum (`max_workers + max_verifiers`); the target is a separate advisory value. |
| `Board.slots_busy` → `CapacityView.running.runs` | Busy worker slots are not open runs. |
| `Board.jobs["running"]` → `CapacityView.running.runs` | Job count is not run count. |
| `Board.cost_today_usd == null` → `0` | `null` means unknown. Unknown is never free. |
| `Health.heartbeat == null` → `supervisor.running == false` | A missing/unreadable heartbeat is not proof the supervisor stopped. |
| `Health.version` → `SupervisorView.version` | It is the daemon binary version, not the supervisor version. |
| `Job.parked_reason` → a capacity `WaitCode` or a brake reason | Parked reasons (`credits`, `auth`, `question`, ...) are not capacity waits. |
| `Job.note` → `JobCardView.detail` | The desktop's `detail` is a redacted, 200-character card field; `note` has no such guarantee. |
| `Session.updated_at` → `SessionCardView.last_event_at` | A row update is not the last event time. |
| `Session.last_seq` → `active_surface` | Sequence numbers say nothing about leases. |
| `Board.at` → freshness or “stale” | A stable board is not resent, so `at` can be old while the link is healthy. |
| `JobPage.total` → a server-side total | It is the number of rows in this response. |
| `GET /api/v1/nodes` → `HostProfile` | `Node` has no pinned host key, identity file, or label. |
| A `202` write response → “job done” / “inbox resolved” | The supervisor applies the queued request on a later tick. |
| An empty array → “none exist” without a capability notice | The route may simply not exist. |

### 3.6 Required bridge contract changes

These are the exact changes the W1 job needs in `packages/spirit-ui/src/bridge/wire.ts` and
`SpiritBridge.ts`. They are listed here, not implemented here.

1. `SpiritBridge.kind` becomes `"tauri" | "mock" | "http-sse"`.
2. Add a static `capabilities` map so the shell never branches on `kind`:

   ```ts
   export interface BridgeCapabilities {
     capacity_detail: boolean;
     supervisor_state: boolean;
     host_management: boolean;
     engine_management: boolean;
     capability_gates: boolean;
     job_actions: boolean;      // route exists and the write proxy is present
     inbox_answer: boolean;     // route exists and the write proxy is present
     set_target: boolean;
     raw_documents: boolean;
     session_detail: boolean;
   }
   ```

   Desktop: `true` for every capability it already implements; `session_detail` stays
   `false` until a session drawer exists. Web W1a: all `false`. Preview/mock: per scenario,
   and never `true` for a capability the scenario cannot honestly supply.

3. `FleetView.capacity` becomes `CapacityView | null`, and add:

   ```ts
   export interface CapacityUnavailable {
     code: "no_route" | "not_loaded";
     route: string | null;
     detail: string;
   }
   ```

   Add `capacity_unavailable: CapacityUnavailable | null` to `FleetView`.

4. `FleetView.supervisor` becomes `SupervisorView | null`, with
   `supervisor_unavailable: { code: "no_route"; detail: string } | null`.

5. `WorkerCardView.node` becomes `string | null`.

6. `JobLanes.queue_cap` becomes `number | null`.

7. `FleetView.source` accepts `"daemon"`.

8. Add `FleetView.unavailable: UnavailableNotice[]`, where an item names a feature
   (`capacity_report`, `supervisor_state`, `active_surfaces`, `gate_failures`,
   `orchestrators`, `project_state`, `raw_documents`) and a reason string. The shell must not
   render an empty state for a feature in this list.

9. Add `SpiritErrorCode` values `daemon_unreachable`, `daemon_incompatible`,
   `writes_disabled`, and `desktop_only`, with the fixed copy in §4.6, plus matching
   `errorTitle` and `isKnownCode` entries in
   `packages/spirit-ui/src/bridge/errors.ts`.

10. Add an optional `daemon` read for the header:

    ```ts
    export interface DaemonHealth {
      origin: string;          // scheme + host + optional port, no path, no query
      version: string;         // daemon binary version
      node: string;
      project: string;
      schema_version: number;  // store schema, not the snapshot schema
      heartbeat_at: Timestamp | null;
      received_at: Timestamp;
    }
    ```

    `SpiritBridge.daemon?: { health(): Promise<DaemonHealth> }` is present on the Web and
    mock bridges; the desktop bridge may omit it because it already has the richer
    `FleetView`.

The store changes are correspondingly small: `capacityUnavailable` and
`supervisorUnavailable` are copied in `acceptFleet`, and the Capacity rail checks
`capacityUnavailable` before the generic “connect a host” empty state. Every consumer that
currently reads `fleet.supervisor.*` (TopBar, StatusStrip, VpsRootCard, FleetCanvas) must
handle the `null` branch before the Web shell mounts; a Web build must not crash or show
“supervisor stopped” when the state is simply unavailable.

### 3.7 Exact copy

These strings are part of the contract; tests assert them. The capacity string deliberately
does **not** say “retrying”: retrying cannot create a missing route.

| Constant | Copy |
|---|---|
| `CAPACITY_DETAIL_UNAVAILABLE` | `Capacity detail unavailable — this daemon has no capacity/storage read route (GET /api/v1/capacity).` |
| `CAPACITY_DETAIL_EXPLAIN` | `The board reports job counts, configured worker slots, active limits, cost, and the inbox count. It does not report disk, RAM, CPU pressure, the worker target, or the brake state, so StackSpirit shows no values instead of zeroes or a guessed target.` |
| `SUPERVISOR_STATE_UNAVAILABLE` | `Supervisor state unavailable — this daemon exposes only the last heartbeat, not running, pid, paused, or fail-streak state.` |
| `PROJECT_STATE_UNAVAILABLE` | `Project state unavailable — the daemon does not report active, paused, or archived.` |
| `HOSTS_DESKTOP_ONLY` | `Host profiles and SSH keys live in the desktop app. This Web build talks to one daemon and cannot add, connect, or switch hosts.` |
| `ENGINE_DESKTOP_ONLY` | `The StackSpirit Engine runs beside the desktop app. This Web build cannot start the engine, manage provider keys, or switch models.` |
| `GATES_DESKTOP_ONLY` | `Capability gates are probed over SSH from the desktop app. This Web build cannot probe them.` |
| `READ_ONLY_NOTICE` | `Read-only Web build. Job and inbox actions require the local write proxy.` |
| `GATE_FAILURES_UNAVAILABLE` | `Gate failure detail is unavailable — this daemon does not serve the job-detail routes (GET /api/v1/jobs/{id}, /runs, and /gates).` |
| `GATE_FAILURES_READ_FAILED` | `Gate failure detail could not be read — the job-detail routes are present, but a read failed. The cards below still come from GET /api/v1/jobs.` |
| `JOB_DETAIL_UNAVAILABLE` | `Job detail is unavailable — this daemon does not serve GET /api/v1/jobs/{id} with /runs and /gates.` |
| `JOB_DETAIL_NOT_FOUND` | `Job not found — the daemon has no job with this id. It may have been removed or replanned.` |
| `JOB_DETAIL_STALE` | `The board snapshot is stale. This job detail was read directly from the daemon when the drawer opened.` |
| `JOB_DETAIL_LOADING` | `Reading the job, runs, and gates from the daemon…` |
| `JOB_RUNS_EMPTY` | `No runs recorded — the daemon returned an empty run list for this job.` |
| `JOB_GATES_EMPTY` | `No gate results recorded — the daemon returned an empty list for the newest attempt.` |
| `JOB_RUNS_TRUNCATED` | `The daemon paginated this job's run list; only the returned page is shown.` |
| `JOB_GATES_TRUNCATED` | `The daemon paginated this job's gate results; only the returned page is shown.` |
| `SESSION_SURFACES_UNAVAILABLE` | `Session surfaces are unavailable — the daemon does not expose active surface leases or last-event times.` |
| `ORCHESTRATORS_UNAVAILABLE` | `Orchestrators are unavailable — the daemon has no orchestrator read route.` |
| `RAW_DOCUMENT_UNAVAILABLE` | `Raw VOWP documents are unavailable — the daemon serves board and list documents, not the fleet-status or capacity-report documents.` |
| `SAME_ORIGIN_REQUIRED` | `This daemon has no CORS support. Serve this app and /api/v1 from the same origin through the reverse proxy.` |

---

## 4. `HttpSseBridge` behaviour

### 4.1 Startup

1. Resolve and validate the base URL (§1.2). With no base URL, show the setup panel; do not
   construct a bridge and do not load mock data.
2. Check the same-origin rule (§1.3). On a mismatch, fail with `daemon_incompatible` and the
   `SAME_ORIGIN_REQUIRED` copy.
3. `GET /api/v1/health` with a 10 s timeout. A success sets the connection to `connected`,
   records `since`, and fills `DaemonHealth`.
4. Probe the optional W1b routes **once per daemon session**, not in a loop:
   `GET /api/v1/capacity` and `GET /api/v1/fleet`. A `200` enables the matching capability
   and uses the document. A `404` sets the capability false and the `no_route` reason. A
   network error sets `not_loaded` and retries only on the next explicit refresh. The probe
   must not log response bodies.
5. Fetch the first complete snapshot with `Promise.allSettled` over board, jobs, sessions,
   and inbox (`state=open`). If the board succeeds, build and emit the `FleetView` even if a
   secondary read failed; attach a typed error for the failed read. If the board fails,
   show the error and keep the previous view if there is one.
6. Open the board stream only after the first board read succeeds.
7. Return the unsubscribe from `subscribe`; it closes the `EventSource`, clears all timers,
   and aborts in-flight `fetch` requests.

### 4.2 Board stream

`GET /api/v1/board/stream` is a `text/event-stream` response:

| Event | Data | Meaning | Bridge action |
|---|---|---|---|
| `board` | the exact `Board` JSON | A complete snapshot. The first poll always sends one; later frames arrive only when the document changes apart from its wall-clock `at`. | Parse, validate, retain as the new board, and bump the applied generation. Do not emit a `FleetView` that mixes this board with older job/session/inbox rows; emit after the secondary reads for this generation settle or fail. Never merge arrays. |
| `heartbeat` | `{"at": "<RFC 3339>"}` | Liveness only; sent every 15 s, including while the board is stable. | Update the last-heartbeat time. Do **not** bump the revision or mutate data. |
| anything else | — | The board stream has no `resync` and no cursor. | Ignore unknown named events; ignore the default `message` event. |

There is no `id:`, no `retry:`, no `Last-Event-ID`, no `after_seq`, no resume, and no merge.
A `board` frame is self-contained. A parse failure keeps the previous view and emits a
`parse` error; it never produces a partial `FleetView`.

If two consecutive `board` frames are byte-identical, the bridge may ignore the second; the
daemon normally does not send it.

### 4.3 Reconnect, backoff, and polling fallback

Use a controlled reconnect, not the browser's default silent retry, so the UI can show the
state:

| Step | Behaviour |
|---|---|
| Stream opens | Connection `connected`; reset the retry delay to 1 s; stop the polling timer. |
| Stream error/close | Close the source; connection `reconnecting`; keep the last good view; start the 5 s poll timer immediately; schedule one reconnect. |
| Retry delay | 1 s, doubling to a cap of 15 s. Reset to 1 s on `open` and on a valid `board` frame. |
| Heartbeat timeout | If no `board` or `heartbeat` arrives for 45 s (three 15 s heartbeats), treat it as a stream failure. |
| Poll success | Connection `connected`; update the view; a success after a stale period clears `stale`. |
| Three poll failures | Connection `stale`; keep the last good view with `stale: true`; move polling to the 30 s backoff already used by `fleetStore` (`STALE_BACKOFF_MS`); show the retry countdown. |
| Stream success after polling | Stop polling; connection `connected`; clear `stale`. |
| `EventSource` unavailable | Never poll faster than 5 s; show the polling-only notice; connection stays `connected` while reads succeed. |
| Unsubscribe | Close the source, clear both timers, abort in-flight requests. |

The 5 s poll and 15 s heartbeat mirror the daemon's own board page. The 30 s stale backoff
mirrors `fleetStore`. The Web stale notice must use `FleetView.fetched_at` and the local
heartbeat time, never `generated_at` (the changed-snapshot time) or
`supervisor.last_tick_age_secs` (unavailable). It says “no data from the daemon”, not “VOWP
is stale”.

### 4.4 Revision monotonicity and older snapshots

`Board` has no sequence number, revision, or ETag. The bridge therefore orders by **request
generation**, not by document content:

1. Keep `requestSeq` and `appliedSeq`.
2. Every `fleet.get()`/`fleet.refresh()` increments `requestSeq` and captures its value.
3. Every SSE `board` frame also counts as a newer generation.
4. When a response arrives:
   - if its captured `seq < appliedSeq`, drop it entirely; a newer snapshot already won;
   - otherwise set `appliedSeq = seq`; increment `revision` and emit only when that
     generation's inputs have settled (§4.5).
5. `fleet.refresh()` returns the retained view if a newer SSE frame won the race; it does not
   overwrite newer state with its older response.
6. `fleetStore.applyEvent` still drops `event.revision < state.revision`. Equal revisions
   are allowed because a connection event and its fleet event share one revision.

Consequence: “newer” means “produced by a later request or frame”. The bridge must not
compare two `Board` documents field by field to decide which is newer, because there is no
authoritative version. This is a documented limitation, not a reason to invent one.

### 4.5 Refresh semantics

| Call | Requests | Emission |
|---|---|---|
| `fleet.get()` | health (if not fresh), board, jobs, sessions, inbox open | One `fleet` event; one `connection` event if the state changed. |
| `fleet.refresh()` | board always; jobs, sessions, inbox open always; health best-effort | One `fleet` event. |
| SSE `board` | board frame | Retain the board and bump the generation. If the board changed, coalesce one refetch of jobs/sessions/inbox and emit only after it settles; if a newer frame arrives while that refetch is in flight, mark dirty and refetch once after it settles. |
| Poll tick | board only | If the board changed (ignoring `at`), refetch jobs/sessions/inbox before emitting. If unchanged, update liveness only. |
| `fleet.inbox(true)` | three inbox states | No `fleet` event; the drawer owns the result. |

The board is the change signal. Polling five endpoints every 5 s when nothing changes is
wasteful and increases the chance of mixed snapshots.

**Snapshot coherence.** A `FleetView` must not combine a new `Board` with a previous
generation's job, session, or inbox rows. `fetched_at` is the time the emitted view's inputs
settled, and `generated_at` remains `Board.at`. If a secondary read fails, emit the
board-derived view with the last good rows, attach the typed error for that read, and label
the secondary data as last-known; never present the mix as one atomic snapshot.

### 4.6 Typed error mapping

`ApiError` is `{"error":{"kind","message","hint"}}`; `hint` may be `null`. The bridge maps
the transport condition and, where present, `error.kind` to `SpiritErrorCode`. It shows the
daemon's path-free `message`, never raw HTML, a stack trace, or a response body.

| Condition | Code | Retryable | UI copy / action |
|---|---|---|---|
| No base URL | `invalid_input` | false | Setup panel; no request. |
| Invalid base URL (userinfo, bad scheme, query, fragment) | `invalid_input` | false | Setup panel with the field error. |
| Base origin differs from page origin | `daemon_incompatible` | false | `SAME_ORIGIN_REQUIRED`. |
| `fetch` rejects, connection refused, DNS failure | `daemon_unreachable` | true | `The daemon at <origin> did not answer.` |
| 10 s read timeout / `AbortError` | `remote_timeout` | true | `The daemon did not answer in time.` |
| `400` + `invalid_input` | `invalid_input` | false | Show the daemon message. |
| `401` + `auth` | `auth` | false | Clear the proxy session, hide actions, show `The write proxy rejected this session.` Never prompt for `VOWP_API_TOKEN` in the browser. |
| `404` on a fixed **required** read route | `daemon_incompatible` | false | Name the missing route; disable that feature; keep other views. The optional `/api/v1/capacity` and `/api/v1/fleet` probes treat `404` as `no_route`, not as an error. |
| `404` on a write for a specific id | `not_found` | false | `That item no longer exists.` Refresh. |
| `409` + `conflict` | `conflict` | false | `VOWP changed this item; refreshing.` Refresh and show the new state. |
| `413` | `invalid_input` | false | `The item is too large for this daemon.` |
| `415` | `invalid_input` | false | Show the daemon message. |
| `503` + `config` | `writes_disabled` | false | `Writes are disabled on this daemon. The local write proxy is not configured.` |
| `500` + any kind | `remote` | true (one retry) | Show the daemon's path-free message; retry once, then stop. |
| `3xx` / opaque redirect | `daemon_incompatible` | false | API requests use `redirect: "manual"`; a redirect is not followed and never becomes a silent login page. |
| `2xx` with non-JSON or a shape mismatch | `parse` | false | `The daemon returned a document StackSpirit could not read.` Keep the last view. |
| Desktop-only method | `desktop_only` | false | The exact desktop-only copy; no request. |
| `fleet.capacity()` | not an error | — | Returns `null` + `capacity_unavailable`; the UI shows the capacity copy. |

All reads use `redirect: "manual"`, `credentials: "omit"`, and an `AbortController` timeout
(10 s reads, 15 s writes). W1c writes use `credentials: "same-origin"` for the proxy session
cookie and send the `X-Spirit-CSRF` header. Error text is truncated to 500 characters,
control characters are stripped, and only the origin plus path is ever included — never the
query string, headers, or a home path.

### 4.7 `MockBridge` and preview mode

The Web app uses the same factory pattern as the desktop app:

```ts
if (import.meta.env.VITE_SPIRIT_MOCK === "1") {
  return new MockBridge(scenario);
}
return HttpSseBridge.fromEnvironment();
```

- `VITE_SPIRIT_MOCK_SCENARIO` selects a named scenario; an unknown name falls back to
  `fresh`, never to a live daemon.
- `MockBridge.preview` is `true`; the shell always shows the persistent preview badge.
- There is **no silent fallback** from a failed live read to mock data. A live failure shows
  the typed error and the last view; it never says “preview”.
- For Web tests, `MockBridge` gains options for `capabilities` and
  `capacityUnavailable`, and the named scenarios `web-read-only`, `web-stale`, and
  `web-recovering`. `web-read-only` sets `capacity_detail: false`,
  `supervisor_state: false`, `host_management: false`, `engine_management: false`,
  `capability_gates: false`, `job_actions: false`, `inbox_answer: false`,
  `set_target: false`, `raw_documents: false`, `session_detail: false`, and a `no_route`
  capacity reason. `web-stale` starts with three failed polls; `web-recovering` starts with
  the stream down and polling healthy. Existing scenarios keep their desktop behaviour.
- The preview badge and the “Preview data” copy are not a substitute for honest data: a mock
  scenario that claims capacity must supply a complete `CapacityView`, not zeroes.

---

## 5. Security

### 5.1 Browser boundary

- The browser never holds `VOWP_API_TOKEN`. W1a has no token input and sends no
  `Authorization` header.
- The browser never holds a provider API key. The `engine` group is desktop-only; no key
  material is fetched, rendered, or stored.
- The browser never holds an SSH key, a `known_hosts` entry, or a `HostProfile`.
- Reads are token-free. For any deployment that is not loopback-only, **the reverse proxy
  must authenticate users itself**. The daemon has no user accounts; the write token is not
  an access-control system.
- The write proxy (W1c) holds the daemon token server-side. The browser gets only an
  HttpOnly, `SameSite=Strict`, `Secure` session cookie scoped to the proxy origin, plus a
  CSRF header. No token value is ever in JavaScript, `sessionStorage`, `localStorage`, a
  URL, or the DOM.

### 5.2 Reverse proxy requirements

| Requirement | Value |
|---|---|
| Listener | Loopback or the private network only; never the daemon directly. |
| TLS | Terminate TLS for any non-loopback deployment; an `https` page must not call an `http` API. |
| Upstream | The daemon's `127.0.0.1:<port>` from `vowp up`; no public bind. |
| Paths | Serve the SPA and proxy `<base>/api/v1/*` on the same origin. Do not rewrite the `ApiError` JSON. |
| SSE | Disable response buffering and compression on `/api/v1/board/stream`; allow at least 60 s idle (the stream heartbeats every 15 s); do not cache `text/event-stream`. |
| Caching | `Cache-Control: no-store` for `/api/v1/*`; never cache board or inbox responses. |
| Headers | Set the CSP in §5.3; do not add CORS headers; do not inject `VOWP_API_TOKEN`. |
| Logs | Do not log `Authorization`, cookies, query strings, request bodies, or response bodies. |
| Auth | Require a user login/session for any non-loopback deployment. Reads are otherwise open to anyone who reaches the proxy. |

The daemon remains loopback-only. `vowp tunnel open window` is an SSH-forwarded loopback
path, not a public listener, but it still serves VOWP's page rather than the StackSpirit SPA.

### 5.3 URL, log, and error hygiene

- No home path, SSH key, host-key fingerprint, provider key, or daemon token in any URL.
  The only URL the user supplies is the base URL; it is validated and has no userinfo,
  query, or fragment.
- The `?daemon=` value is stored in `sessionStorage` and removed from the address bar with
  `history.replaceState`.
- Never log request/response bodies, headers, the base URL's query, or the daemon's
  `ApiError.hint` when it is not needed. A debug log may include method, path template,
  status, and duration; it never includes values from the payload.
- Errors show the daemon's path-free message plus a fixed StackSpirit title. They never
  concatenate raw response text, HTML, a stack trace, or an internal URL.
- `fleet.job(id)` omits `Job.worktree` and every home path. `report_path` is kept only when
  VOWP reports a repo-relative value; absolute, home, traversal, and Windows paths become
  `null`. Run detail and gate evidence summaries strip control characters and local
  absolute paths, and never include a raw output tail, report body, token, or provider key.
- The UI renders all server strings as text nodes (React escapes by default); no
  `dangerouslySetInnerHTML`.
- Production CSP:

  ```text
  default-src 'self';
  script-src 'self';
  style-src 'self' 'unsafe-inline';
  img-src 'self' data:;
  connect-src 'self';
  font-src 'self';
  frame-ancestors 'none';
  base-uri 'none';
  form-action 'self';
  ```

  `connect-src 'self'` is correct only under the same-origin requirement. A development
  server may add the dev websocket origin; the production build must not.
- No analytics, telemetry, third-party scripts, remote fonts, or external images.
- If the SPA registers a service worker, it must bypass `/api/` and any
  `text/event-stream` request. API and SSE data are never cached as authoritative.
- The W1 shell does not fetch `/api/v1/nodes`; this avoids putting `ssh_target` values in
  the browser and in logs for no W1 user value.

### 5.4 Local write proxy (W1c)

The proxy is a separate narrow component, not the Web app and not a generic reverse proxy.

| Property | Contract |
|---|---|
| Bind | Loopback only, behind the same-origin reverse proxy. |
| Token | Reads `VOWP_API_TOKEN` from its own environment; never forwards it to the browser; never logs it. |
| Allowlist | `POST /api/v1/jobs/{id}/retry`, `/park`, `/cancel`, and `POST /api/v1/inbox/{id}/answer`. No other method, route, or path prefix. No GET proxying, no ASIF, no switch. |
| Proxy routing | The reverse proxy sends exactly those four `POST` path shapes to the write proxy and every other `/api/v1/*` request to the daemon. The proxy's capability route is `/api/spirit/write-capabilities`, outside `/api/v1`, so it never shadows a daemon route. |
| Origin | Exact `Origin`/`Sec-Fetch-Site` check; reject foreign origins. |
| CSRF | HttpOnly, `SameSite=Strict`, `Secure` session cookie plus a custom `X-Spirit-CSRF` header bound to the session. |
| Body limit | 16 KiB at the proxy; the daemon still enforces the 10,000-byte answer/reason limit. |
| Rate limit | At least 10 writes per minute per session; `429` with `Retry-After`. |
| Capability probe | `GET /api/spirit/write-capabilities` returns `{"job_actions":true,"inbox_answer":true}`. The bridge probes it once; absence means actions stay hidden. |
| Response | Pass through the daemon's status and `ApiError` JSON unchanged; the bridge performs the `401`/`503` mapping. Do not expose the daemon's URL. |
| Audit | Method, route template, id, status, duration. No bodies, headers, token, or home path. |
| Shutdown | Closing the proxy must not stop `vowp up` or the daemon. |

The proxy does not make reads. Reads continue to go directly to the daemon through the
reverse proxy on the same origin.

---

## 6. Phased W1 plan

### 6.1 W1a — read-only board, fleet, jobs, inbox

Scope:

- Scaffold `apps/spirit-web` (Vite + React + TypeScript) that imports `@stackspirit/ui`.
- Add `HttpSseBridge` with the read methods in §2 and the semantics in §4.
- Add the capability map, the nullable `capacity`/`supervisor`, and the `unavailable`
  notices from §3.6.
- Shell: top bar (node, project, daemon version, connection state, preview badge), fleet
  canvas, job lanes, inbox drawer, capacity notice. A lane card opens a read-only job
  drawer with the job, its runs oldest first, and the newest-attempt gate results. Hosts,
  Engine, and Gates are absent from the nav; direct URLs show the desktop-only panel.
- Select one project from `Health.project`; no project switcher.
- Show slots as a plain “N / M configured slots” fact from
  `Board.slots_busy / slots_total`; do not draw the target-based worker-slot ring, because
  there is no target.

Acceptance:

- With the dev proxy pointed at a live `vowp up` daemon (or the production proxy in front
  of it), the board, running cards, job lanes, and open inbox render.
- The network trace contains only `GET` to `/api/v1/health`, `/api/v1/board`,
  `/api/v1/board/stream`, `/api/v1/jobs`, `/api/v1/sessions`, and `/api/v1/inbox`, plus the
  one-time W1b capability probes to `/api/v1/capacity` and `/api/v1/fleet` (expected `404`
  in W1a; no retry loop), plus `/api/v1/jobs/{id}`, `/runs`, and `/gates` for each job in the
  review/fix lane and for the opened drawer.
- No `Authorization` header, no `POST`, no token field, and no provider key anywhere.
- The capacity panel shows `CAPACITY_DETAIL_UNAVAILABLE` and does not contain “retrying”,
  “0 B”, “0%”, “target 0”, or a guessed advised worker count.
- The supervisor area shows the heartbeat age and `SUPERVISOR_STATE_UNAVAILABLE`; it never
  says “stopped” or “running” from `health.heartbeat` alone.
- The review/fix lane shows its count and the real failed gates from
  `GET /api/v1/jobs/{id}/gates`, or a true empty state. A missing route shows
  `GATE_FAILURES_UNAVAILABLE`; a failed read shows the `read_failed` notice. It never shows
  an empty “no failures” lane for a failed read.
- A stale stream falls back to 5 s polling; three failed polls show stale and the 30 s
  countdown.
- No console errors, no unhandled rejection, and no mock fallback when the live daemon is
  unreachable.

### 6.2 W1b — VOWP contract ask: capacity/storage and fleet state

This is a **VOWP v0.3 daemon API gap**, not a `v4-*` milestone. Neither
`docs/design/v0.3/http-api.md` nor the `vowp-daemon` router serves a capacity/storage
document. Do not claim the route exists and do not gate the ask on an invented milestone.

#### Ask 1: `GET /api/v1/capacity`

Token-free read, `Cache-Control: no-store`, `Content-Type: application/json`. Return the
exact `vowp capacity report --json` document (the `vowp-protocol` `CapacityReport` plus the
CLI extras `provider_caps` and `reload_pending`), so StackSpirit keeps one parser. Exact
fields:

| Field | Type | Required | W1 use |
|---|---|---|---|
| `generated_at` | RFC 3339 timestamp | yes | Freshness of the report. |
| `host` | `HostSample` or `null` | yes | `at`, `interval_secs`, `mem_total_bytes`, `mem_available_bytes`, `swap_used_bytes`, `load1`, `load5`, `load15`, `cpu_count`, `cpu_quota_cores`, `disk_free_home_bytes`, `disk_free_worktrees_bytes`, `pressure_cpu_avg10`, `pressure_mem_avg10`, `pressure_io_avg10`, `source`. |
| `target` | `CapacityTarget` | yes | `mode`, `target_workers`, `advised_workers`, `is_override`, `set_by`, `set_at`, `reason`. |
| `running` | `RunningCapacity` | yes | `runs`, `ram_p90_bytes`, `cpu_p90_cores`. |
| `brakes` | `BrakeState` | yes | **Plural** name, exactly as the protocol type. `active`, `reasons[]` (`code`, `detail`), `since`. |
| `reserve_ram_bytes` | u64 | yes | RAM reserve floor. |
| `min_disk_bytes` | u64 | yes | Disk floor. |
| `estimates` | `UsageEstimate[]` | yes (may be empty) | `project`, `job_kind`, `harness`, `model`, `build_mode`, `samples`, `ram_p50_bytes`, `ram_p90_bytes`, `cpu_p50_cores`, `cpu_p90_cores`, `duration_p50_ms`, `duration_p90_ms`, `source`. |
| `waiting` | `WaitingJob[]` | yes (may be empty) | `project`, `job`, `reason.code`, `reason.detail`, `since`. |
| `limits` | `Limit[]` | yes (may be empty) | The full limit row, not the reduced `BoardLimit`. |
| `provider_caps` | object<provider, u32> | yes (CLI extra) | Effective caps. |
| `reload_pending` | bool | yes (CLI extra) | Config reload note. |

Response rules: `200` for the served project; `503` when the home cannot be opened; a
path-free `500` otherwise. No token, no home path, no provider key. A daemon that cannot
read the sampler returns `host: null`; it does not invent a zero sample.

When this route returns `200`, `HttpSseBridge` fills `CapacityView` from it and sets
`capacity_detail: true`. When it returns `404`, the capability stays false and the exact
capacity copy remains. No code change is needed to flip the capability.

#### Ask 2: `GET /api/v1/fleet`

Token-free read returning the exact `vowp fleet status --json` wrapper for the served
project: `snapshot`, `source`, `fetched_at`, `stale`. The snapshot fields W1 needs are:

- `home`, `generated_at`, `sequence`.
- `supervisor`: `running`, `pid`, `paused`, `last_tick`, `tick_secs`, `fail_streak`,
  `version`.
- `projects[]`: `name`, `state`, `paused_reason`, `jobs_by_state`, `cost_today_usd`.
- `sessions[]`: `id`, `project`, `job_id`, `role`, `title`, `state`, `executor`, `node`,
  `active_surface`, `last_event_at`.
- `orchestrators[]`: `name`, `session_id`, `project`, `app`, `state`, `join`, and a `node`
  field when available.
- `capacity` (the fast path), `limits`, `inbox` summaries.

This single route would fill `SupervisorView`, `ProjectView.state`, session surfaces,
orchestrators, and the `source`/`stale` flags. It is optional for W1a; without it those
fields stay `null` plus notices.

#### Ask 3: `GET /api/v1/jobs/{id}` — served and consumed

The daemon registers `GET /api/v1/jobs/{id}` (the full `Job`) and its `/runs`
(`RunPage`) and `/gates` (`GateResultPage`) sub-resources as token-free reads. The
StackSpirit gap is now closed by `fleet.job(id)`:

| Piece | Contract |
|---|---|
| Method | `fleet.job(id): Promise<JobDetailView>` on the Web and preview bridges. Desktop bridges may omit it until the desktop drawer adopts it. |
| Reads | `GET /api/v1/jobs/{id}` first, then `/runs` and `/gates` in parallel. All three use `credentials: "omit"`, `redirect: "manual"`, `cache: "no-store"`, and the same-origin guard. |
| Job fields | `id`, `project`, `title`, `kind`, `role`, `state`, `priority`, `needs`, `attempts`, `note`, `parked_reason`, `report_path`, `created_at`, `state_changed_at`, `updated_at`, `last_progress_at`. No `worktree` and no home path. |
| Runs | Oldest first by `started_at` then id. Each row carries `id`, `role`, `purpose`, executor, `started_at`, `ended_at`, `outcome`, `exit_code`, and a sanitized `detail`. |
| Gates | The newest attempt, ordered by `gate_index`. Each row carries `attempt`, `gate_index`, `kind`, `passed`, a safe `evidence_summary`, `duration_ms`, and `checked_at`. |
| Errors | A 404 on the job is `not_found` with `JOB_DETAIL_NOT_FOUND`; a 404 on `/runs` or `/gates` after the job read is `daemon_incompatible`; a 500 is `remote`; a network failure is `daemon_unreachable`. A missing page is never an empty list. |

The review/fix lane uses the same method for jobs in `gating`, `reviewing`, and `fixing`
and shows the real failed gate or a true empty state. The remaining job-detail gap is only
the daemon contract itself: the routes stay unpaginated (`next: null`); if VOWP ever
paginates them, the view sets `runs_truncated`/`gates_truncated` and the drawer labels it
rather than presenting the page as complete.

#### Ask 4: capacity target write

`POST /api/v1/capacity/target` (token-gated, body `{target, mode, override, reason}`) is
missing. The daemon sets `by` itself (for example `web` or `stackspirit`); the browser
never sends a spoofable actor. `fleet.setTarget` stays unavailable and the target editor
stays hidden until both the read route and a write proxy exist.

### 6.3 W1c — token-gated writes through the local proxy

Scope:

- Build the narrow proxy in §5.4 and its `GET /api/spirit/write-capabilities` probe.
- Enable `fleet.jobAction` and `fleet.answerInbox` only when both the route and the proxy
  capability are present.
- Show confirmations for cancel and park; `cancel` is danger-styled and confirmed.
- On `202`, say “queued; the supervisor applies it on its next tick”, refresh the board and
  jobs, and never claim the job already moved.
- On `409`, say “VOWP changed this item; refreshing” and refresh.
- On `401`/`503`, hide actions and show the auth/writes-disabled copy; never ask for the
  daemon token.

Acceptance:

- The browser has no daemon token in JS, storage, URL, or DOM; only the proxy session
  cookie is present.
- A cross-origin request, a missing CSRF header, or a foreign `Origin` is rejected by the
  proxy before the daemon is called.
- A read-only build with no proxy sends zero `POST`s and shows `READ_ONLY_NOTICE`.
- The proxy allowlist is tested: `switch`, ASIF import, and arbitrary paths are `404`/`405`.

### 6.4 Explicit non-goals

- No `apps/spirit-web` code is created by this document.
- No second parser: the Web bridge consumes the same view types as the desktop.
- No live SSH, host-key, keychain, engine supervision, or gate probe in the browser.
- No project switcher, multi-home, or `All projects` view.
- No ASIF export/import in W1; those remain desktop D5 and gated on `v4-asif-*`.
- No local orchestrator actions; those remain desktop D6 and gated on VOWP Phase 2/3.
- No capacity values, supervisor state, host list, or engine state from a guessed source.
- No CORS change in VOWP and no public daemon bind.

---

## 7. Test plan

### 7.1 Bridge unit tests

Location: `apps/spirit-web/src/bridge/HttpSseBridge.test.ts` (Vitest, jsdom), with
`fakeFetch` and `FakeEventSource` helpers. No live daemon, no network, no timers left open.

| Test | Asserts |
|---|---|
| base URL resolution | query wins over env; trailing slash stripped; no hardcoded port; missing → setup state; userinfo/query/fragment/bad scheme → `invalid_input` |
| same-origin guard | a different origin → `daemon_incompatible` + `SAME_ORIGIN_REQUIRED` |
| board frame | exact `Board` JSON replaces the view; no array merge; revision increments once |
| heartbeat | no revision bump, no fleet event, liveness updated |
| unknown event | ignored without an error |
| bad board JSON | previous view retained; `parse` error; no partial view |
| request race | a poll response older than an applied SSE frame is dropped; `refresh()` returns the retained newer view |
| stream error | 5 s polling starts; retry 1→2→4→8→15 s; `open` resets; polling stops on a successful frame |
| heartbeat timeout | 45 s without a frame is treated as a stream failure |
| three poll failures | connection `stale`, `fleet.stale`, 30 s backoff, countdown set |
| unchanged board poll | no jobs/sessions/inbox refetch |
| changed board poll | one coalesced refetch; a second change during the fetch causes exactly one more |
| inbox all | three state requests merged, sorted by `created_at` then `id`, de-duplicated by id; `state=all` never sent |
| inbox open | one request with `state=open` |
| job detail | the three routes return `JobDetailView`; runs sorted oldest first; gates sorted by index; no worktree or home path; `report_path` only when repo-relative |
| job detail errors | job 404 → `not_found`; `/runs` or `/gates` 404 after the job read → `daemon_incompatible`; 500 retried once → `remote`; network → `daemon_unreachable`; an invalid id rejects before fetch |
| review/fix lane | cards come from `GET /api/v1/jobs` plus `fleet.job(id)`; a real failed gate shows its kind and summary; a passed-only or empty gate page says so; a missing route adds `no_route`, a failed read adds `read_failed`, and neither is a false “no failures” state |
| job actions | correct URL per action; no body for retry/cancel; reason body only for park; `202` → `ActionResult` with `refreshed` |
| answer inbox | `202 InboxItem` → `InboxItemView`; `answered_by` preserved |
| errors | 400/401/404 fixed route/404 write/409/413/415/500/503/non-JSON/timeout/network map to the exact codes and copy |
| redirects | `redirect: "manual"`; a `3xx`/opaque redirect is `daemon_incompatible` and is not followed |
| capacity | `null` + `no_route`; no canvas, no zeroes |
| setTarget | rejects before fetch with `no_route` |
| desktop-only | every `hosts`, `engine`, `gates` method and `connect`/`disconnect` rejects with `desktop_only` and makes zero fetch calls |
| unsubscribe | closes the stream, clears timers, aborts in-flight requests |

### 7.2 `MockBridge` Playwright flows

Location: `apps/spirit-web/e2e/read-only.spec.ts`, `reconnect.spec.ts`,
`accessibility.spec.ts`. Each Playwright project builds with `VITE_SPIRIT_MOCK=1` and its
own `VITE_SPIRIT_MOCK_SCENARIO` (`web-read-only`, `web-stale`, `web-recovering`); no daemon.
The scenario is fixed at build time; a test must not switch it silently.

| Flow | Asserts |
|---|---|
| read-only board | board, running cards, lanes, and open inbox render; preview badge is present |
| capacity unavailable | `CAPACITY_DETAIL_UNAVAILABLE` and `CAPACITY_DETAIL_EXPLAIN` are visible; “retrying” is absent; no `0 B`, `0%`, or “target 0” in the panel |
| desktop-only surfaces | Hosts/Engine/Gates are absent from the nav; direct `/hosts`, `/engine`, `/gates` show the exact desktop-only copy |
| read-only network | `page.on("request")`: every `/api/` request is a `GET` to an allowlisted route; no `Authorization` header; no `POST` |
| stale/reconnect | `web-stale` scenario dims data, shows the stale banner and the retry countdown; a recovery scenario clears it |
| review/fix lane | count is visible; the real failed gate kind and safe evidence summary render from `fleet.job(id)`; there is no `GATE_FAILURES_UNAVAILABLE` placeholder when the routes answer |
| job drawer | a lane card opens the job, runs oldest first, and newest-attempt gates; a job with no runs shows a true empty state; Esc closes the drawer and focus returns to the trigger |
| inbox drawer | full body/options; answer control absent without the write capability; Esc closes the drawer; focus returns to the trigger |
| disabled actions | every disabled control has its reason text, per `PLAN.md` §10.4 |

### 7.3 Fixture and contract tests

- `apps/spirit-web/src/test/fixtures.test.ts` parses captured `Board`, `JobPage`,
  `SessionPage`, and `InboxPage` documents through the bridge. Fixtures use documentation
  addresses and fake values only.
- A compile-time exhaustive coverage map lists every `SpiritBridge` method and its
  availability class; the test fails if a method is added without a class.
- A route constant lists every route in §2; a test asserts the bridge only calls those
  routes and never a static VOWP page.
- A shape test asserts `Health.project == Board.project`; a mismatch is
  `daemon_incompatible`, not a mixed view.

### 7.4 Security tests

- A read-only build sends zero `POST`s and zero `Authorization` headers (fetch spy and
  Playwright).
- No `VOWP_API_TOKEN`, provider key, PEM block, or home-directory prefix appears in source,
  fixtures, built assets, error strings, or URLs. The test asserts on patterns, so no real
  credential is ever needed.
- Base URL validation rejects userinfo and query strings; the error never echoes the value.
- `ApiError.message` is rendered as text; a fixture containing HTML tags is displayed
  literally, not parsed.
- A service worker test (if one is added) proves `/api/` and `text/event-stream` bypass the
  cache.
- The proxy test (W1c) rejects foreign origins, missing CSRF headers, oversized bodies, and
  routes outside the allowlist.

### 7.5 Manual live smoke (not CI)

1. Run `vowp up` against a disposable test home and read the `web window` address from its
   output or `vowp up --json`'s `web_window`. Never commit the address or the token.
2. Start the Web dev server with `VOWP_DAEMON_URL` set to the daemon base for the Vite proxy
   target and `VITE_VOWP_DAEMON_URL` set to the dev-server origin.
3. Verify the board, running cards, lanes, and inbox against the daemon's own page; verify
   the capacity notice and the supervisor notice.
4. Stop the daemon and verify `daemon_unreachable`, then restart and verify recovery.
5. Do not use a real host, a real provider key, or a `.env` file. The smoke is opt-in and
   never part of CI.

### 7.6 CI gates for the future Web job

```sh
npm ci
npm run typecheck -w @stackspirit/web
npm run lint -w @stackspirit/web
npm test -w @stackspirit/web
npm run build -w @stackspirit/web
npm run test:e2e -w @stackspirit/web   # MockBridge only
```

No live daemon, no network, no real keychain, no real provider call. The existing desktop
gates stay unchanged.

---

## 8. Gap summary

| Gap | Status | Owner | Exact route / field |
|---|---|---|---|
| Capacity and storage read | missing | VOWP daemon | `GET /api/v1/capacity`; the exact `CapacityReport` fields in §6.2 |
| Supervisor state | missing | VOWP daemon | `GET /api/v1/fleet`; `snapshot.supervisor` |
| Project state and paused reason | missing | VOWP daemon | `GET /api/v1/fleet`; `snapshot.projects[]` |
| Session active surface and last event | missing | VOWP daemon | `GET /api/v1/fleet`; `snapshot.sessions[]` |
| Orchestrator cards with node | missing | VOWP daemon | `GET /api/v1/fleet`; `snapshot.orchestrators[]` |
| Job detail and gate failures | closed (W1b) | StackSpirit | `fleet.job(id)` consumes `GET /api/v1/jobs/{id}` with `/runs` and `/gates` |
| Capacity target write | missing | VOWP daemon | `POST /api/v1/capacity/target`, token-gated |
| Host profiles, SSH, keychain, engine | desktop-only | StackSpirit | no daemon route; §2.2, §2.5 |
| Capability gates | desktop-only | StackSpirit | no daemon route; §2.6 |
| Job/inbox writes in the browser | W1c | StackSpirit | local write proxy; §5.4 |
| CORS | not provided | VOWP daemon | same-origin proxy is the supported path; §1.3 |
| Web app | W1a read-only implemented | StackSpirit W1 | `apps/spirit-web`; see §9 |

Until every “missing” row lands, the Web shell shows the exact copy in §3.7 and no data for
that feature. It never fills a gap with zeroes, a guessed target, an empty state, or mock
data.

---

## 9. W1a implementation status (2026-11-15)

Status: **read-only Web shell implemented** in `apps/spirit-web` as `@stackspirit/web`.
The W1b read-only job drawer and review/fix gate detail are implemented; W1c writes and
every D5/D6-gated feature remain absent.

### 9.1 Local commands

```sh
npm ci
npm run typecheck
npm run lint
npm test
npm run build
npm run test:e2e
```

The Web e2e suite is **MockBridge-only**. Its Playwright projects build two preview bundles
with fixed scenarios (`fresh` and `stale-cache`) and never need a live daemon, a token, or a
network beyond the two local preview servers.

Development against a real daemon needs both the server-side proxy target and the browser
origin:

```sh
# Terminal 1: the real loopback daemon started by `vowp up` (read the port from its output).
export VOWP_DAEMON_URL=http://127.0.0.1:<port>

# Terminal 2: Vite proxies /api to VOWP_DAEMON_URL; the browser uses this same origin.
export VITE_VOWP_DAEMON_URL=http://127.0.0.1:5174
npm run dev:web
```

Do not put a token in either variable. The Web build performs no write and sends no
`Authorization` header. A production deployment serves the built SPA and proxies
`/api/v1/*` from the same origin, as §1.3 requires.

### 9.2 Routes actually called

`apps/spirit-web/src/bridge/HttpSseBridge.ts` calls only:

| Method | Route | Use |
|---|---|---|
| `GET` | `/api/v1/health` | Node, project, daemon version, schema, heartbeat age. |
| `GET` | `/api/v1/board` | Board facts, job counts, running cards, limits, cost, checkpoint. |
| `GET` | `/api/v1/board/stream` | `board` snapshots and `heartbeat` liveness. |
| `GET` | `/api/v1/jobs` | Ready, parked/blocked, title, priority, state-age rows. |
| `GET` | `/api/v1/jobs/{id}` | The opened job drawer's full job fields. |
| `GET` | `/api/v1/jobs/{id}/runs` | The drawer's runs, oldest first. |
| `GET` | `/api/v1/jobs/{id}/gates` | The review/fix lane card and drawer gates for the newest attempt. |
| `GET` | `/api/v1/sessions` | Join a running board run to its newest session node/id. |
| `GET` | `/api/v1/inbox?state=open` | Open inbox snapshot; `state=answered` and `state=dismissed` only for the read-only all-items drawer. |

The bridge does **not** call `/api/v1/capacity` or `/api/v1/fleet`. Those routes are absent
from the current daemon; the pre-build acceptance text proposed one-time `404` probes, but
the implementation deliberately makes no request to a route the served daemon does not
register. The capability stays false and the exact §3.7 copy is shown.

The bridge also does not call these existing routes, per §2.7:

| Route | Why it is not called |
|---|---|
| `GET /api/v1/cost` | No `SpiritBridge` cost method; `Board.cost_today_usd` is the W1 fact. |
| `GET /api/v1/nodes` | `Node` is not a `HostProfile`; W1 avoids putting `ssh_target` values in the browser. |
| `GET /api/v1/sessions/{id}` and `/events` | No session drawer or event-timeline method in W1. |
| `POST` routes | W1c write proxy is not built; zero `POST`s are sent. |

`GET /api/v1/jobs/{id}`, `/runs`, and `/gates` are served by the daemon and back
`fleet.job(id)`. The review/fix lane reads them for each `gating`/`reviewing`/`fixing` job,
and a lane card opens the drawer with the same data. A board run without a job row is
labelled “No matching job row for this board run”; it is not presented as a job detail.

### 9.3 Honest data mapping

`WebFleetView` is the Web read model. It deliberately does not reuse the desktop
`FleetView`, because `FleetView` requires a complete `CapacityView` and `SupervisorView`.
The Web snapshot supplies:

- `source: "daemon"`, `host_id: "daemon"`, a local monotonic `revision`, `fetched_at`, and
  `generated_at` from `Board.at` (labelled “snapshot changed at”).
- One project from `Health.project`; its `state` is `"unknown"` and the shell shows
  `PROJECT_STATE_UNAVAILABLE` instead of rendering an empty paused reason as “active”.
- No `CapacityView` is constructed at all; `capacity_unavailable = { code: "no_route",
  route: "/api/v1/capacity" }`. The capacity rail contains no gauges, target, brake,
  estimate, waiting list, or zeroes.
- No `SupervisorView` is constructed; `supervisor_unavailable` carries the exact reason.
  The shell shows only the heartbeat age from `/api/v1/health` and never infers “running” or
  “stopped”.
- A separate “Board facts” strip for `slots_busy / slots_total` (“configured slots”, not a
  target), active limits from `Board.limits`, `cost_today_usd` (`null` stays unknown),
  `inbox_open`, and `last_checkpoint`.
- Running cards joined to `Job` by id and to the newest `Session` by `job_id`; an unmatched
  board run stays visible with `node: null` and “No matching job row for this board run.”
- Lanes whose counts come from `Board.jobs`; ready and parked/blocked cards come from
  `GET /api/v1/jobs`; review/fix cards come from the same list plus `fleet.job(id)`, so each
  card shows its real failed gate or a true “no gate results” state; done is count-only.
- `fleet.job(id)` maps the full job, runs oldest first, and newest-attempt gates into
  `JobDetailView`. It omits `worktree` and home paths, keeps only a repo-relative report
  path, and sanitizes run detail and gate evidence.
- `sessions: []`, `orchestrators: []`, and explicit `UnavailableNotice` rows for
  `active_surfaces`, `orchestrators`, `raw_documents`, and the capacity and supervisor gaps.
  A `gate_failures` notice appears only when a served job-detail route is missing
  (`no_route`) or a present one fails (`read_failed`).

`JobPage.next` is treated honestly: `queue_truncated` becomes true and a notice is added if
the daemon ever paginates the list. `queue_cap` is `null` for the unpaginated list.

### 9.4 Bridge behaviour and tests

`HttpSseBridge` implements `SpiritBridge` with:

- Token-free reads using `credentials: "omit"`, `redirect: "manual"`, `cache: "no-store"`,
  and a 10 s `AbortController` timeout.
- Typed mapping for `daemon_unreachable`, `daemon_incompatible`, `remote_timeout`, `parse`,
  `auth`, `not_found`, `conflict`, `invalid_input`, `writes_disabled`, `desktop_only`, and
  `no_route`.
- Controlled SSE reconnect with 1 s → 2 s → 4 s → 8 s → 15 s backoff, a 45 s heartbeat
  timeout, a 5 s polling fallback, three-failure stale state with a 30 s backoff, and
  cleanup that closes the stream, clears timers, and aborts in-flight reads.
- Request-generation ordering: an older poll or refresh response is dropped when a newer
  SSE frame has already applied; `Board.at` is never used as a version.
- Coalesced secondary refetches for changed board frames; unchanged polls do not refetch
  jobs, sessions, or inbox.
- `fleet.inbox(true)` merges `open`, `answered`, and `dismissed`, sorted by `created_at`
  then `id`; `state=all` is never sent.
- `fleet.job(id)` reads the job first, then `/runs` and `/gates`, so a job 404 is
  `not_found` while a missing sub-route is `daemon_incompatible`. Runs are sorted oldest
  first, gates by index, and both pages remember a `next` cursor as a truncation flag.
- Desktop-only methods reject with `desktop_only` **before** any fetch; `setTarget` rejects
  with `no_route`; `jobAction` and `answerInbox` reject with `writes_disabled`.

Unit coverage lives in `apps/spirit-web/src/bridge/HttpSseBridge.test.ts` with a fake
fetch/EventSource and fake timers; no test needs a live daemon. The coverage map in
`src/bridge/contract.test.ts` fails if a bridge method is added without an availability
class. The source scan fails on an `Authorization` header, bearer literal, token assignment,
or local storage use.

### 9.5 Preview mode

`VITE_SPIRIT_MOCK=1` builds `WebMockBridge`, which wraps the shared `MockBridge` and replaces
its desktop capacity/supervisor fields with the same unavailable notices as the live bridge.
The shell shows a persistent **Preview data** badge in both the top bar and the content
header. An unknown scenario name falls back to `fresh`, never to a live daemon.
`web-read-only` maps to `fresh`, `web-stale` maps to the shared `stale-cache` scenario, and
`web-recovering` maps to `fresh` because the shared mock has no separate recovering
scenario. The mock never claims capacity; the capacity rail keeps the unavailable copy.

The shared mock also serves `fleet.job(id)` without a live daemon. In `fresh`,
`ss-9-gate-check` has two runs and two failed gates (the review/fix case), `ss-5-mock` is
ready with no runs (the true empty case), and any unknown id is a typed `not_found`. The
`job-detail-error` scenario makes every job read fail with `daemon_unreachable`. Preview
stays labeled with the persistent **Preview data** badge.

The Playwright flows are `e2e/read-only.spec.ts`, `e2e/stale.spec.ts`, and
`e2e/accessibility.spec.ts` (WCAG 2.2 AA). They assert no `/api/` request, no `POST`, no
`Authorization` header, the exact unavailable copy, the desktop-only direct routes, and the
read-only inbox drawer with no answer control.

### 9.6 Deliberate UI deviation

The shared `AppShell`, `FleetCanvas`, and `CapacityRail` are desktop-shaped: they read the
desktop store and require `FleetView.capacity`/`fleet.supervisor` to be non-null. Mounting
them in the Web build would force fake capacity or supervisor values. This job therefore:

- reuses the shared design system (`Badge`, `Button`, `Drawer`, `Panel`, `EmptyState`),
  the shared `JobCard`/`JobLane`/`WorkerCard` prop-driven views, and the shared lane/format
  domain helpers from `packages/spirit-ui` source;
- composes a Web-specific shell over `WebFleetView` and the same `SpiritBridge` boundary.

The clean follow-up is in `packages/spirit-ui` (outside this job's ownership): make
`FleetView.capacity`/`supervisor` nullable, add the §3.6 capability map to the shared shell,
export the pure job views for all hosts, and add the five Web error codes to the shared
`errors.ts` title table. The Web shell uses a local title map in
`apps/spirit-web/src/bridge/errors.ts` today. Until those follow-ups land, the Web shell
keeps the honest `WebFleetView` and does not mount a desktop view with fabricated fields.

### 9.7 Still missing (unchanged W1b/W1c)

| Gap | Status | Exact route / field |
|---|---|---|
| Capacity and storage read | missing | `GET /api/v1/capacity` |
| Supervisor state | missing | `GET /api/v1/fleet`; `snapshot.supervisor` |
| Project state | missing | `GET /api/v1/fleet`; `snapshot.projects[].state` |
| Session surfaces and orchestrators | missing | `GET /api/v1/fleet`; session/orchestrator fields |
| Capacity target write | missing | `POST /api/v1/capacity/target` |
| Job and inbox writes in the browser | W1c | local write proxy; §5.4 |
| Hosts, Engine, Gates | desktop-only | no daemon route; §2.2, §2.5, §2.6 |

No gated D5/D6 feature is claimed. The Web build is read-only and has no browser token,
provider key, SSH code, or mock fallback on a live failure.

---

## 10. Meadow design alignment (ss-27, 2026-11-18)

Status: the read-only cockpit now uses the desktop shell's Meadow tokens and surfaces.
No bridge method, route, write, auth, or network surface changed.

### 10.1 What changed

- `apps/spirit-web/index.html` sets `data-ui="next"` on `<html>`. The app no longer pins
  `data-theme="dark"`, so light is the default and dark follows the OS, like the desktop
  shell. The `color-scheme` meta is `light dark`.
- `apps/spirit-web/src/main.tsx` loads the shared Meadow stylesheet
  (`packages/spirit-ui/src/next/theme.css`) after `@stackspirit/ui/styles.css`, then the
  shared fleet stylesheet (`packages/spirit-ui/src/views/fleet/fleet.css`), then
  `src/web.css`. Every colour, radius, shadow, and focus ring comes from the `--nx-*`
  Meadow tokens.
- Geometry follows the desktop shell: 14 px body text and rows, 32 px sidebar and fact
  rows, 56 px page headers, 12 px secondary text, card/panel surfaces (`--nx-panel`,
  `--nx-raised`), status dots, and the AA-safe Meadow button fill (`--nx-btn-bg`, `#2e7f48`
  in light mode).
- `src/web.css` restyles the reused classic primitives (`Button`, `Badge`, `Panel`,
  `Drawer`, `EmptyState`, `JobCard`, `JobLane`, `WorkerCard`) with the Meadow tokens. The
  page header is fixed above a scrolling content column, and the five job lanes wrap
  responsively instead of hiding lanes behind horizontal scroll.

### 10.2 Shared components reused

- `next/SpiritMark` in the top bar, setup page, boot error, and desktop-only shell.
- `next/NodeMap` for the read-only **Fleet structure** panel: daemon → project → exact
  lane counts. It is a pure props component; the leaves are labels, not actions.
- Not reused: `NextShell`, `Sidebar`, `ServerPage`, `WorkersPage`, `JobRows`,
  `PageHeader`, `ContextPanel`, and `ErrorNotice`. They read the desktop store
  (`useFleetStore`/`useShellUi`) or open the desktop details panel, so mounting them would
  add desktop-only actions or a dead panel toggle to the read-only cockpit. Their layout
  and tokens are reused instead.

### 10.3 Still classic

- The public `@stackspirit/ui` primitives `Button`, `Badge`, `Panel`, `Drawer`,
  `EmptyState`, and the prop-driven `JobCard`/`JobLane`/`WorkerCard` views are still the
  classic components. The Web app does not fork them: `web.css` restyles them with Meadow
  tokens and leaves their DOM, props, and test ids unchanged.
- `AppShell`, `TopBar`, `FleetCanvas`, and `CapacityRail` remain unmounted because they
  need a desktop `FleetView` with capacity and supervisor data. The Web-specific shell
  remains, as §9.6 describes.
- The classic `--state-verifier` token has no Meadow equivalent. `web.css` maps it to
  `#6d28d9` in light mode and `#c4b5fd` in dark mode so gate/verifier badges and the
  review/fix lane pass WCAG AA. This is a Web-local token, not a shared theme change.

### 10.4 Missing shared exports (follow-up outside this job)

- `packages/spirit-ui/package.json` exports only `.` and `./styles.css`. The `next/`
  components and `next/theme.css` are therefore deep-imported from the workspace source,
  the same pattern the Web app already used for `views/fleet/*` and `domain/fleet`. A
  future change can add `./next` and `./next/theme.css` exports; this job does not edit the
  desktop package.
- `--state-verifier` could become a Meadow token (`--nx-verifier` / `--nx-verifier-soft`)
  so each host does not map it locally.

### 10.5 Verification (2026-11-18)

```sh
npm run typecheck -w @stackspirit/web   # pass
npm run lint -w @stackspirit/web        # pass
npm test -w @stackspirit/web            # 73 passed
npm run test:e2e -w @stackspirit/web    # 10 passed
```

The e2e run includes the three WCAG 2.2 AA axe checks (fleet view, inbox drawer, job
drawer). No bridge test, route, copy constant, write action, or network request changed.
§9 remains the data and transport contract; this section only records the visual
alignment.

---

## 11. Current Web/PWA state (0.6.1+)

Status: current implementation, 2026-11-27. This section records the shipped app after the
account-first Web work (`048e13e`). The app now has a `/cloud` route and an account session in
addition to the read-only W1a cockpit; the no-account path is unchanged. The transport,
data-honesty and bridge rules in §1–§4 still govern the daemon cockpit, and §9–§10 remain the W1a
and Meadow history.

Source of truth for this section:
[`main.tsx`](../apps/spirit-web/src/main.tsx),
[`WebAuthPanel.tsx`](../apps/spirit-web/src/app/WebAuthPanel.tsx),
[`session.ts`](../apps/spirit-web/src/auth/session.ts),
[`client.ts`](../apps/spirit-web/src/auth/client.ts),
[`CloudApp.tsx`](../apps/spirit-web/src/app/CloudApp.tsx),
[`cloud/runtime.ts`](../apps/spirit-web/src/cloud/runtime.ts),
[`relayBridge.ts`](../apps/spirit-web/src/bridge/relayBridge.ts),
[`CloudRelayClient.ts`](../apps/spirit-web/src/bridge/CloudRelayClient.ts),
[`relayCrypto.ts`](../apps/spirit-web/src/bridge/relayCrypto.ts),
[`registerServiceWorker.ts`](../apps/spirit-web/src/registerServiceWorker.ts),
[`manifest.webmanifest`](../apps/spirit-web/public/manifest.webmanifest) and
[`sw.js`](../apps/spirit-web/public/sw.js).

### 11.1 Routes

| Route | What ships |
|---|---|
| `/` | The read-only daemon cockpit over the daemon HTTP/SSE API. It is the W1a app in §9 and needs no account. |
| `/cloud` | The optional StackSpirit Account / Always-On Homes app. It is never the boot default, offers a local-daemon link, and keeps the account flow separate from the daemon cockpit. |
| `?cloud=1` | An alias for `/cloud`: `main.tsx` renders `CloudApp` when the query value is `1`. |

`main.tsx` still sends `/hosts`, `/engine` and `/gates` to the desktop-only page and every other
path to the daemon cockpit. Constructing the cloud runtime is inert: the root page makes no cloud
request and opens no WebSocket until the owner signs in or connects a Home.

### 11.2 Account authentication

Sign-in is optional and belongs to `/cloud`; the no-account cockpit at `/` never asks for an
account.

- `WebAuthPanel` offers Google and GitHub OAuth starts, email sign-up with a six-digit
  verification code, and email/password sign-in. A signed-in account with no enrolled machine
  gets the two onboarding paths: link an existing VPS with a one-time enrollment token, or open
  the Managed Homes plan catalogue.
- The OAuth callback arrives as `#auth_token=<token>`. `consumeAuthTokenFragment` in
  `src/auth/session.ts` reads it once in the first-render state initializer, validates the opaque
  bearer pattern, stores a valid value in module memory, and calls `history.replaceState` to
  remove the fragment from the address bar and history before any UI can display it. A malformed
  `auth_token` is also scrubbed and is not stored.
- The token lives in `src/auth/session.ts` memory only. `WebCloudClient` reads it when it sends an
  authenticated cloud request and puts it in the `Authorization: Bearer` header for that request.
  It is never written to `localStorage`, `sessionStorage`, a cookie, a log, an error or a view
  model. Cloud requests use `credentials: "omit"`.
- Sign-out calls `client.clearSession()`, which calls `setSessionToken(null)`; disposing the cloud
  runtime clears the session too.
- The cloud API is a separate origin (`api.stackspirit.io` by default) from the local daemon. The
  account token is never sent to the daemon or used for `/api/v1` reads.

The historical no-browser-token statements in §5.1 and §9.7 remain true for the daemon
`VOWP_API_TOKEN` and the no-account cockpit; the StackSpirit account token is a separate cloud
credential.

### 11.3 No-account daemon cockpit

`/` is unchanged. `HttpSseBridge` performs token-free `GET` reads and the board SSE stream against
the daemon, under the same route, data-honesty and read-only rules as §2, §3 and §9. It has no
token input and sends no `Authorization` header to the daemon. Job and inbox actions stay hidden
or reject with `writes_disabled` because the W1c local write proxy is still not in this tree
(§5.4). Everything in §9.7 still applies to this route.

### 11.4 PWA shell and service worker

`index.html` links `manifest.webmanifest`, whose `start_url` and `scope` are `/` and whose display
mode is `standalone`; the manifest supplies 192 px, 512 px and maskable icons. The app is
installable as a PWA on browsers that support the manifest and service worker; it is not a
separate native app.

`registerServiceWorker` registers `/sw.js` in production builds only. The service worker caches
the static shell (`/`, the manifest and two icons) at install, then caches only same-origin `GET`
requests under `/assets/` and `/icons/`; navigation is network-first with the cached `/` as the
offline fallback. It returns before caching for `/api/`, `Accept: text/event-stream` (SSE),
non-`GET` and cross-origin requests, so daemon API and stream data are never served from the
cache.

### 11.5 Cloud route and account actions

`CloudApp` and `createWebCloudRuntime` in `src/cloud/runtime.ts` wire the account API and the
encrypted relay. The `/cloud` page exposes what the cloud client actually provides:
Google/GitHub OAuth starts, email sign-up/verification/login, the account and Always-On Home list,
one-time enrollment tokens, the published Managed Homes plan catalogue, and provisioning status
returned by the API. These account and cloud actions live in `WebCloudClient`
(`src/auth/client.ts`), not in the daemon bridge. This tree contains no checkout or billing call;
the plan catalogue and provisioning panel are reads, not a claim that purchase or provisioning is
live here.

After sign-in and machine selection, `CloudApp` mounts the same read-only `WebApp` over
`createRelayBridge`. The Web runtime does not enable writes (`CloudRelayClient` defaults to
`allowWrites: false`), and the mounted shell exposes no job or inbox action controls. Board SSE is
not tunnelled in v1, so the relay-backed cockpit uses the bridge's documented 5-second polling
fallback. There is no second daemon write surface on `/cloud`; the cockpit's read-only rules are
the same as §9.

The account model — account-free local mode as the default, and an optional free account for Web,
cross-device and Managed Home access — is in [`docs/STRATEGY.md`](STRATEGY.md) §4.

### 11.6 Relay, hub and CSP notes

- The Web/PWA relay client, the per-machine `MachineHub` Durable Object and the relay security
  model are described in
  [`runtime/docs/design/relay-and-packaging.md`](../runtime/docs/design/relay-and-packaging.md)
  §6.7–§6.8. That is a design note for the relay; the hosted MCP endpoint it describes at
  `mcp.stackspirit.io` is a separate planned surface, not part of the Web app in this tree.
- The production CSP and reverse-proxy rules for the Web build are in §5.3 of this document. The
  static marketing site's own CSP, cache and security headers are in
  [`apps/site/_headers`](../apps/site/_headers).
