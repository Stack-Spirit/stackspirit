# v0.3 — web / PWA window

Status: moved from `docs/design/v0.2/` by the v0.2 spec; planned for v0.3, not v0.2. The static
board, session, capsule, cost and inbox pages and the token-gated inbox answer are implemented;
the rest of this note is design. v0.1 has no browser UI and no listening socket.

## Goal

A small installable web app served by the local daemon, usable from the same laptop or over an SSH
tunnel. It should answer the questions an operator asks from a browser without opening a terminal:

- **Nodes** — name, SSH target, last seen, version and probe state.
- **Sessions** — filter by project, job, role and state; show executor, last event and last capsule.
- **Session view** — an event timeline with live updates, lineage, a capsule panel, and an **executor
  switcher** that sends the same switch request as `vowp session switch`.
- **Capsule view** — the capsule's metadata and structured fields (`at_seq`, trigger, `from`
  executor, intent, constraints, decisions, state, files, gotchas and open questions).
- **Board** — job counts, running jobs, slot occupancy, limits, today's cost and open inbox count.

The board uses the same JSON shape as `vowp board --json`; the session list and event timeline use
the API from [`http-api.md`](http-api.md). Reads are token-free. Answering an inbox item is the one
browser write in this slice: it uses the existing request queue behind the daemon bearer token.
Switching an executor remains later and, when added, runs the same `SessionOps::switch` path as the
CLI (the queue has no switch variant).

## Non-goals

- A code editor, a terminal emulator or a replacement for the tmux windows.
- Sending a new chat turn from the browser in the first slice. That needs a run queue, cancellation and
  a streaming path; design it after the read-only timeline works.
- Multi-user sessions, sharing links or a hosted URL.
- A server-side rendering stack or a Node process in production.

## Existing crates it builds on

- `vowp-daemon` and the endpoints in [`http-api.md`](http-api.md).
- `vowp-protocol` — `Session`, `EventEnvelope`, `Capsule`, `Switch`, `Job`, `Limit`, `Cost`.
- `vowp-core::ledger::BoardSnapshot` — the board snapshot and its state names.
- The `vowp session switch` and `vowp inbox answer` command semantics; the web action must produce
  the same switch call or request and the same ledger rows.

## Shape

- TypeScript, built to static assets and served by the daemon; no runtime Node dependency. The
  implemented first slice deliberately skips the build step: it is one hand-written
  `index.html` with inline CSS and vanilla JavaScript.
- Routes: `/` (board), `/sessions`, `/sessions/:id`, `/sessions/:id/capsules/:capsuleId`,
  `/nodes`, `/cost`, `/inbox`. The first slice implements `/`, `/sessions`, `/sessions/:id`,
  `/sessions/:id/capsules/:capsuleId`, `/cost` and `/inbox`; `/nodes` is later.
- PWA manifest and a small service worker; cache the shell, never cache event data as authoritative.
- The write token is kept in `sessionStorage`, never in a URL. The inbox page shows a one-field
  unlock panel when no token is stored; a `401` clears it and shows the panel again. Rotating the
  token means restarting the daemon with a new `VOWP_API_TOKEN`, then unlocking again.
- SSE with `after_seq` resume and a visible "reconnecting" state; fall back to polling if SSE fails.
  The session timeline uses the per-session stream with `after_seq` resume. The board uses the
  snapshot `/api/v1/board/stream` with reconnect and polling fallback. The cost and inbox pages
  still poll every five seconds.

## Open questions

1. **Frontend stack.** Plain TypeScript + a small DOM helper is enough for the first slice and avoids a
   build-tool decision. Pick a framework only when the session timeline needs it.
2. **Asset embedding.** The first slice embeds one hand-written `web/index.html` with
   `include_str!`, so the binary carries the page and `cargo build` needs no asset step. Decide
   later whether a built `web/dist` is embedded at release time or served from disk in development.
3. **Write scope.** Is executor switching safe from a browser before a live-input/queue design exists?
   The switch protocol drains an active turn; the UI must show the drain and its `blocked` result.
4. **Mobile vs desktop.** The phone board is a separate note; decide whether it is the same app at a
   narrow width or a dedicated route with less data.

