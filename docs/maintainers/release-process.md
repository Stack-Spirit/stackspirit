# Release process

How a VOWP release is built, checked, published and rolled back. The release
workflow has not run on GitHub yet; it was validated locally as described under
[Validation performed](#validation-performed). The first manual dispatch is the
real end-to-end test.

## Artifact names (final)

These names are used by `scripts/install.sh`, the npm wrapper and the
Homebrew formula. For version `X.Y.Z` and tag `vX.Y.Z` the draft release
contains exactly:

| Asset | Contents |
|---|---|
| `vowp-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | `vowp-X.Y.Z-x86_64-unknown-linux-gnu/{vowp,LICENSE,README.md}` |
| `vowp-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz` | `vowp-X.Y.Z-aarch64-unknown-linux-gnu/{vowp,LICENSE,README.md}` |
| `vowp-X.Y.Z-x86_64-apple-darwin.tar.gz` | `vowp-X.Y.Z-x86_64-apple-darwin/{vowp,LICENSE,README.md}` |
| `vowp-X.Y.Z-aarch64-apple-darwin.tar.gz` | `vowp-X.Y.Z-aarch64-apple-darwin/{vowp,LICENSE,README.md}` |
| `vowp-X.Y.Z-<target>.tar.gz.sha256` | One per archive, `<hash>  <archive name>` |
| `SHA256SUMS` | All four archive hashes, same format, one line each |

For the first release (`0.1.0`) the four archive names are:

```
vowp-0.1.0-x86_64-unknown-linux-gnu.tar.gz
vowp-0.1.0-aarch64-unknown-linux-gnu.tar.gz
vowp-0.1.0-x86_64-apple-darwin.tar.gz
vowp-0.1.0-aarch64-apple-darwin.tar.gz
```

Download URL pattern:

```
https://github.com/syver777/vowp/releases/download/vX.Y.Z/vowp-X.Y.Z-<target>.tar.gz
https://github.com/syver777/vowp/releases/download/vX.Y.Z/vowp-X.Y.Z-<target>.tar.gz.sha256
https://github.com/syver777/vowp/releases/download/vX.Y.Z/SHA256SUMS
```

## What the workflow does

`.github/workflows/release.yml` has four jobs:

1. **prepare** resolves the version (tag without `v`, the manual dispatch
   input, or the `Cargo.toml` workspace version), checks that it matches
   `Cargo.toml`, and picks the release notes: `docs/release/<version>.md` when
   it exists, otherwise `CHANGELOG.md`.
2. **build** runs once per target on a native GitHub-hosted runner:
   `ubuntu-22.04` (x86_64 Linux), `ubuntu-22.04-arm` (aarch64 Linux),
   `macos-15-intel` (x86_64 macOS) and `macos-15` (aarch64 macOS). The Linux
   archives are built against Ubuntu 22.04's glibc 2.35. It builds with
   `cargo build --release --locked -p vowp-cli --bin vowp --target <target>`,
   runs `vowp --version` and compares it with the release version, then creates
   the archive, the per-archive `.sha256`, and uploads both as a workflow
   artifact.
3. **checksums** downloads all four build artifacts, writes `SHA256SUMS`, runs
   `sha256sum --check` against every `.sha256`, and uploads the complete set as
   the `release-assets` artifact.
4. **release** runs only for a `v*` tag push. It downloads `release-assets` and
   creates a **draft** GitHub release with the notes file and every asset. It
   never publishes the release.

The release profile is set from the environment rather than `Cargo.toml`:
`CARGO_PROFILE_RELEASE_STRIP=symbols` and `CARGO_PROFILE_RELEASE_LTO=thin`.
`--locked` makes the build use the committed `Cargo.lock`. The macOS archives
are not code-signed or notarized.

Third-party actions are pinned to full commit SHAs. The tags they were resolved
from on 2026-11-09 are noted next to each SHA:

| Action | Tag |
|---|---|
| `actions/checkout` | v4.4.0 |
| `actions-rust-lang/setup-rust-toolchain` | v1.17.0 |
| `actions/upload-artifact` | v4.6.2 |
| `actions/download-artifact` | v4.3.0 |
| `softprops/action-gh-release` | v2.6.2 |

## Before tagging

The tag version, `Cargo.toml` workspace version, crate versions and the npm
package version must all be the same plain version (`0.1.0` for the first
release). The workflow refuses a tag whose version does not match
`Cargo.toml`.

Run the repository checks on the exact commit to tag:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Check that `Cargo.lock` is committed (the release build uses `--locked`).

## Test without tagging

### Manual dispatch (no release)

On the repository's Actions tab choose **Release** and **Run workflow**, leave
the version empty to use `Cargo.toml`, or enter `0.1.0`. With `gh`:

```sh
gh workflow run release.yml --ref main -f version=0.1.0
gh run watch
```

A manual run builds the four archives, verifies the checksums and uploads the
`release-assets` workflow artifact. It does **not** create a tag or a release.
Download the artifact from the run page and check it with the commands in
[Publish the draft](#publish-the-draft) (use the artifact directory instead of a
release).

### `act`

`act` is not installed in the night container, so it was not run here. On a
machine with Docker and `act`, this runs the manual-dispatch path locally:

```sh
act workflow_dispatch \
  -W .github/workflows/release.yml \
  --input version=0.1.0 \
  --artifact-server-path /tmp/vowp-act-artifacts
```

The Linux jobs run in containers. The macOS jobs cannot run under `act`; use a
throwaway fork for those.

### Throwaway fork

Push the release branch to a throwaway repository, run **Actions → Release →
Run workflow** there (version `0.1.0`), and inspect the artifacts. To exercise
the draft-release job, push a tag `v0.1.0` to the throwaway repository; the
draft release is created there and stays private/draft. Nothing in the real
repository is touched.

## Tag and build

The orchestrator runs these after the release commit is on `main`:

```sh
git tag -a v0.1.0 -m "VOWP 0.1.0"
git push origin v0.1.0
```

The tag push starts the workflow. When it finishes, the release is a draft at
`https://github.com/syver777/vowp/releases` with the notes from
`docs/release/0.1.0.md` (or `CHANGELOG.md`) and the assets listed at the top of
this page.

## Publish the draft

Verify every asset before publishing. Download the draft's assets and check
both the combined checksum file and the per-archive files:

```sh
mkdir -p /tmp/vowp-release
gh release download v0.1.0 --dir /tmp/vowp-release
cd /tmp/vowp-release
sha256sum -c SHA256SUMS
sha256sum -c ./*.tar.gz.sha256
```

Check the archive layout and run the native binary:

```sh
tar -tzf vowp-0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf vowp-0.1.0-x86_64-unknown-linux-gnu.tar.gz
./vowp-0.1.0-x86_64-unknown-linux-gnu/vowp --version
```

Expected: `vowp 0.1.0`. On macOS use the matching `apple-darwin` archive.

Publish the draft only after those checks pass:

```sh
gh release edit v0.1.0 --draft=false
```

Publishing is a public action and is the orchestrator's job. After it is
public, update the Homebrew tap (see below) and let the npm package publisher
use the same release assets.

## Installer

`scripts/install.sh` downloads the archive for the current machine, verifies
the `.sha256`, extracts it in a temporary directory, checks that the binary
reports the requested version, and installs it to `~/.local/bin`
(or `--prefix DIR`). It refuses unsupported platforms, never uses `sudo`, and
never pipes downloaded data to a shell. `VOWP_VERSION` pins a release; with no
version it resolves the latest release.

```sh
VOWP_VERSION=0.1.0 ./scripts/install.sh --no-service --no-cron
VOWP_VERSION=0.1.0 ./scripts/install.sh --dry-run --no-service --no-cron
```

From a source checkout with a built binary, `./scripts/install.sh` installs
that binary and, when the templates are next to the script, the systemd unit
and cron watchdog described in `scripts/README.md`. `--download` forces the
prebuilt path.

To test the download path without GitHub, serve a fake release tree:

```sh
mkdir -p /tmp/vowp-release/v0.1.0
# put the four archives, their .sha256 files and SHA256SUMS in that directory
python3 -m http.server 18765 --directory /tmp/vowp-release
# in another terminal:
HOME=/tmp/vowp-home VOWP_VERSION=0.1.0 \
  VOWP_RELEASE_BASE_URL=http://localhost:18765 \
  ./scripts/install.sh --no-service --no-cron
```

A checksum mismatch stops the install and leaves the prefix unchanged.

## Homebrew

The formula template is `packaging/homebrew/vowp.rb`; run
`packaging/homebrew/update.sh <version>` to fill it from the release
`SHA256SUMS` before copying it to the tap. The tap repository `homebrew-vowp`
is created by the owner, not by this repository. The full commands, including
`brew tap` and the update step, are in
[`packaging/homebrew/README.md`](../../packaging/homebrew/README.md).

## Rollback

Nothing before the draft is published changes public state.

- **Draft release is wrong:** delete it and re-run the workflow after fixing the
  tag or commit. Deleting a draft does not delete the tag:

  ```sh
  gh release delete v0.1.0 --yes
  ```

- **Tag points at the wrong commit, before publishing:** delete the remote tag,
  fix `main`, and tag again:

  ```sh
  git push origin :refs/tags/v0.1.0
  git tag -a v0.1.0 -m "VOWP 0.1.0"
  git push origin v0.1.0
  ```

- **Published release is wrong:** delete the release and tag, then ship a
  corrected version. Deleting a published release does not remove the tag
  unless `--cleanup-tag` is passed:

  ```sh
  gh release delete v0.1.0 --yes --cleanup-tag
  ```

  Consumers may already have downloaded assets, so prefer a new patch version
  over replacing published files. A crate or npm version that was already
  published cannot be treated as removable: crates.io allows only a yank, and
  npm allows an unpublish only within a limited window. Revert the Homebrew
  tap to the previous formula and publish a fixed release.

## Validation performed

On 2026-11-09 in this worktree:

```sh
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
sh -n scripts/install.sh
sh -n packaging/homebrew/update.sh
docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color
docker run --rm -v "$PWD:/mnt" -w /mnt koalaman/shellcheck:stable -s sh \
  scripts/install.sh packaging/homebrew/update.sh
docker run --rm -v "$PWD:/mnt" -w /mnt ruby:3-alpine ruby -c packaging/homebrew/vowp.rb
```

Every `run:` block in the workflow was extracted and checked with `bash -n`.
The existing `sh scripts/tests/run.sh` suite still passes, and
`bash docs/tests/run.sh` passes its link and command checks.
The installer was exercised against a fake release served by
`python3 -m http.server`, including a good checksum, a tampered checksum, a
custom `--prefix`, and an unsupported platform. `packaging/homebrew/update.sh`
was run twice on the same temporary formula (0.1.0 and then 0.2.0 with
different hashes) and against a locally served `SHA256SUMS`, so a second
release updates the existing formula rather than only the placeholder
template.

The workflow has still not run on GitHub. The first manual dispatch on the new
public repository is the remaining end-to-end check; `act` and the macOS
runner labels are documented above but were not exercised locally.
