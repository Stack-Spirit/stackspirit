# Launch checklist — VOWP 0.2.0

This is the ordered runbook for the owner/orchestrator who publishes VOWP
0.2.0. It combines the public-tree audit
([`scripts/audit/go-public.md`](../../scripts/audit/go-public.md)), the release
process ([`release-process.md`](release-process.md)), the crates.io procedure
([`publishing.md`](publishing.md)) and the npm publish script
([`packaging/npm/scripts/publish.sh`](../../packaging/npm/scripts/publish.sh)).

The public repository already exists from 0.1.0 (`syver777/vowp`; the old
private repository is `syver777/vowp-private`). **Add a 0.2.0 release commit to
its `main`; do not recreate the repository and do not rewrite its published
history.** The release version is **0.2.0** for every crate, the npm package and
the tag. There is no website deploy, domain or Vercel step; GitHub, the README
and `docs/` are the home.

Run every step from a clean checkout of the final `main`, not from a job
worktree with uncommitted changes. Record the results in the launch log at the
bottom. **Never put a token on a command line, in a file, in this document or in
shell history.** Use the environment variables named below and clear them when
you are done.

Where [`release-process.md`](release-process.md) or
[`publishing.md`](publishing.md) still show `0.1.0` in an example, this file's
`0.2.0` commands are the current ones; the scripts themselves take the version
from `Cargo.toml` or the npm package.

## 0. Inputs and owner decisions

- [ ] `gh` is authenticated as the owner: `gh auth status`.
- [ ] The owner has a crates.io token and an npm token available to set as
      `CARGO_REGISTRY_TOKEN` and `NODE_AUTH_TOKEN` when the publish step starts.
      Refer to them by name only.
- [ ] Repository names: the public repository is `syver777/vowp`; the old
      private repository is `syver777/vowp-private`.
- [ ] Release version and tag: `0.2.0` / `v0.2.0`.
- [ ] No website: do not deploy `site/`, do not create a domain and do not add a
      site link to the README or release notes.
- [ ] GitHub Discussions stays **off** unless the owner explicitly enables it;
      issues are the support entry point.
- [ ] Do not publish a Homebrew tap for 0.2.0. `packaging/homebrew/vowp.rb` is a
      template; the tap remains an owner decision for a later release.
- [ ] The npm package version must be `0.2.0` before step 10. **This file's job
      does not own `packaging/npm/package.json`;** if it still says `0.1.0`,
      stop and have the release owner bump the package version (and its
      `packaging/npm/README.md` early-release line) before the npm publish.
- [ ] The owner accepts that the T1/T3/T5 owner-device checks are marked "not
      run" in `docs/release/0.2.0.md` and are not waived.

## 1. Pre-flight on the final `main`

```sh
git status --short                 # must be empty
git fetch --all
git rev-parse --short HEAD         # record this commit in the launch log
git log -1 --format='%H %s'
RELEASE_COMMIT=$(git rev-parse HEAD)
RELEASE_ROOT=$(pwd)
export RELEASE_COMMIT RELEASE_ROOT
echo "$RELEASE_COMMIT"
echo "$RELEASE_ROOT"
```

Keep `RELEASE_ROOT` pointing at this checkout. If you open a new shell, re-export
it before running the later steps; the publish and final-check blocks use it to
return to the release checkout after working in temporary directories.

### 1.1 Required checks

```sh
scripts/audit/scan.sh --public-tree
scripts/audit/public-tree-markers.sh --worktree
scripts/audit/licenses.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --locked
```

`scan.sh --public-tree` and `public-tree-markers.sh --worktree` must exit 0.
The public-tree scan skips the maintainer and acceptance paths that are pruned
from the public repository; a plain `--worktree` scan is expected to report
the fake addresses and temporary homes in those private records, so it is not
the release gate. The marker scan rejects private build ids, private paths and
build-diary wording. The audit tooling is pruned from the public tree; these
checks run on the private release checkout before the public tree is built.

### 1.2 Version audit

Every workspace package and the npm package must say `0.2.0`; historical
release notes and acceptance records may mention earlier versions.

