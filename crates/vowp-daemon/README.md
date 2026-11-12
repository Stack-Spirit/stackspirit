# vowp-daemon

The read-only loopback HTTP API for one VOWP project.

`vowp-daemon [--home <dir>] [--project <name>] [--port <n>]` opens an existing VOWP home
read-only, serves the static board page at `GET /` and its phone alias `GET /board`, the
static session list at `GET /sessions` and session detail at `GET /sessions/{id}`, and the
API endpoints `GET /api/v1/health`, `/api/v1/board`, `/api/v1/jobs`, `/api/v1/sessions` and
`/api/v1/sessions/{id}/events`, and binds `127.0.0.1` only. The default port is `0`, so
the operating system picks an ephemeral port and the binary prints the bound address.

The board and session pages are plain HTML, CSS and vanilla JavaScript embedded in the
binary with `include_str!`; the board fetches `/api/v1/board` on load and every five
seconds, and its card layout is usable at a 360 px viewport. `/sessions` fetches
`/api/v1/sessions`; `/sessions/{id}` shows one session's metadata and a read-only event
timeline that polls `/api/v1/sessions/{id}/events?after_seq=` every five seconds and
appends only newer events. No Node or TypeScript build step is involved.

This is the first v0.3 slice: there is no token, no write endpoint and no SSE. The listener
is loopback-only and nothing writes, so there is no privileged action to authenticate. A
token must be added before a write endpoint or a non-loopback listener.

The API serves the same `vowp-protocol` objects the CLI prints. Spec:
[`docs/design/v0.3/http-api.md`](../../docs/design/v0.3/http-api.md).
