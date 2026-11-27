## Summary

<!-- What does this change do, and why? Link the issue it fixes, if there is one. -->

## Tests run

<!-- Paste the exact commands and results. Mark the gates you ran and add any platform-specific ones. -->

- [ ] `npm run typecheck`
- [ ] `npm run lint`
- [ ] `npm test`
- [ ] `cargo test -p spirit-core`
- [ ] `node apps/site/scripts/verify.mjs` (site changes)
- [ ] `npm run test:e2e` (desktop or Web changes)
- [ ] `cd runtime && cargo test --workspace --no-fail-fast` (runtime changes)
- [ ] Other: <!-- command and result -->

## Docs updated

<!-- Which docs, README or --help text changed? Write "None needed" with a reason if not. -->

## Screenshots / recordings (UI changes)

<!-- Add before/after screenshots or a short recording. Redact tokens, keys, real
hostnames or IP addresses, and private paths. Delete this section for non-UI changes. -->

## Checklist

- [ ] The change is focused on one problem.
- [ ] I added or updated a test that covers the behaviour change, or explained why a test is not practical.
- [ ] I updated the documentation or `--help` text for user-visible changes.
- [ ] UI changes include before/after screenshots or a recording, with secrets and private data removed.
- [ ] I removed secrets, `.env` files, real hostnames or IP addresses, and private paths from the diff, logs and screenshots.
- [ ] I expect CI to pass.
- [ ] I did not force-push `main`; this work is on a topic branch.

By submitting this pull request I agree that my contribution is licensed under the MIT
License, with no CLA. Maintainers may rebase, edit or merge this branch while reviewing;
I do not need to keep it in sync with `main` myself.
