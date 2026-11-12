# v0.3 — daemon HTTP + SSE API

Status: the read-only API (including session detail and single-capsule reads), the
per-session SSE stream and the board snapshot SSE stream are implemented in
`crates/vowp-daemon`; the rest of this note is design. The daemon is **not** part of v0.2
and is not wired into `vowp up` yet. Nothing here is implemented in v0.1.

## First slice (implemented)

`crates/vowp-daemon` ships a library and the `vowp-daemon` binary:

`vowp-daemon [--home <dir>] [--project <name>] [--port <n>]`

It resolves the home from `--home`, `VOWP_HOME`, then `$HOME/.vowp`; selects the only
non-reserved registered project (or the one named by `--project`, with a clear error when
zero or several are registered); reads the node name from that project's registered
`vowp.toml` config; and opens the home with `Store::open_read_only`. It never creates,
migrates or writes a home. The listener binds `127.0.0.1` only, defaults to port `0` and
prints the bound address.

Served endpoints, all `GET`; every response is JSON except the two SSE streams:

- `/api/v1/health` — binary version, node, project, project schema version and the
  supervisor heartbeat from `<home>/heartbeat` (`null` when absent, unreadable or stopped).
- `/api/v1/board` — the exact `vowp board --json` document, computed from
  `BoardSnapshot::collect` against the served project's read-only store and registered
  `VowpConfig`.
- `/api/v1/board/stream` — a `text/event-stream` response. The first frame is
  `event: board` with the exact `Board` JSON from `GET /api/v1/board`; the snapshot is then
  collected once per second and another `event: board` frame is sent only when the document
  changes apart from its wall-clock `at`. An `event: heartbeat` frame with
  `{"at":"<RFC 3339>"}` is sent every 15 seconds. There is no cursor, `after_seq`, resume,
  merge or resync: the board is a self-contained snapshot. Each poll opens and drops its own
  read-only store on `spawn_blocking`, so no handle is held between polls and a slow client
  waits for the next snapshot. A collect error ends the stream cleanly; the page reconnects
  and keeps its five-second polling fallback. The board still has no write or auth surface.
- `/api/v1/jobs?state=&kind=&role=` — a `JobPage`; `state` may repeat or be comma-separated.
- `/api/v1/sessions?project=&job=&state=` — a `SessionPage`; `project` defaults to the
  served project.
- `/api/v1/sessions/{id}` — a `SessionDetail`: the exact `Session`, its latest capsule by
  `at_seq` (or null) and its parent lineage, nearest parent first, bounded at 32 entries and
  stopped when a parent row is missing or a parent session repeats.
- `/api/v1/sessions/{id}/capsules/{capsuleId}` — the exact stored `Capsule` when it belongs
  to that session; an unknown session, unknown capsule or a capsule of another session is
  `404`.
- `/api/v1/sessions/{id}/events?after_seq=<n>&limit=<n>` — an `EventPage` of exact stored
  `EventEnvelope` objects, in sequence order; the default limit is 200 and the cap is 1000.
  A full page's `next` is the cursor for the next `after_seq`.
- `/api/v1/sessions/{id}/events/stream?after_seq=<n>` — a `text/event-stream` response with
  one `event: session` frame per stored `EventEnvelope`, in sequence order and with the exact
  stored JSON as the `data:` line. `after_seq` defaults to 0 and an invalid value is `400`;
  an unknown session is `404` before the stream starts. An `event: heartbeat` frame with
  `{"at":"<RFC 3339>"}` is sent every 15 seconds, including while no events are flowing.
  The log is polled every 200 ms in bounded batches (at most 256 events per poll), so a slow
  client applies HTTP backpressure instead of growing an unbounded queue. When the cursor can
  no longer be served (the log was compacted away, removed or truncated behind it), the stream
  sends one `event: resync` frame with `{"after_seq":<n>}` naming the newest readable cursor,
  or the client's own cursor when nothing is readable, and closes; the client may reconnect
  with that cursor. The stream is read-only and has no token, lease or poll fallback.
