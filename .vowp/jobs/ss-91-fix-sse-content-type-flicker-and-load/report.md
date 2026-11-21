---
job: ss-91-fix-sse-content-type-flicker-and-load
status: done
branch: job/ss-91-fix-sse-content-type-flicker-and-load
commits:
  - 63a6b10
  - 84b4c10
tests:
  - node scripts/fetch-engine.mjs (pinned sidecar present and verified)
  - scripts/cargo-gate.sh fmt --all -- --check (clean)
  - scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings (clean)
  - scripts/cargo-gate.sh test -p spirit-core (407 passed; chat suite 14, incl. new non_sse_event_endpoint_stays_ready_without_reconnect_flicker)
  - npm run test -w @stackspirit/ui (36 files, 375 tests passed, incl. new serverLoad.test.tsx)
  - npm run typecheck -w @stackspirit/ui; npm run lint -w @stackspirit/ui (clean)
files_changed:
  - crates/spirit-core/src/engine/chat/local.rs
  - crates/spirit-core/tests/chat.rs
  - packages/spirit-ui/src/next/health.tsx
  - packages/spirit-ui/src/next/Sidebar.tsx
  - packages/spirit-ui/src/test/next/serverLoad.test.tsx
---

# ss-91 report

## Bug 1 (SSE flicker)
- `sse_headers()` now returns `SseHead::{Stream,Unsupported}`; a 2xx response without
  `Content-Type: text/event-stream` (case-insensitive) is `Unsupported`, not `Connected`.
- `run_pump()` on `SseSignal::Unsupported` calls `on_sse_connected` once (session attached
  and left `Ready`, history reloaded) and returns: no reconnect loop and no
  `Connecting`/`Error` flicker.
- Test: fake `/event` returns `200 text/html`; asserts exactly one connection after 1.5 s,
  no `Connecting`/`Error` session events, and the session stays `Ready`.

## Bug 2 (load display)
- `cpuDisplay()` in `next/health.tsx` is built from the host sample, not the zero-filled
  `capacity.cpu`. No sample or zero cores gives `—` and no bar. Otherwise the text is
  `load 2.1 / 8 cores`, plus `· N% CPU pressure` when reported. `load1 > cpu_count` keeps
  the real figures; only the bar clamps. `cpuOver` exposes that case.
- `useServerHealth()` now takes disk free from the host sample only, because
  `capacity.disk.free_home_bytes` is `0` before a sample arrives; missing data shows
  `unknown`/`disk unknown` instead of the false `0 B free`.
- Server page and right-side server details use `cpuText`/`ramText`/`diskText` directly.
  The sidebar keeps its one compact Load number (design handoff), but shows `100%+` when
  the CPU load average is above the core count, and its title/accessible name carry the
  real `load 10.3 / 8 cores` (or RAM used/total) instead of only the clamped percent.
- Tests: `packages/spirit-ui/src/test/next/serverLoad.test.tsx` covers missing host sample
  (CPU/RAM/disk), normal load, CPU pressure, high load (`load1 > cpu_count`) and zero cores.
- Not changed: `capacity.rs` still zero-fills `CpuView`/`DiskView` without a sample; the
  Next UI no longer reads those zero-filled fields for health.

## Gate evidence
- `scripts/cargo-gate.sh fmt --all -- --check`: pass.
- `scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings`: pass
  (spirit-core, spirit-cli and spirit-desktop).
- `scripts/cargo-gate.sh test -p spirit-core`: 407 tests pass, 0 failed.
- `npm run test -w @stackspirit/ui`: 375 tests pass, 0 failed.
- `npm run typecheck -w @stackspirit/ui` and `npm run lint -w @stackspirit/ui`: pass.

## Notes
- The brief had no `owns` list; edits touched only the files above.
- `npm ci` was run to run the UI gates (node_modules is gitignored).