## First slice (implemented)

`vowp-daemon` serves a static, read-only board page at `GET /`. The page is
`crates/vowp-daemon/web/index.html`, embedded in the binary with `include_str!` and returned as
`text/html`. It fetches `GET /api/v1/board` on load and renders the project, job counts by
state, running jobs (job, executor, age, last event age), slots busy/total, planner and keeper,
active limits with reset times, today's cost (`unknown` when the API reports null), the open
inbox count, the last checkpoint and a visible "last updated" time. After that initial fetch it
opens `EventSource('/api/v1/board/stream')`. A `board` frame replaces the whole board document
and updates "last updated"; a `heartbeat` frame updates only the live line. On a stream error
or close the page shows a visible "Reconnecting..." state, retries with bounded exponential
backoff and keeps the five-second polling fallback until a stream open succeeds. If
`EventSource` is unavailable the page polls only. A failed fetch or stream keeps the last board
and its timestamp visible. The page uses text labels as well as colour and carries a
`<noscript>` message. It links to `/sessions`, `/cost` and `/inbox`.

The session pages are the same kind of embedded static shell,
`crates/vowp-daemon/web/sessions.html`, served at `GET /sessions` and `GET /sessions/{id}`. The
shell reads its own path. On `/sessions` it fetches `GET /api/v1/sessions` and renders a card
list with id, title, project, job, role, state, executor, last seq and updated time; each row
links to `/sessions/{id}`. On `/sessions/{id}` it fetches the session list to find the metadata
and `GET /api/v1/sessions/{id}/events?after_seq=<n>&limit=200`, renders the metadata and the
event timeline of seq, time, type, actor and a compact summary, then opens
`EventSource('/api/v1/sessions/{id}/events/stream?after_seq=<last rendered seq>')`. It appends
`session` frames in sequence order and ignores sequences already on screen, so a reconnect does
not duplicate a row. `heartbeat` frames update the visible "live updates / last updated" line
and are never rendered as timeline rows. On a stream error or close the page shows a visible
"Reconnecting..." state, starts its five-second polling fallback at `after_seq=<last>`, and
retries the stream with bounded exponential backoff; a successful stream open stops the
fallback. A `resync` frame moves the resume cursor to the readable cursor it names. If
`EventSource` is unavailable the page says so and polls only. A failed fetch or stream keeps the
last rendered events and timestamps visible, and a 404 or failed fetch shows a visible error or
not-found line. On `/sessions/{id}` the page also fetches `GET /api/v1/sessions/{id}` for the
session-detail document. When the session has a capsule, it renders the capsule id, `at_seq`,
trigger, `from` executor, intent, constraints, decisions (`what`, `why`, `at_seq`), state (done,
in progress, blocked, next steps), files (path and action), gotchas and open questions in a
read-only panel with no edit control, and the panel's "View capsule" link opens
`/sessions/{id}/capsules/{capsule_id}`. When the latest capsule is null the panel says "No capsule
yet." The page renders the parent lineage returned by the API as a compact chain of links to
`/sessions/{id}`, using the session-list titles when they are available and the id otherwise, and
says "No parent sessions." when the chain is empty. The page carries
`<noscript>The session view requires JavaScript.</noscript>`, links back to the board and to
cost and inbox, and never prints a token, home path or provider secret.

The capsule page is `crates/vowp-daemon/web/capsule.html`, served at
`GET /sessions/{id}/capsules/{capsule_id}`. The shell reads its own path and fetches
`GET /api/v1/sessions/{id}/capsules/{capsuleId}`. It renders the capsule id, session id (linked to
`/sessions/{id}`), created time, `at_seq`, trigger, the `from` executor with its harness, model and
profile when present, intent, constraints, decisions (`what`, `why`, `at_seq`), state (done, in
progress, blocked, next steps), files (path and action), gotchas, open questions and one compact
telemetry line. The page is read-only: it has no Markdown parser, capsule editor or write control.
An unknown session or capsule is the API's `404`, which the page shows as a visible "Capsule not
found." line while keeping its back link; any other failed fetch shows an error line and still
leaves the static shell loaded. The page links to the board, the session, cost and inbox, carries
`<noscript>The capsule page requires JavaScript.</noscript>`, and never prints a home path, auth
token or provider secret. Markdown rendering, capsule editing and capsule writes remain out of
scope.

