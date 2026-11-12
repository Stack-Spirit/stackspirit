# vowp-daemon

The read-only loopback HTTP API for one VOWP project.

`vowp-daemon [--home <dir>] [--project <name>] [--port <n>]` opens an existing VOWP home
read-only, serves the static board page at `GET /` and its phone alias `GET /board`, the
static session list at `GET /sessions` and session detail at `GET /sessions/{id}`, and the
API endpoints `GET /api/v1/health`, `/api/v1/board`, `/api/v1/jobs`, `/api/v1/sessions`,
`/api/v1/sessions/{id}/events`, `/api/v1/sessions/{id}/events/stream`, `/api/v1/inbox` and
`/api/v1/cost`, and binds `127.0.0.1` only. The default port is `0`, so the operating system
picks an ephemeral port and the binary prints the bound address.

The board and session pages are plain HTML, CSS and vanilla JavaScript embedded in the
binary with `include_str!`; the board fetches `/api/v1/board` on load and every five
seconds, and its card layout is usable at a 360 px viewport. `/sessions` fetches
`/api/v1/sessions`; `/sessions/{id}` shows one session's metadata and a read-only event
timeline. The timeline first fetches `/api/v1/sessions/{id}/events?after_seq=`, then opens
`/api/v1/sessions/{id}/events/stream?after_seq=` and appends `session` frames while using
`heartbeat` frames only to refresh the live line. A stream error or close shows a visible
reconnecting state, starts the five-second polling fallback at the same cursor and retries
the stream with bounded backoff; a browser without `EventSource` polls only. No Node or
TypeScript build step is involved.

The per-session stream at `/api/v1/sessions/{id}/events/stream?after_seq=` sends one
`event: session` frame per stored `EventEnvelope`, a periodic `event: heartbeat` frame and a
single `event: resync` frame if the log was compacted away or truncated behind the cursor.
The session detail page uses it. The board, session-list, cost and inbox pages still poll
their JSON endpoints; the board/global SSE stream and the reconnect UI on those pages are
later.

This is the first v0.3 slice: there is no token and no write endpoint. The listener is
loopback-only and nothing writes, so there is no privileged action to authenticate. A token
must be added before a write endpoint or a non-loopback listener.

The API serves the same `vowp-protocol` objects the CLI prints. Spec:
[`docs/design/v0.3/http-api.md`](../../docs/design/v0.3/http-api.md).
