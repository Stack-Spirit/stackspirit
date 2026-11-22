---
job: ss-107-live-capacity-and-clean-auth-store
status: done
branch: job/ss-107-live-capacity-and-clean-auth-store
commits:
  - 110b31a
tests: "cargo clippy -D warnings (spirit-core, spirit-desktop) clean; cargo test spirit-core + spirit-desktop pass; ui typecheck, lint, vitest (401) pass; Playwright e2e NOT run"
---

## Done
- **Capacity**: `connect_host` now calls `refresh_capacity` best-effort after connecting and spawns a 20s capacity loop; `fleet_refresh` also refreshes capacity. Failures are ignored, so older hosts stay connected. The poll loop reuses the last capacity report, so values persist between polls.
- **Auth store**: new `PrivateKeyStore` (`keychain.rs`): atomic temp 0600 -> rename JSON file, 0700 parent, Zeroizing buffers, errors carry no values. `AppState::new` uses it at `engine/state/provider-keys.json`. `OsKeychain` is kept for opt-in.
- **Settings badge**: `defaultModelLabel` shows `Default · <model>` (explicit selection, else first free model, else first model); `No model` only when no models exist.
- Tests: 2 Rust tests for the private store (round trip, permissions, atomicity, not-found, no secrets in errors); 2 vitest tests for the badge.

## Caveats
- Keys previously saved in the macOS Keychain are not migrated (reading them would prompt); users re-enter them once.
- No new Rust test for the desktop capacity wiring (existing core tests cover `refresh_capacity`); no new Playwright e2e/axe specs were written.
- `refresh_capacity` emits an Error event on failure; I did not check how the UI shows that on older hosts.