```sh
cargo metadata --no-deps --format-version 1 |
  python3 -c 'import json, sys
d = json.load(sys.stdin)
bad = [(p["name"], p["version"]) for p in d["packages"] if p["version"] != "0.2.0"]
print("all workspace packages are 0.2.0" if not bad else bad)
raise SystemExit(1 if bad else 0)'

if grep -RIn '0\.1\.0-' Cargo.toml crates/*/Cargo.toml crates/*/README.md \
    packaging/npm/package.json packaging/npm/README.md README.md \
    docs/guides docs/reference; then
  echo "unexpected pre-release version" >&2
  exit 1
fi

cargo run -q -p vowp-cli --bin vowp -- --version   # vowp 0.2.0

node -p 'require("./packaging/npm/package.json").version'   # must be 0.2.0
```

The npm line is the one deliberate cross-path check. If it prints `0.1.0`, stop:
the package cannot publish a release that does not exist, and editing
`packaging/npm/package.json` is outside the J2 release job's owned paths.

### 1.3 Package and npm dry runs

```sh
scripts/release/check-packages.sh
scripts/release/publish-crates.sh
```

`check-packages.sh` must end with `RESULT: PASS (7 crates packaged and
verified)`. `publish-crates.sh` repeats the checks and prints the publish order
without uploading.

```sh
cd packaging/npm
npm test
npm pack --dry-run --json
cd ../..
```

`npm test` must pass and `npm pack --dry-run --json` must list exactly
`package.json` plus the package's `bin/`, `lib/`, `scripts/postinstall.js`,
`README.md` and `LICENSE`. The publish script refuses when the public release
assets for its package version are missing, so the npm publish itself stays in
step 10.

### 1.4 Documentation and script checks

```sh
bash docs/tests/run.sh
bash scripts/tests/run.sh
```

Both must exit 0. The docs check covers relative links and the documented CLI
commands.

### 1.5 Manual private-data review

The scanner covers home paths, e-mails, tokens, private hosts, SSH hosts, large
files and build markers. It cannot judge prose. Review these before publication:

```sh
git grep -nI 'vowp-night' -- .
git grep -nI 'v2-[a-z0-9]' -- crates framework packaging README.md \
  docs/tests docs/reference docs/guides docs/concepts docs/setup docs/design
```

The remaining `vowp-night` hits are the product/workflow name in `AGENTS.md`
and the audit text that searches for it. The `v2-*` check must find no shipping
job id in a source comment, fixture or public document. Do not reintroduce
private hostnames, home directories, costs or session names.

### 1.6 Binary attribution

MIT, Apache-2.0, BSD, ISC, Unicode and Zlib all require notices when their code
is redistributed in a binary. The reviewed root `THIRD-PARTY-LICENSES.md` ships
in every archive alongside `vowp`, `LICENSE` and `README.md`; it is generated
from the locked dependency tree. Confirm it is current and non-empty before
tagging:

```sh
cargo fetch --locked
python3 scripts/release/generate-third-party-licenses.py --check
test -s THIRD-PARTY-LICENSES.md
```

`scripts/release/package-archive.sh` refuses to build an archive when the
binary, `LICENSE`, `README.md` or `THIRD-PARTY-LICENSES.md` is missing or empty.

**Do not build the public release commit until every item in 1.1–1.6 passes or
the owner records a decision in the launch log.**

## 2. Build the public release commit

The public tree is the product-only tree. Its prune list is maintained in
[`scripts/audit/public-tree.txt`](../../scripts/audit/public-tree.txt); the
current list is:

| Pruned path | Why |
|---|---|
| `AGENTS.md` | Private agent workflow. |
| `docs/dogfood/` | Private dogfood notes. |
| `docs/fresh-user/` | Private acceptance notes. |
| `docs/maintainers/` | Maintainer and acceptance runbooks, including this file. |
| `docs/release/0.1.0-rc.1.md` | Historical pre-release stub. |
| `scripts/acceptance/` | Private acceptance rig and v0.1 fixture generators. |
| `scripts/audit/` | Private publication and secret-scan tooling. |
| `scripts/dogfood/` | Private dogfood helpers. |
| `scripts/fresh-user/` | Private fresh-user helpers. |
| `scripts/spikes/` | Private spike notes and prototypes. |
| `site/` | No website deploy. |

Everything else in the repository is allowed, subject to the manifest's
required list. `docs/release/0.2.0.md` is a product document and ships; it is the
release body the workflow uses.