The cost page is `crates/vowp-daemon/web/cost.html`, served at `GET /cost`. It fetches
`GET /api/v1/cost?by=<group>` on load and every five seconds. The selector offers job, day, role,
provider and model (default job), and the URL keeps the chosen value. The page renders the grouped
rows and the total with key, runs, USD, token usage, source and unknown-entry count. A null USD
total is shown as `?` and never as zero, and the page states that unknown costs stay unknown. A
failed fetch shows an error line and keeps the last report and timestamp visible; an empty report
shows "No cost entries." There are no charts and no price-table editing. The page links to the
board, sessions and inbox, carries `<noscript>The cost page requires JavaScript.</noscript>`, and
never prints a home path, token or provider secret.

The inbox page is `crates/vowp-daemon/web/inbox.html`, served at `GET /inbox`. It fetches
`GET /api/v1/inbox?state=open` on load and every five seconds. The state selector adds all,
answered and dismissed; "all" fetches the three API states and merges their lists. The page
renders id, kind, job, title, age, state, options, a short body preview and, for an open item, an
answer form. The form offers a select of the item's options plus a free-text input, or only a
text input when the item has no options; the Answer button posts the chosen or typed text to
`POST /api/v1/inbox/{id}/answer` as JSON. A token unlock panel stores the `VOWP_API_TOKEN` value
in `sessionStorage` (never the URL) and hides once stored. A `401` clears the stored value,
shows the unlock panel and leaves the list visible; `503` shows the writes-disabled message;
`202` shows "Answer queued; the supervisor applies it on the next tick" and refreshes the list;
any other error shows a line and keeps the list. Reads continue without a token. A failed fetch
shows an error line and keeps the last list and timestamp visible; an empty list shows "No inbox
items." There is no dismiss or other write action. The page links to the other pages, carries
`<noscript>The inbox page requires JavaScript.</noscript>`, and never prints a home path, token
value or provider secret.

There is no Node or TypeScript build step, no PWA manifest, no service worker and no browser test:
`crates/vowp-daemon/tests/board_page.rs`, `session_page.rs`, `capsule_page.rs` and
`cost_inbox_pages.rs` are HTTP-only tests over a temporary home and a real `127.0.0.1:0` listener,
and `tests/sse.rs` reads real chunked stream bodies. The board-page test pins the stream path, the
`board` and `heartbeat` handlers, the "Reconnecting..." text and the polling fallback without
running a browser. The session-page test pins the stream path, the `after_seq` resume, the
`session` and `heartbeat` handlers, the reconnect text and the polling fallback. It also pins the
session-detail path, the capsule field labels, the lineage links, the "View capsule" link and the
no-capsule branch, and fetches `/api/v1/sessions/{id}` in the same test to prove the panel's data
source. The capsule-page test pins the capsule API path, the metadata and section labels, the
session back link, the `<noscript>` fallback and the absence of paths or secrets, fetches
`/api/v1/sessions/{id}/capsules/{capsuleId}` in the same test to prove the page's data source, and
checks that an unknown session or capsule is the API's `404` behind the page's not-found text. The
inbox-page test pins the answer form, the `Authorization: Bearer` header, the `sessionStorage`
unlock, the queued message, the `401` and `503` lines, and the absence of a token in the URL; the
answer API test proves the write itself.

Still later: `/nodes`, the cost dashboard and charts, the reconnect UI on the pages that still
poll, executor switching, dismiss and the other write actions, PWA installability and a token
rotation service. Markdown rendering, capsule editing and capsule writes remain out of scope.
The API routes and their fallback are otherwise unchanged; unknown non-API paths still return the
API's JSON 404.

The app must never print a token value, a home path or a provider secret; the board only contains the
fields already shown by `vowp board`, and the session pages only the fields already shown by
`vowp session show` and `vowp session tail`.
