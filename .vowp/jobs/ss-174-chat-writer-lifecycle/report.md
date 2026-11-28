---
job: ss-174-chat-writer-lifecycle
status: done
branch: job/ss-174-chat-writer-lifecycle
commits: ["226056e", "694f12a"]
tests: "all brief gates pass: fetch-engine, cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test -p spirit-core; also cargo check -p spirit-desktop, npm run typecheck, npm run lint, npm test -w @stackspirit/ui (582 passed) and Playwright chat-takeover.spec.ts (3 passed)"
questions_for_owner: []
---

# Phase 4: release, reconnection and lost-writer handling

## What landed

- `crates/spirit-core/src/engine/chat/writer.rs`
  - The 15 s watcher now also publishes terminal transitions through a core-only
    `WriterViewListener`; no lease identity crosses the callback.
  - `probe()` re-resolves the home endpoint with the surface id while held, so focus,
    resume and attach/reconnect pick up a restarted engine's new loopback URL/auth
    without reissuing the lease. A non-owner, stale or unverifiable result releases the
    child and returns the `lost` view.
  - The token path is unchanged: the grant token is zeroized/dropped, and the listener
    only sees `ChatWriterView`.
- `crates/spirit-core/src/engine/chat/local.rs`
  - The provider installs the writer listener and, on loss, flips the session read-only,
    names the observed owner, closes a streaming turn without appending its late answer,
    and re-reads the authoritative native log from the home endpoint.
  - Late SSE frames and a late background POST response are fenced for the abandoned
    turn; the next explicit attach/takeover clears the fence after the log re-read.
  - A send still requires `writer.state == "held"` **and** `access == "owner"`; the
    pre-send check runs before every turn.
  - An SSE disconnect re-resolves a held writer and restarts the pump on the new
    loopback endpoint when the lease is still ours; two failed verification attempts
    remain terminal.
- `apps/spirit-desktop/src-tauri/src/state.rs` and `lib.rs`
  - Window close (`CloseRequested`/`Destroyed`) releases tracked writers with the
    existing 3 s bounded, idempotent shutdown hook.
  - Window focus/resume re-verifies tracked writers with a 5 s bounded check.
  - Chat close and session switch release through the existing bridge command; the
    exit hook remains the app-quit path.
- `packages/spirit-ui/src/next/ChatView.tsx`
  - Unmount-only release for the current session (including a take-over still in flight).
    No new control or copy; the Phase 3 action and read-only notice are unchanged.
- New hermetic tests:
  - `crates/spirit-core/tests/chat_writer_lifecycle.rs`: watcher loss and child reap,
    mid-turn fencing plus home-log backfill, engine restart re-resolution without a new
    attach, and sleep/stale-loss/reissue with a new surface.
  - `apps/spirit-desktop/e2e/chat-takeover.spec.ts`: preview fake bridge only; session
    switch release, chat-view close release, and the `lost` read-only view. No real
    lease, VOWP home, token or engine.

## Gate results

All commands below passed in this worktree:

- `node scripts/fetch-engine.mjs` — sidecar pinned and checksum-verified.
- `scripts/cargo-gate.sh fmt --all -- --check` — clean.
- `scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings` — clean.
- `scripts/cargo-gate.sh test -p spirit-core` — all suites passed, including the new
  `chat_writer_lifecycle` (4 tests) and the existing `chat_writer` (25 tests).
  The lifecycle suite was rerun 8 times consecutively with no failures after making
  the sleep/resume check deterministic (the periodic watcher is kept out of that test
  so the focus probe is the first check after the simulated sleep).
- `scripts/cargo-gate.sh check -p spirit-desktop` — passed on this Linux host (the GTK
  and WebKit development libraries were available). The macOS bundle/signing check
  remains owner-only.
- `npm run typecheck` — UI, desktop and web clean.
- `npm run lint` — UI, desktop and web clean.
- `npm test -w @stackspirit/ui` — 63 files, 582 tests passed.
- `npx playwright test e2e/chat-takeover.spec.ts --project=chromium` — 3 passed.

## Owner-run `docs/SESSION-TAKEOVER.md` §7.5 checklist

**Not run.** These require a real co-located VOWP home, a live home-hosted OpenCode
orchestrator and a second writer surface. Record only placeholders; never a URL, port,
password, token, native id or real hostname. The owner must run this on the real home
and macOS build.

| Check | Result (owner fills in) | Notes |
|---|---|---|
| `vowp orchestrator attach` prints one grant and holds until EOF |  | Real CLI/DB contract |
| `vowp session lease <SESSION>` names the DesktopUi surface while held |  | Kind, holder, heartbeat |
| A second writer without `--takeover` is refused with the holder name |  | Conflict text/exit class |
| A second writer with `--takeover` replaces the lease; the first app flips read-only within one watch interval |  | 5 s drain / 15 s watch |
| Closing the app releases within seconds; `session lease` shows no owner |  | stdin EOF/release path |
| Kill the app hard; the lease is stale within 60 s and a new attach succeeds |  | Crash bound; no clean-release promise |
| Sleep past 60 s; the next check is read-only and a new attach gets a new token |  | Sleep/reissue rule |
| An OpenCode window keeps writing after the takeover |  | Known cooperative limit; proxy deferred |
| A node-hosted orchestrator returns typed `node_hosted` and no button |  | Today's Mac fallback |
| No token appears in the app log, a snapshot, the DOM or an error card |  | Use a debug wrapper around the real grant |

Owner-only steps beyond this checklist: macOS app bundle/signing, the real VOWP home
and a second real writer surface. This job's tests are hermetic fakes only.

## Honest limits

- Takeover remains cooperative. A direct OpenCode/HTTP writer is not stopped by this
  change; the loopback engine proxy is still the eventual enforcement point.
- The live node-hosted Local chat remains read-only with no Take over action. Phase 4
  does not add a node-side attach or SSH attach.
- Release is not Stop: a running turn may continue on the orchestrator after the lease
  is released. The UI copy was not changed.
- A force-quit can leave a stale lease for up to 60 s; the app does not promise an
  immediate release after a crash.