The public repository already has the `VOWP 0.1.0` commit. Add one `VOWP 0.2.0`
child commit from the audited tree; do not replace the branch or force-push the
old release. From the clean release checkout:

```sh
git remote add public git@github.com:syver777/vowp.git 2>/dev/null || true
git remote set-url public git@github.com:syver777/vowp.git
git fetch public
PUBLIC_BASE=$(git rev-parse public/main)
echo "$PUBLIC_BASE"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-public-0.2.0.XXXXXX")
GIT_INDEX_FILE=$tmp/index git read-tree --empty
GIT_INDEX_FILE=$tmp/index git add -A

for path in AGENTS.md docs/dogfood docs/fresh-user docs/maintainers \
    docs/release/0.1.0-rc.1.md scripts/acceptance scripts/audit \
    scripts/dogfood scripts/fresh-user scripts/spikes site; do
  GIT_INDEX_FILE=$tmp/index git rm -r --cached --ignore-unmatch --quiet -- "$path"
done

GIT_INDEX_FILE=$tmp/index scripts/audit/public-tree-scrub.sh --index
GIT_INDEX_FILE=$tmp/index scripts/audit/public-tree-check.sh --index \
  --manifest scripts/audit/public-tree.txt
tree=$(GIT_INDEX_FILE=$tmp/index git write-tree)
rm -rf "$tmp"

commit=$(GIT_AUTHOR_NAME='Syver-August' \
  GIT_AUTHOR_EMAIL='92798051+syver777@users.noreply.github.com' \
  GIT_COMMITTER_NAME='Syver-August' \
  GIT_COMMITTER_EMAIL='92798051+syver777@users.noreply.github.com' \
  git commit-tree "$tree" -p "$PUBLIC_BASE" -m 'VOWP 0.2.0')
git update-ref refs/heads/public-main "$commit"
```

Then verify the candidate before any push:

```sh
git log --oneline --decorate -2 public-main
git rev-list --count public-main                 # 2: VOWP 0.1.0 + VOWP 0.2.0
git log -1 --format='%an <%ae>%n%s' public-main  # owner identity, "VOWP 0.2.0"
git ls-tree -r --name-only public-main | wc -l
scripts/audit/public-tree-check.sh --rev public-main
scripts/audit/scan.sh --rev public-main
scripts/audit/public-tree-markers.sh --rev public-main
```

All three audit commands must exit 0. Record the `public-main` commit hash and
the file count in the launch log. If the owner explicitly chooses a fresh orphan
history instead of a child commit, use
`scripts/audit/squash-for-public.sh --force` and stop for that decision; never
force-push a published branch by habit.

## 3. Push the public release commit

```sh
git push public public-main:main

git ls-remote public refs/heads/main
git rev-parse public-main
```

The two hashes must match. Open the repository in a private browser window; the
file list must match `git ls-tree -r --name-only public-main`, and there must be
no new branches or tags beyond the release tag you are about to create.

## 4. Optional: validate the release workflow before tagging

This runs the same build/checksum jobs without creating a release. It costs CI
time but catches a broken workflow before the tag exists.

```sh
gh workflow run release.yml --repo syver777/vowp --ref main -f version=0.2.0

RUN_ID=$(gh run list --repo syver777/vowp --workflow Release \
  --event workflow_dispatch --limit 1 --json databaseId --jq '.[0].databaseId')
test "$RUN_ID" != "null"
gh run watch "$RUN_ID" --repo syver777/vowp --compact --exit-status

rm -rf /tmp/vowp-release-dry
gh run download "$RUN_ID" --repo syver777/vowp --name release-assets \
  --dir /tmp/vowp-release-dry
cd /tmp/vowp-release-dry
sha256sum -c SHA256SUMS
sha256sum -c ./*.tar.gz.sha256
cd "$RELEASE_ROOT"
```

If this run fails, fix the workflow and push the fix before tagging. A manual
dispatch never creates a tag or a release.

## 5. Tag `v0.2.0` and run the release workflow

```sh
git tag -a v0.2.0 -m "VOWP 0.2.0" public-main
git push public v0.2.0
git ls-remote --tags public v0.2.0
```

The tag push starts `.github/workflows/release.yml`. Watch the tag run (not the
earlier manual-dispatch run):

