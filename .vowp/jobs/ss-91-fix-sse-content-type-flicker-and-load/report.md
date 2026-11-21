---
job: ss-91-fix-sse-content-type-flicker-and-load
status: done
branch: job/ss-91-fix-sse-content-type-flicker-and-load
commits:
  - 63a6b10
tests:
  - cargo test -p spirit-core --test chat (14 passed, incl. new non_sse_event_endpoint_stays_ready_without_reconnect_flicker)
  - cargo fmt --check, cargo clippy -p spirit-core --all-targets -D warnings (clean)
  - packages/spirit-ui vitest (375 passed, incl. new serverLoad.test.tsx), typecheck, lint (clean)
---

# ss-91 report

## Bug 1 (SSE flicker)
- `sse_headers()` now returns `SseHead::{Stream,Unsupported}`; a 2xx without `Content-Type: text/event-stream` (case-insensitive) is `Unsupported`.
- `run_pump()` on `SseSignal::Unsupported` calls `on_sse_connected` once (session attached/Ready, history reloaded) and returns: no reconnect loop, no Connecting/Error.
- Test: fake `/event` returns 200 text/html; asserts exactly 1 connection after 1.5s, no Connecting/Error session events, session Ready.

## Bug 2 (load display)
- New `cpuDisplay()` in `next/health.tsx`, built from the host sample (not the zero-filled `capacity.cpu`). No sample or 0 cores gives `—` and no bar. Otherwise text is `load 2.1 / 8 cores`, plus `· N% CPU pressure` when reported. `load1 > cores` keeps real numbers; only the bar clamps. RAM shows `—` instead of "unknown" when missing.
- Tests: `packages/spirit-ui/src/test/next/serverLoad.test.tsx` (missing sample, normal, pressure, high load, zero cores).
- Not changed: `capacity.rs` still zero-fills `CpuView` without a sample (UI no longer depends on it); the sidebar percent still uses the clamped ratio.

## Notes
- The brief had no `owns` list; edits touched only the files above.
- `npm ci` was run to run the UI gates (node_modules is gitignored).