- `/api/v1/inbox?state=open|answered|dismissed` — an `InboxPage` over the served project;
  `state` defaults to `open`.
- `/api/v1/cost?by=day|job|role|provider|model&since=&until=` — the exact `vowp cost --json`
  document over the served project. `by` defaults to `job`; `since` and `until` accept RFC 3339
  or `YYYY-MM-DD` (UTC midnight), and `until` is inclusive.

The new wire documents are exported from `vowp_protocol::api` and `schema::all_schemas()`:
`Health`, `Board`, `BoardRun`, `BoardLimit`, `Page<T>`, `JobPage`, `SessionPage`,
`SessionDetail`, `EventPage`, `InboxPage`, `CostReport`, `CostRow`, `CostTotal`, `ApiError`
and `ApiErrorBody`. Failures use the same `ApiError` shape as `vowp --json` errors and map to
`400` for an invalid filter or id, `404` for an unknown session or capsule and `500` for a
store, config or I/O failure.

**No token in this slice.** Nothing writes and the listener is loopback-only, so there is
no privileged action to protect; do not add a placeholder auth layer. A token must land
with the first write endpoint or a non-loopback listener.

Still later: every write endpoint (job retry/park/cancel, session switch, inbox answer),
the reconnect UI on the pages that still poll, the cost dashboard page, nodes, the
capsule web view, the auth token, `vowp up` integration and the remaining browser pages.
There are no placeholder routes for any of them. The per-session SSE stream is implemented
and used by the session detail page; the board stream is implemented and used by the board
page.

Tests are hermetic: a temporary `NodePaths`, a real `127.0.0.1:0` listener, fake clocks,
no real home, harness, tmux server or network beyond loopback. Every returned document is
validated against its exported JSON Schema, not just round-tripped through serde.

## Goal

Serve the data the CLI already reads, over a loopback HTTP API, so the web window, the phone board,
the cost dashboard and third-party tools do not have to shell out to `vowp`:

- `GET /api/v1/health` — version, node name, project, schema version, supervisor heartbeat.
- `GET /api/v1/jobs?state=&kind=&role=` — `Job` objects.
- `GET /api/v1/jobs/{id}` — one `Job`; `/runs` and `/gates` as sub-resources.
- `GET /api/v1/sessions?project=&job=&state=` — `Session` objects.
- `GET /api/v1/sessions/{id}` — metadata, latest capsule and lineage.
- `GET /api/v1/sessions/{id}/events?after_seq=<n>&limit=<n>` — exact `EventEnvelope` lines.
- `GET /api/v1/sessions/{id}/events/stream?after_seq=<n>` — SSE frames, one event per `seq`.
- `GET /api/v1/board` — the JSON shape already printed by `vowp board --json` (`at`, `project`,
  `jobs`, `running`, `slots_busy`, `slots_total`, `planner`, `keeper`, `limits`,
  `cost_today_usd`, `inbox_open`, `last_checkpoint`).
- `GET /api/v1/board/stream` — SSE `board` frames carrying the exact `Board` document when it
  changes, plus `heartbeat` frames; no cursor or resume.
- `GET /api/v1/inbox` and `POST /api/v1/inbox/{id}/answer` — read and answer; the read route
  is implemented in this slice and the answer write is later.
- `GET /api/v1/cost?by=day|job|role|provider|model&since=&until=` — the `vowp cost --json` shape;
  implemented in this slice.
- `GET /api/v1/nodes` — **later slice**: node name, SSH target, last seen, version and probe state.
  The web-window `/nodes` page needs it, but the window's first slice has no nodes page.
- `GET /api/v1/sessions/{id}/capsules/{capsuleId}` — one capsule belonging to the session;
  implemented in this slice. The window's capsule view remains later.
- `POST /api/v1/jobs/{id}/retry|park|cancel` — enqueue the existing
  `vowp_core::requests::Request`; do not duplicate the state machine.
- `POST /api/v1/sessions/{id}/switch` — run the same `SessionOps::switch` path as
  `vowp session switch`. The v0.1 request queue has no switch variant, so this is the one write that
  needs its own daemon-side call and cancellation story.

