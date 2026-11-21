---
job: ss-90-deepseek-harness-chat-ui-and-spirit-loader
status: done
branch: job/ss-90-deepseek-harness-chat-ui-and-spirit-loader
commits:
  - 648fcdc
tests: ui vitest 370/370 pass; typecheck and lint clean (ui, desktop, web); Playwright meadow-chat-turns, meadow-chat-tools and next-shell 19/19 pass (light and dark, axe WCAG 2.2 AA)
---

# ss-90 report

- Removed the "StackSpirit" sender header above assistant replies (`ChatView.tsx`).
- Added an answer footer under each completed model/preview reply:
  - Copy button with "Copied" feedback.
  - Model and provider label, output tokens and context when the engine reports them, and time.
  - Retry on the newest reply of a settled session, which resends the last user message.
- Added `SpiritLoader.tsx`: pure SVG + CSS sprout built from `design/brand/mark/grid.json`.
  - Sprout grows, unfurls and sways; the eyes blink.
  - Static pose under `prefers-reduced-motion`.
  - Uses `role="status"` with a screen-reader label.
  - Shown at the tail of the transcript while the session is replying.
- `spiritSprite.ts` is generated from the grid (run-length SVG paths). I did not commit the generator script; it was a one-off in /tmp.
- Tests: `test/next/answerFooter.test.tsx` and `e2e/meadow-chat-turns.spec.ts`.

Notes:
- The footer's model label is the session's current model; the engine does not report a per-message model.
- The "Preview" tag moved from the removed header into the footer.
- The brief had no `owns` list, so I kept changes to the UI chat files, theme CSS and new tests.
- I did not run the Rust gates; no Rust files changed.