```sh
gh run list --repo syver777/vowp --workflow Release --event push --limit 5
TAG_SHA=$(git rev-parse v0.2.0)
RUN_ID=$(gh run list --repo syver777/vowp --workflow Release --event push \
  --commit "$TAG_SHA" --limit 1 --json databaseId --jq '.[0].databaseId')
test "$RUN_ID" != "null"
gh run watch "$RUN_ID" --repo syver777/vowp --compact --exit-status
```

Expected: the workflow is green and created a **draft** release named `0.2.0`
with the four archives, their `.sha256` files and `SHA256SUMS`.

```sh
gh release view v0.2.0 --repo syver777/vowp \
  --json isDraft,name,tagName,url,assets
```

`isDraft` must be `true`. Do not publish it yet.

## 6. Verify the draft release, then publish it

```sh
rm -rf /tmp/vowp-release && mkdir -p /tmp/vowp-release
gh release download v0.2.0 --repo syver777/vowp --dir /tmp/vowp-release
cd /tmp/vowp-release

sha256sum -c SHA256SUMS
sha256sum -c ./*.tar.gz.sha256
```

On macOS use `shasum -a 256 -c SHA256SUMS` and the same for each `.sha256`
file. Every checksum must pass. Then check each archive's layout and run the
binary for this machine:

```sh
for archive in *.tar.gz; do
  echo "== $archive"
  tar -tzf "$archive"
done

tar -xzf vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz
./vowp-0.2.0-x86_64-unknown-linux-gnu/vowp --version   # vowp 0.2.0
```

On macOS extract and run the matching `apple-darwin` archive instead. The
non-native archives cannot run here; the workflow smoke-tested each binary on a
native runner and the checksums cover all four. Every archive must contain
`vowp`, `LICENSE`, `README.md` and `THIRD-PARTY-LICENSES.md`.

Publish the draft only after those checks pass:

```sh
gh release edit v0.2.0 --repo syver777/vowp --draft=false --latest
gh release view v0.2.0 --repo syver777/vowp --json isDraft,url,assets
gh api repos/syver777/vowp/releases/latest --jq '.tag_name, .draft'

cd "$RELEASE_ROOT"
```

Expected: `isDraft` is `false`, `/releases/latest` reports `v0.2.0` and
`false`, and the notes from `docs/release/0.2.0.md` render correctly.

## 7. Publish the seven crates to crates.io from the owner's Mac

Use the script. It is dry-run by default and reads the token only from
`CARGO_REGISTRY_TOKEN`. The owner runs this from the final checkout on the Mac;
the release job only prepares and dry-runs it.

```sh
cd "$RELEASE_ROOT"
scripts/release/publish-crates.sh          # dry run: checks and planned order

# Preferred: inject CARGO_REGISTRY_TOKEN from the owner's secret store.
# Interactive fallback (bash/zsh): read the value without echoing it or writing
# it to shell history.
printf 'crates.io token (CARGO_REGISTRY_TOKEN): '
read -rs CARGO_REGISTRY_TOKEN
printf '\n'
export CARGO_REGISTRY_TOKEN
scripts/release/publish-crates.sh --execute
unset CARGO_REGISTRY_TOKEN
```

The order is `vowp-protocol`, `vowp-store`, `vowp-graph`, `vowp-tmux`,
`vowp-adapters`, `vowp-core`, `vowp-cli`. The script waits for each version to
appear in the crates.io index before continuing and stops at the first failure.
There is no meta-crate: `cargo install vowp-cli` installs the `vowp` binary.

Verify the index and the published metadata:

```sh
for crate in vowp-protocol vowp-store vowp-graph vowp-tmux vowp-adapters vowp-core vowp-cli; do
  cargo search "$crate" --limit 1
done

cargo info vowp-protocol
```

Open `https://docs.rs/vowp-protocol` and confirm the docs built. A published
version cannot be deleted or replaced; only yanked.

## 8. Publish the npm wrapper from the owner's Mac

The npm script refuses unless the public GitHub release assets for its package
version exist and their checksums match. Publish the GitHub release (step 6)
first and bump the package to `0.2.0` (step 0) if it is still `0.1.0`.

