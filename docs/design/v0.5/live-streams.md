# v0.5 — live snapshot streams for the web window

Status: **implemented** for v0.5 Slices 1a and 1b. The routes and pages described here exist in
`crates/vowp-daemon`; the tests named at the end run against a temporary home and a real
loopback listener.

This note is the one place that describes the whole live-window contract. It supersedes the
earlier statements in [`v0.3/http-api.md`](../v0.3/http-api.md) and
[`v0.3/web-window.md`](../v0.3/web-window.md) that the cost, inbox and nodes pages only
polled. The design scope is in [`scope.md`](scope.md) §1a and §1b.

## Routes

Five route families, six snapshot endpoints. All are `GET`, loopback-only, token-free and
read-only; none has a cursor, `after_seq`, resume, write or per-row protocol.

| Route | Event | Payload |
|---|---|---|
| `GET /api/v1/board/stream` | `board` | the exact `Board` document |
| `GET /api/v1/jobs/stream?state=&kind=&role=` | `jobs` | the exact filtered `JobPage` |
| `GET /api/v1/jobs/{id}/stream` | `job` | a `JobDetailPage`: the exact `Job`, its `RunPage` and its newest-attempt `GateResultPage` |
| `GET /api/v1/cost/stream?by=&since=&until=` | `cost` | the exact `CostReport` |
| `GET /api/v1/inbox/stream?state=` | `inbox` | the exact `InboxPage` |
| `GET /api/v1/nodes/stream` | `nodes` | the exact `NodePage` |

The filters are the same ones the matching non-streaming read accepts. `by`, `since` and
`until` on the cost stream are the `GET /api/v1/cost` filters; `state` on the inbox stream is
the `GET /api/v1/inbox` filter and defaults to `open`. An invalid filter is the normal JSON
`400` `ApiError` before the stream starts. An invalid or unknown job id on the job-detail
stream is the same JSON `400` or `404` before streaming. The nodes stream has no filter.

Every `data:` line is the document itself, not a wrapper around it. `JobDetailPage` is a
transport-only composition in `crates/vowp-daemon/src/stream.rs`; it adds no exported protocol
schema. The pages read the same documents the CLI and the non-streaming endpoints produce.

## Common frame rules

- **First frame always.** The first successful poll sends one snapshot frame even when the
  page is empty, so a fresh stream is a full render rather than a delta.
- **Changed only.** Later polls send a frame only when the snapshot document differs from the
  last one sent. A stable document produces no duplicate frame. The board's wall-clock `at`
  is liveness, not content: it is ignored for the comparison and the heartbeat carries
  liveness instead. No other snapshot has a wall-clock field.
- **One snapshot per poll, never per row.** A `jobs`, `cost`, `inbox` or `nodes` frame is one
  complete page (`items` for the pages, `rows` and `total` for cost); a `job` frame is one
  complete job/runs/gates snapshot. A new row changes the next snapshot; it never produces its
  own event.
- **Heartbeat.** An `event: heartbeat` frame with `{"at":"<RFC 3339>"}` is sent every 15
  seconds while the connection is idle. A heartbeat never replaces the last snapshot.
- **Clean close on error.** A store or encoding failure ends the stream without an error
  frame. The page treats that as a disconnect: it shows a visible reconnect state, keeps the
  last rendered snapshot, starts its five-second polling fallback and retries the stream.
- **Bounds.** Snapshot polls run once per second on the same injected interval the board
  stream uses. Each poll opens a read-only store on `spawn_blocking`, collects exactly one
  snapshot and drops the store before the next await; no store handle is held between polls
  and no snapshot is queued per client. The stream yields one frame at a time and lets HTTP
  backpressure slow a slow client. The session-log timeline stream keeps its separate 200 ms
  poll and 256-event batch bound.
- **No caching.** The jobs, cost, inbox and nodes streams are sent with
  `Cache-Control: no-store`. The service worker bypasses every SSE request in any case.

The `/api/v1/cost`, `/api/v1/inbox` and `/api/v1/nodes` response shapes, the inbox answer
write and node semantics are unchanged. The streams are extra read-only transports.

## Pages

`/cost`, `/inbox` and `/nodes` keep their initial fetch, their five-second `refresh()`
fallback and their service-worker registration. Each then opens one `EventSource` for its
snapshot stream:

- **`/cost`** follows `/api/v1/cost/stream` with the selected `by` query. A `cost` frame
  replaces the table and the inline chart from the same exact `CostReport`. Changing the
  selector reopens the stream with the new query.
- **`/inbox`** follows `/api/v1/inbox/stream?state=<state>` for `open`, `answered` or
  `dismissed`. An `inbox` frame replaces the list. Changing the state selector reopens the
  stream with the new query. The answer `202` still triggers an immediate `refresh()`. The
  merged `all` view has no single API state, so it keeps its existing three-state polling
  fallback rather than opening a stream that cannot represent it.
- **`/nodes`** follows `/api/v1/nodes/stream`. A `nodes` frame replaces the table. The page
  stays read-only: no add, remove or probe control.

On a stream error or close each page shows "Reconnecting... polling every 5 seconds.", keeps
the last rendered report/list/table and timestamp visible, starts its five-second fallback and
retries with bounded exponential backoff: 1 second, doubling to a 15 second cap. A successful
open, snapshot frame or heartbeat resets the delay and stops the fallback. If `EventSource` is
absent (or cannot be constructed), the page polls only and says so on its visible live line.
All three pages remain usable at a 360 px viewport; no wide layout was added.

The board, `/jobs` and `/jobs/{id}` pages follow the same reconnect and fallback discipline
described above; Slice 1a landed those pages and this note records their contract unchanged.

## Service worker

`crates/vowp-daemon/web/sw.js` already bypasses `/api/` paths, any path ending in `/stream`
and any request whose `Accept` names `text/event-stream`. Those requests go to the network
only and are never cached. Slice 1b did not change `sw.js`; the page tests and
`crates/vowp-daemon/tests/pwa.rs` pin the bypass.

## Tests

The stream tests in [`crates/vowp-daemon/tests/sse.rs`](../../../crates/vowp-daemon/tests/sse.rs)
use a real chunked HTTP client and short injected intervals:

- Slice 1a: `board_stream_sends_initial_changed_snapshots_and_heartbeats`,
  `board_stream_closes_when_a_snapshot_cannot_be_collected`,
  `jobs_stream_sends_initial_changed_snapshots_and_no_store`,
  `job_stream_sends_snapshots_for_state_run_and_gate_changes` and
  `job_stream_rejects_unknown_jobs_and_bad_ids_before_streaming`.
- Slice 1b: `cost_stream_sends_one_initial_snapshot_then_changed_only_frames`,
  `inbox_stream_carries_the_state_filter_and_updates`,
  `nodes_stream_sends_initial_nodes_and_updates`,
  `live_streams_never_emit_a_per_row_frame` and
  `streams_end_cleanly_when_the_store_is_unavailable`.

The page tests in `crates/vowp-daemon/tests/cost_inbox_pages.rs` and
`crates/vowp-daemon/tests/nodes_page.rs` pin each stream path and event handler, the
heartbeat handler, the "Reconnecting" text, `REFRESH_MS = 5000`, the
`setInterval(refresh, REFRESH_MS)` fallback, the inbox selector reopening the stream, the
read-only nodes page, and the service-worker `/api/` and `/stream` bypass.
