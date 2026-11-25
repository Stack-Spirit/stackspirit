# Publishing a release

`.github/workflows/release.yml` implements the tag → draft → owner-publish → OIDC sequence
(design: `docs/design/relay-and-packaging.md` §5). Nothing in the repository publishes by itself:
a tag creates a **draft**, and npm/PyPI are published only after a human publishes that draft.

## Steps

1. Bump the version everywhere (Cargo workspace, `packaging/npm/**`, `packaging/pypi/stackspirit`),
   and check it: `bash scripts/release/check-release.sh --version X.Y.Z --tag vX.Y.Z --publish`.
2. Push a `vX.Y.Z` tag on a commit on `main`. The workflow builds the four VOWP archives
   (`vowp-<version>-<target>.tar.gz`), the private StackSpirit archives
   (`stackspirit-<version>-<target>.tar.gz`, workflow artifact only), the four platform npm tarballs,
   the root npm tarball and the four wheels, runs an offline install smoke test, writes `SHA256SUMS`
   and creates a draft release in the `release` environment.
3. Review the draft. Delete it and re-tag if it is wrong; no registry was touched.
4. Publish the draft. The `release: published` event runs `publish-npm` (environment `npm`) and
   `publish-pypi` (environment `pypi`) with GitHub OIDC; each downloads the release assets,
   verifies `SHA256SUMS`, attests them, refuses an already-published version and publishes.
5. `verify-install` runs `npx -y stackspirit@X.Y.Z --version`, `npx -y -p stackspirit@X.Y.Z vowp
   --version`, an MCP `initialize` through `npx -y stackspirit@X.Y.Z mcp`, and
   `uvx stackspirit@X.Y.Z --version` on clean runners.

A manual `workflow_dispatch` run is a dry build. `publish: true` skips the build and runs only the
publish jobs for an already-published release.

## One-time owner setup (not done by the repository)

- GitHub environments `release`, `npm` and `pypi`, each with required reviewers.
- npm trusted publishers for `stackspirit` and the four `@stackspirit/*` packages naming workflow
  `release.yml` and environment `npm`.
- A PyPI trusted publisher for `stackspirit` naming workflow `release.yml` and environment `pypi`.

No `NPM_TOKEN`, `PYPI_API_TOKEN` or password exists or is needed.

## Tests

`bash scripts/release/test-workflow.sh` checks that `id-token: write` appears only in the publish
jobs, no registry token is named, both publish jobs declare an environment, the release job is
draft-only, every action is pinned to a SHA, the matrix is the four targets, and the version
check rejects every mutation of a test manifest.