The node list is a **later slice**, not part of the first API slice: the web-window first slice is
a static shell and read-only board page with no nodes page, so it does not call it. Do not add a
placeholder route for it. The single-capsule route is implemented read-only in this slice; the
capsule web view is later.

A write response returns the affected row or the request id; consumers poll or wait for the next
event. A browser action must never apply a state transition directly.

## Wire contract

`EventEnvelope` is already frozen for v0.1 (`docs/design/architecture.md` §8): v0.3 may add optional
fields and new event types, but readers ignore unknown **fields**; unknown event **types** are a hard
error in v0.1 and should be tolerated as opaque in v0.3. The API sends the envelope as-is rather than
wrapping the payload, so a stored log line and an SSE `data:` value are the same object.

`Job`, `Session`, `Run`, `GateResult`, `Switch`, `Capsule`, `Limit`, `Cost` and `LedgerEntry` are
served in their `vowp-protocol` JSON form. The new document and transport types are:

- `Health { version, node, project, schema_version, heartbeat }` — the `/api/v1/health` document.
  `version` is the binary version, `node` and `project` are names, `schema_version` is the integer
  store schema, and `heartbeat` is the RFC 3339 time of the last supervisor tick or null when none
  has run.
- `Board { at, project, jobs, running, slots_busy, slots_total, planner, keeper, limits,
  cost_today_usd, inbox_open, last_checkpoint }` — the exact `vowp board --json` document. `jobs`
  maps a canonical state name to a count; `running` is a `BoardRun[]`; `limits` is a `BoardLimit[]`;
  `planner` and `keeper` are executors or null; `cost_today_usd` is null when today's cost is
  unknown; `last_checkpoint` is an RFC 3339 time or null.
- `BoardRun { job, executor, age_secs, last_event_age_secs }` — one running job in `Board.running`.
  `executor` and `last_event_age_secs` are nullable.
- `BoardLimit { provider, kind, resets_at }` — the reduced limit row in `Board.limits`; `kind` is
  the canonical limit class (`quota`, `credits`, `rate`, `context`, `auth` or `unavailable`) and
  `resets_at` is an RFC 3339 time or null.
- `Page<T> { items, next, total }` for lists.
- `SessionDetail { session, latest_capsule, lineage }` — the `/api/v1/sessions/{id}` document.
  `session` is the exact `Session`, `latest_capsule` is the capsule with the highest `at_seq`
  (or null when the session has none), and `lineage` is the parent-session chain, nearest
  parent first, at most 32 entries, stopped when a parent row is missing or a parent session
  repeats, or when the bound is hit.
- `InboxPage` = `Page<InboxItem>` — one inbox list page.
- `CostReport { by, rows, total }` — the exact `vowp cost --json` document. `by` echoes the
  grouping; `rows` and `total` split known USD, token usage, source and unknown-entry counts.
- `CostRow { key, runs, usd, tokens, source, unknown_entries }` — one grouped cost row.
  `usd` is null when every entry is unknown, and `source` is `receipt`, `price-table`,
  `unknown` or `mixed`.
- `CostTotal { runs, usd, tokens, source, unknown_entries }` — the totals row.
- `ApiError { error: { kind, message, hint? } }` — the same shape as `vowp --json` errors.
- An SSE frame with `event: session` / `event: board` / `event: heartbeat` / `event: resync`
  and the JSON data line. This is a `text/event-stream` frame, not a JSON document, so it has
  no schema of its own: the `data:` line is an exact `EventEnvelope` for `session`, the exact
  `Board` document for `board`, `{ at }` for `heartbeat`, and `{ after_seq }` for the one
  recovery `resync` frame. The board stream uses `board` and `heartbeat` only; it has no
  `resync`.

`vowp schema` remains the source of truth; add `Health`, `Board`, `BoardRun`, `BoardLimit`, `Page`,
`SessionDetail` and `ApiError` to `all_schemas()` rather than hand-writing JSON examples. Export the
concrete page instantiations the API serves (`JobPage`, `SessionPage`, `EventPage` and `InboxPage`)
and `CostReport` alongside their item types, and keep the SSE frame as the one explicit schema
exception above.