```sh
cd "$RELEASE_ROOT"
node -p 'require("./packaging/npm/package.json").version'   # must be 0.2.0
packaging/npm/scripts/publish.sh           # dry run: assets + npm publish --dry-run

# Preferred: inject NODE_AUTH_TOKEN from the owner's secret store.
# Interactive fallback (bash/zsh): no echo and no shell-history entry.
printf 'npm token (NODE_AUTH_TOKEN): '
read -rs NODE_AUTH_TOKEN
printf '\n'
export NODE_AUTH_TOKEN
packaging/npm/scripts/publish.sh --execute
unset NODE_AUTH_TOKEN
```

Verify the registry:

```sh
npm view vowp version                      # 0.2.0
npm view vowp dist.tarball
```

## 9. Post-publish verification in clean containers

Run these in a clean environment, not the release checkout.

### 9.1 `cargo install vowp-cli`

```sh
docker run --rm rust:1.85-bookworm bash -lc '
  export CARGO_HOME=/tmp/cargo
  cargo install vowp-cli --version 0.2.0 --locked
  "$CARGO_HOME/bin/vowp" --version
'
```

Expected: `vowp 0.2.0`. If `--locked` fails because the published package has no
lockfile, record the exact error and retry without `--locked`; then fix the
package before announcing the release.

### 9.2 `npm install -g vowp`

```sh
docker run --rm node:22-bookworm bash -lc '
  npm install -g vowp
  vowp --version
'
```

Expected: `vowp 0.2.0`. Repeat on arm64 only if an arm64 host or emulation is
available; the npm package supports both Linux architectures.

### 9.3 Quickstart and example

Set `VOWP_BIN` to the **absolute path** of the release binary you installed:
`/tmp/cargo/bin/vowp` from 9.1, the npm-installed binary from 9.2, or the
`vowp` file from a prebuilt archive. The block refuses a missing or
non-executable path. The temporary `HOME` and repositories live in a subshell,
so step 10 and later run with the original `HOME` and the owner's `gh`
configuration.

```sh
VOWP_BIN=${VOWP_BIN:?set VOWP_BIN to an installed vowp 0.2.0 binary}
test -x "$VOWP_BIN" || exit 1
"$VOWP_BIN" --version

(
  export HOME=$(mktemp -d)
  WORK=$(mktemp -d)
  git init -q -b main "$WORK/repo"
  git init -q --bare "$WORK/origin.git"
  git -C "$WORK/repo" remote add origin "$WORK/origin.git"
  cd "$WORK/repo"
  "$VOWP_BIN" init --name hello-vowp
)
```

`vowp init` must print the project summary. A full `vowp doctor` needs a signed-in
Claude Code or opencode; in a bare container, either install one or use the
repository's fake harnesses. Run the fake-adapter demo from a checkout as the
end-to-end smoke test:

```sh
cd "$RELEASE_ROOT"
VOWP="$VOWP_BIN" FAKE=1 examples/hello-vowp/run.sh
```

Expected: all three jobs reach `done` and the merge queue leaves the three job
commits on the example's `origin/main`. Nothing touches `~/.vowp` or the real
tmux server.

### 9.4 Upgrade drill from a v0.1 home

Before announcing 0.2.0, run the migration drill from a generated v0.1 home and
keep the path:

```sh
DRILL=/tmp/vowp-v01-drill
H=$DRILL/home
bash scripts/acceptance/rig.sh v01-home --name release-drill \
  --dest "$DRILL" --binary /path/to/vowp-0.1.0 --jobs 3
vowp --home "$H" migrate --dry-run
vowp --home "$H" migrate --yes
vowp --home "$H" migrate --rollback "$H/backups/pre-0.2-<timestamp>.db"
```

The generator and rig are internal and pruned from the public tree; run this
from the private release checkout. The shorter owner check in step 6 of
`docs/release/0.2.0.md` is the runbook users follow.

## 10. Repository settings and final checks

```sh
cd "$RELEASE_ROOT"
gh repo view syver777/vowp \
  --json name,visibility,description,repositoryTopics,defaultBranchRef,hasDiscussionsEnabled
gh api repos/syver777/vowp/private-vulnerability-reporting
gh release view v0.2.0 --repo syver777/vowp --json isDraft,url,assets
gh release list --repo syver777/vowp --limit 5 --json tagName,isDraft,isLatest
gh api repos/syver777/vowp/branches --jq '.[].name'
gh api repos/syver777/vowp/tags --jq '.[].name'
```

