# v0.3 — web / PWA window

Status: moved from `docs/design/v0.2/` by the v0.2 spec; planned for v0.3, not v0.2. This note is
design only. v0.1 has no browser UI and no listening socket.

## Goal

A small installable web app served by the local daemon, usable from the same laptop or over an SSH
tunnel. It should answer the questions an operator asks from a browser without opening a terminal:

- **Nodes** — name, SSH target, last seen, version and probe state.
- **Sessions** — filter by project, job, role and state; show executor, last event and last capsule.
- **Session view** — an event timeline with live updates, lineage, a capsule panel, and an **executor
  switcher** that sends the same switch request as `vowp session switch`.
- **Capsule view** — the latest capsule rendered as Markdown, with its `at_seq`, trigger and executor.
- **Board** — job counts, running jobs, slot occupancy, limits, today's cost and open inbox count.

The board uses the same JSON shape as `vowp board --json`; the session list and event timeline use
the API from [`http-api.md`](http-api.md). The app is read-only in the first slice. When writes are
added, answering an inbox item is the only one that uses the existing request queue, and switching an
executor runs the same `SessionOps::switch` path as the CLI (the queue has no switch variant).

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
  `/nodes`, `/cost`, `/inbox`.
- PWA manifest and a small service worker; cache the shell, never cache event data as authoritative.
- The token is kept in memory or `sessionStorage`, not in a URL. A 401 shows a one-field unlock page.
- SSE with `after_seq` resume and a visible "reconnecting" state; fall back to polling if SSE fails.
  The implemented board and session slices are polling only, every five seconds.

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
`text/html`. It fetches `GET /api/v1/board` on load and every five seconds, and renders the
project, job counts by state, running jobs (job, executor, age, last event age), slots busy/total,
planner and keeper, active limits with reset times, today's cost (`unknown` when the API reports
null), the open inbox count, the last checkpoint and a visible "last updated" time. A failed fetch
shows an error line and keeps the last board and its timestamp visible. The page uses text labels
as well as colour and carries a `<noscript>` message. It links to `/sessions`, `/cost` and
`/inbox`.

The session pages are the same kind of embedded static shell,
`crates/vowp-daemon/web/sessions.html`, served at `GET /sessions` and `GET /sessions/{id}`. The
shell reads its own path. On `/sessions` it fetches `GET /api/v1/sessions` and renders a card
list with id, title, project, job, role, state, executor, last seq and updated time; each row
links to `/sessions/{id}`. On `/sessions/{id}` it fetches the session list to find the metadata
and `GET /api/v1/sessions/{id}/events?after_seq=<n>&limit=200`, then renders the metadata and an
event timeline of seq, time, type, actor and a compact summary. It polls every five seconds with
`after_seq` set to the last rendered sequence, appends only newer events and keeps the last data
and its "last updated" time on failure. A 404 or a failed fetch shows a visible error or
not-found line, and the page carries
`<noscript>The session view requires JavaScript.</noscript>`. It links back to the board and to
cost and inbox. The timeline never prints a token, home path or provider secret.

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
renders id, kind, job, title, age, state, options and a short body preview. A failed fetch shows an
error line and keeps the last list and timestamp visible; an empty list shows "No inbox items."
There is no answer, dismiss or other write action. The page links to the other pages, carries
`<noscript>The inbox page requires JavaScript.</noscript>`, and never prints a home path, token or
provider secret.

There is no Node or TypeScript build step, no PWA manifest, no service worker and no browser test:
`crates/vowp-daemon/tests/board_page.rs`, `session_page.rs` and `cost_inbox_pages.rs` are HTTP-only
tests over a temporary home and a real `127.0.0.1:0` listener.

Still later: the capsule view, `/nodes`, inbox answering, the cost dashboard and charts, SSE live
updates in the window pages, executor switching, write actions, PWA installability and the auth
token. The per-session SSE endpoint exists, but the pages above poll. The API routes and their
fallback are otherwise unchanged; unknown non-API paths still return the API's JSON 404.

The app must never print a token value, a home path or a provider secret; the board only contains the
fields already shown by `vowp board`, and the session pages only the fields already shown by
`vowp session show` and `vowp session tail`.