## Existing crates it builds on

- `vowp-store` — `Store`, `JobFilter`, `SessionFilter`, `event_rows`, `cost_totals`, `list_inbox`.
- `vowp-core` — `Deps`, `BoardSnapshot::collect`, `requests::Request` and the ledger query.
- `vowp-protocol` — every served type and `schema::all_schemas`.
- `vowp-cli` — keep as the first client; its `--json` shapes are the reference for the API.
- New `vowp-daemon` crate proposed: `axum` + `tokio` live here, not in `vowp-core`.

## Non-goals

- A public listener, TLS termination, a hosted relay or remote access without SSH.
- Multi-user identity, quotas or per-user permissions (v0.3).
- A second job/session state machine. The request queue and the supervisor remain the writer.
- Replacing `vowp status`, `vowp board`, `vowp session tail` or the tmux windows.

## Open questions

1. **Process shape.** Is the daemon a separate planned `serve` process, or an optional thread inside
   `vowp up`? A separate process keeps the supervisor unchanged but needs a second lock/ownership
   story; an in-supervisor thread reuses the heartbeat and `BoardSnapshot`.
2. **Scope.** One project per daemon (matching one supervisor per project home), or every project in
   the node database? Default to one project and make it explicit in the URL or a `--project` flag.
3. **Cursor semantics.** Resolved for the board stream: the board is a self-contained
   snapshot, so `GET /api/v1/board/stream` sends the exact `Board` document as a `board`
   frame whenever its content changes apart from `at`, with no cursor, `after_seq`, event
   merge or `resync`. `session_events.seq` remains per session for the timeline stream and
   the ledger has a global autoincrement id; no global event log is introduced.
4. **Token storage.** A random token in `<home>/api-token` (0600) or an environment variable? How is it
   rotated? Never log the value; never put it in a URL.
5. **Backpressure.** Answered for both streams. Each session poll reads at most 256 events,
   the response stream yields one frame at a time and lets HTTP backpressure slow the client,
   and a cursor that falls behind a truncated log gets one `resync` frame and a clean close.
   The board stream collects one snapshot per poll, holds no store between polls and sends a
   frame only when the snapshot changes, so a slow client only ever waits for the next
   snapshot.

## First-issue-sized slice

Implemented: `crates/vowp-daemon` opens an existing node home read-only and serves
`GET /api/v1/health`, `GET /api/v1/board`, `GET /api/v1/board/stream`, `GET /api/v1/jobs`,
`GET /api/v1/sessions`,
`GET /api/v1/sessions/{id}`, `GET /api/v1/sessions/{id}/capsules/{capsuleId}`,
`GET /api/v1/sessions/{id}/events?after_seq=`,
`GET /api/v1/sessions/{id}/events/stream?after_seq=`, `GET /api/v1/inbox?state=` and
`GET /api/v1/cost?by=&since=&until=`. It binds `127.0.0.1:0` in tests, uses a temporary store
with jobs in several states, one running job and a parent/child session chain with a capsule
and a bounded long chain, a limit, open/answered inbox items, cost ledger rows from every
source and a checkpoint, and asserts that every JSON document validates against its exported
schema — including `SessionDetail` for the session detail, `Capsule` for the capsule read,
`Board`, `BoardRun` and `BoardLimit` for the board, `InboxPage` for the inbox and `CostReport`
for cost. The SSE tests use a real loopback listener and short injected intervals: the
session stream compares exact `EventEnvelope` frames and appends live through
`SessionLog::open`; the board stream validates its `board` frames against the `Board` schema,
compares them with `GET /api/v1/board` apart from `at`, changes a job row through a writable
store to receive the next snapshot, and observes heartbeats while the snapshot is stable. No
write endpoints yet. The first-slice section above lists what is still later.

Tests are hermetic: temporary `NodePaths`, a fake clock, no network beyond loopback, no real
harness and no tmux.
