---
job: ss-89-picker-clip-and-verify-feedback-fix-1
status: done
branch: job/ss-89-picker-clip-and-verify-feedback
commits:
  - 4dc9b0a
tests: "cargo fmt --check: clean; cargo clippy -p spirit-core --all-targets -D warnings: clean; cargo test -p spirit-core --lib openrouter_key: pass; npm run typecheck: clean; npm run lint: clean; e2e 'model picker search stays': passes with the fix, fails without it"
files_changed:
  - apps/spirit-desktop/e2e/next-shell.spec.ts
  - crates/spirit-core/src/engine/process.rs
  - packages/spirit-ui/src/next/ComposerBar.tsx
---
1. ComposerBar popover cap now anchors to the top of the closest .nx-chat (the real clip, below the saved-server banner) instead of the .nx-chat-head bottom.
2. E2E test rewritten: fresh scenario, 1024x480, docks the hero so the banner is pinned, asserts popover top >= .nx-chat top and banner bottom, and the search is not covered (elementFromPoint). Verified it fails (80px vs 145px) with the fix reverted.
3. Fake engine: paid/model-a (first ranked) now answers a model-specific 404, so the integration test proves the fallback; renamed to openrouter_key_verifies_after_a_model_specific_404. Free models rank last by design, so they are not probed; that ordering stays covered by the unit tests.
