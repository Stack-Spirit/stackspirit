---
job: ss-162-doc-link-check
status: done
branch: job/ss-162-doc-link-check
commits: ["25999b9"]
tests: "node scripts/check-doc-links.mjs: PASS 342 files; /tmp/opencode bogus-link fixture: exit 1 with 2 expected findings and no code-span/fence false positives; ci.yml YAML parse: PASS; bash runtime/docs/tests/run.sh: PASS; bash scripts/check-public-readiness.sh: PASS"
---

# Add a markdown relative-link check to CI

## Summary

Added a dependency-free Node link checker for every tracked `.md` file, wired it
into CI as the `docs-links` job, and fixed the one real stale link.

## Changes

- `scripts/check-doc-links.mjs` (new): runs `git ls-files -z '*.md'`, skips
  `.git`, `node_modules` and `target`, blanks fenced blocks and inline code
  spans while preserving line numbers, and reports every relative link or image
  target that does not resolve. Fragments and queries are stripped before
  resolution. Relative targets are resolved against the containing file's
  directory; schemes, `mailto:`, absolute paths and `#`-only links are skipped.
  Prints `file:line target` for failures and exits 1; prints a summary and exits
  0 when clean. `--json` prints `{ scanned, broken }`.
- `.github/workflows/ci.yml`: added the `docs-links` job (checkout,
  `setup-node` 24, `node scripts/check-doc-links.mjs`). No npm install,
  browser or network.
- `runtime/docs/maintainers/launch-checklist.md`: changed the line-8 link from
  `../../packaging/npm/scripts/publish.sh` to
  `../../packaging/npm/legacy-vowp/scripts/publish.sh`, matching the moved
  script. Both the visible label and the target were updated.

## Verification

- `node scripts/check-doc-links.mjs` → `PASS  check-doc-links: 342 files, all
  relative links resolve`, exit 0. That is the 341 repository docs plus this
  report; the earlier clean run before the report was committed scanned 341.
- Temporary fixture in `/tmp/opencode` (own git repo with the script copied in):
  a bogus image and a bogus link were reported as
  `docs/links.md:5 missing-image.png` and `docs/links.md:16 missing.md`, exit 1.
  Broken-looking links inside backtick spans, a triple-backtick fence and a
  tilde fence were not reported.
- `python3` + `yaml.safe_load` on `.github/workflows/ci.yml` → parses; the
  `docs-links` job has checkout, setup-node 24 and the expected run step.
- `bash runtime/docs/tests/run.sh` → `PASS check-links: 1161 relative links in
  256 files, 52 headings checked`, `PASS check-commands`, `PASS docs`.
- `cd runtime/docs/maintainers && test -f
  ../../packaging/npm/legacy-vowp/scripts/publish.sh` → target exists.
- `bash scripts/check-public-readiness.sh` → PASS, scanned 1698 tracked text
  files (includes the new script).
- `git diff --stat 6ae7da7...HEAD -- scripts/check-doc-links.mjs
  .github/workflows/ci.yml runtime/docs/maintainers/launch-checklist.md`:
  3 files changed, 141 insertions, 1 deletion. No other paths changed.

## Broken links outside the owned file

None. Before the fix the checker reported exactly one broken link, the stale
publish script in `launch-checklist.md`; no other real broken link was found
across the repository's 341 Markdown files before this report was added.