`gh release list` supplies `isLatest`; `gh release view --json` does not expose
that field.

- [ ] Private vulnerability reporting is enabled and the advisory link works
      while logged out.
- [ ] Secret scanning and push protection are enabled.
- [ ] The default branch is `main`; only `main` exists (no old branches).
- [ ] Release `v0.2.0` is published, is the latest release, and every asset
      downloads.
- [ ] crates.io shows all seven crates at `0.2.0`; npm shows `vowp@0.2.0`.
- [ ] The README leads with the 0.2.0 setup prompt and does not link to a site.
- [ ] The public repository has the `VOWP 0.1.0` commit, the `VOWP 0.2.0`
      commit and no private history, build metadata or maintainer pages.
- [ ] `docs/release/0.2.0.md` is the published release body and its install
      commands match the artifacts that exist.
- [ ] The T1/T3/T5 owner-device items are still marked "not run" in the release
      notes; no launch text claims they passed.
- [ ] `git status --short` is clean in the release checkout.

## 11. Rollback and irreversible actions

Before the draft is published, the public `main` commit and the `v0.2.0` tag are
visible actions; use the table below if one of them is wrong.

| Action | Can it be undone? | What to do |
|---|---|---|
| Public `main` commit is wrong, before the tag | Yes, but visible | Revert with a new commit or force-push only if the repository is new and nobody has cloned. Prefer a corrective commit. |
| Draft release is wrong | Yes | `gh release delete v0.2.0 --repo syver777/vowp --yes`, fix the tree or workflow, re-tag. Deleting a draft does not delete the tag. |
| Tag points at the wrong commit, before publishing | Yes | `git push public :refs/tags/v0.2.0 && git tag -d v0.2.0`, fix `main`, then `git tag -a v0.2.0 -m "VOWP 0.2.0"` and `git push public v0.2.0`. |
| Published GitHub release is wrong | Yes, but visible | Prefer shipping `0.2.1` with a clear note and leaving `v0.2.0` and its tag in place; consumers may already have the assets. Deleting the release and its published tag with `gh release delete v0.2.0 --repo syver777/vowp --yes --cleanup-tag` is a last resort for a release that nobody could have downloaded. |
| crates.io version | **No** | `cargo yank --version 0.2.0 <crate>` only stops new resolutions from selecting it. The version stays downloadable by exact pin and the name is used forever. Publish `0.2.1` for a fix. |
| npm version | Limited | `npm deprecate vowp@0.2.0 "reason"` is the normal path. `npm unpublish vowp@0.2.0` is time-limited and disruptive; prefer deprecate plus `0.2.1`. |
| Homebrew tap | Yes | Revert the tap commit. 0.2.0 does not publish a tap. |
| Website | Not applicable | There is no site deploy, domain or Vercel project. |

Never rewrite a published tag or a published crates.io version. If `0.2.0` is
wrong, yank it where possible and publish `0.2.1` with a clear note.

### Credential hygiene

- `gh auth status` is safe; never pass `--show-token`.
- Use `CARGO_REGISTRY_TOKEN` and `NODE_AUTH_TOKEN` in the environment only.
  Never write them to `.env`, `.cargo/credentials`, `.npmrc` or shell history.
  The publish scripts create and delete temporary config files themselves.
- Do not paste command output that contains a token. The audit scanner masks
  findings for a reason.
- After publishing, unset the variables and close the shell.
- If a token leaks, revoke it at the provider, not in this repository.

## 12. Launch log

Fill this in as the steps complete; it is the release record.

| Item | Value |
|---|---|
| Pre-flight `main` commit | |
| `public-main` commit | |
| Public tree file count | |
| Manual workflow run id | |
| Tag run id | |
| GitHub release URL | |
| `SHA256SUMS` hash | |
| crates.io publish result | |
| npm publish result | |
| `cargo install vowp-cli` result | |
| `npm install -g vowp` result | |
| Quickstart/example result | |
| v0.1 → v0.2 migration drill | |
| Private-data review decision | |
| Discussions on/off | |
| Homebrew tap (yes/no) | |
| Owner/date | |
