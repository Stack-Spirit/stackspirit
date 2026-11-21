---
job: ss-87-searchable-model-picker-and-manage-models
status: done
branch: job/ss-87-searchable-model-picker-and-manage-models
commits:
  - 3c74388
  - 1bac3c0
tests:
  typecheck: pass
  lint: pass
  unit (npm test): pass (ui 364, desktop 10, web 73)
  e2e (npm run test:e2e -w @stackspirit/desktop): pass (39/39)
files_changed:
  - packages/spirit-ui/src/next/ComposerBar.tsx
  - packages/spirit-ui/src/next/theme.css
  - packages/spirit-ui/src/bridge/MockBridge.ts
  - packages/spirit-ui/src/test/next/composer.test.tsx
  - packages/spirit-ui/src/test/next/providerSettings.test.tsx
  - packages/spirit-ui/src/test/next/retryRecovery.test.tsx
  - apps/spirit-desktop/e2e/meadow-provider-connect.spec.ts
  - apps/spirit-desktop/e2e/next-shell.spec.ts
  - apps/spirit-desktop/e2e/meadow-chat-retry.spec.ts
  - docs/HANDOFF-DESKTOP-UI.md
  - docs/ORCHESTRATOR-CHAT.md
open_items:
  - "owns_respected needs apps/spirit-desktop/e2e/meadow-chat-retry.spec.ts added to this job's owns; it is the only changed path outside the original brief and was a required selector migration."
---

# ss-87 searchable model picker and Manage models

## What landed

- `ComposerBar.tsx`: the picker is now a `role="dialog"` popover (`model-picker-popover`) with an auto-focused `type="search"` field (`composer-model-search`), a bounded `listbox` of `option` rows, and a sticky footer. Search is case-insensitive and multi-word across label, id, provider id and provider label. `ArrowUp`/`ArrowDown` wrap, `Enter` selects, `Escape` closes and refocuses `composer-model`. The empty state reads `No models match “<query>”.`.
- `groupChatModels` returns OpenCode Free first (`Free` badge), then one section per provider (popular-first via a TS copy of `POPULAR_PROVIDER_IDS`, then alphabetical). Each connected section has a `Connected` badge and a model count. Rows show a `✓` on the active model, the clean label with no repeated `BYOK ·` prefix, a secondary id such as `anthropic/claude-sonnet-4`, and a badge.
- **Manage models** (`composer-manage-models`) and **Connect another provider…** (`composer-connect-provider`) both close the picker and open the in-place `Engine & Models` modal (`Modal` + `SettingsPage`). `onOpenEngineSettings` is not used, so the owner stays in the chat; unit tests assert that behavior.
- `theme.css`: Meadow-token styling for the popover, border-step search focus, bounded scroll area, sticky search header/footer, provider headings, rows, badges and empty state. The existing light/dark tokens are used unchanged.
- `MockBridge.ts`: connecting OpenRouter, Anthropic or OpenAI in preview mode unlocks realistic multi-model catalogs.
- Tests: composer, providerSettings, retryRecovery, meadow-provider-connect, next-shell and the retry-notice e2e. Playwright axe WCAG 2.2 AA passes in light and dark for the open picker, a filtered list, the empty-search state and the in-place `Engine & Models` panel.
- Docs: `HANDOFF-DESKTOP-UI.md` and `ORCHESTRATOR-CHAT.md` describe the landed picker and record the 39/39 Meadow suite.

## Ownership note

`apps/spirit-desktop/e2e/meadow-chat-retry.spec.ts` is the only changed path outside the brief's original `owns` list. It asserted the retired contract (`role="menu"` / `menuitemradio`, `BYOK ·` row names and focus staying on the trigger), so its two light/dark tests failed the e2e gate after the brief-mandated dialog/listbox and auto-focused-search semantics landed. Commit `1bac3c0` migrates those selectors to `model-picker-popover`, `composer-model-search`, the provider groups and the `option` role; no assertion was weakened. Please widen `owns` to include this file so `owns_respected` can pass. All other changed paths are inside `owns`.

## Gates

- `npm run typecheck` — pass (ui, desktop, web).
- `npm run lint` — pass (ui, desktop, web, zero warnings).
- `npm test` — pass (ui 364, desktop 10, web 73; 447 total).
- `npm run test:e2e -w @stackspirit/desktop` — pass (39/39, 3.7 m).
- `report`, `owns_respected`, `secret_scan` and `review` are supervisor-run meta gates. No secret, `.env` file, API key or token was read, printed, logged or committed.

## Notes

- `npm ci` was needed in this worktree before the gates.
- The preview connection flow and key verification stay mock-only; no real provider key is used in any test.
- Manage models uses the in-place `Engine & Models` modal rather than the shell's `onOpenEngineSettings` navigation, as allowed by the brief.
