# Launch checklist — VOWP 0.1.0

This is the ordered runbook for the owner/orchestrator who publishes VOWP
0.1.0. It combines the public-audit runbook
([`scripts/audit/go-public.md`](../../scripts/audit/go-public.md)), the release
process ([`release-process.md`](release-process.md)), the crates.io procedure
([`publishing.md`](publishing.md)) and the npm publish script
([`packaging/npm/scripts/publish.sh`](../../packaging/npm/scripts/publish.sh)).

The public repository is a **clean repository without the old history**: one
fresh commit of the final tree, author `Syver-August
<92798051+syver777@users.noreply.github.com>`, commit message `VOWP 0.1.0`. The
current private repository and its history stay private. The first public
version is **0.1.0** for every crate, the npm package and the tag. There is no
website deploy, domain or Vercel step; GitHub, the README and `docs/` are the
home.

Run every step from a clean checkout of the final `main`, not from a job
worktree with uncommitted changes. Record the results in the launch log at the
bottom. **Never put a token on a command line, in a file, in this document or in
shell history.** Use the environment variables named below and clear them when
you are done.

## 0. Inputs and owner decisions

- [ ] `gh` is authenticated as the owner: `gh auth status`.
- [ ] The owner has a crates.io token and an npm token available to set as
      `CARGO_REGISTRY_TOKEN` and `NODE_AUTH_TOKEN` when the publish step starts.
      Refer to them by name only.
- [ ] Repository names: the old private repository is renamed to
      `syver777/vowp-private`; the new public repository is `syver777/vowp`.
- [ ] Release version and tag: `0.1.0` / `v0.1.0`.
- [ ] No website: do not deploy `site/`, do not create a domain and do not add a
      site link to the README or release notes.
- [ ] Decide whether GitHub Discussions is on or off. The default for 0.1.0 is
      **off**; issues are the support entry point.
- [ ] Decide whether to publish a Homebrew tap. The 0.1.0 release does **not**
      publish one; `packaging/homebrew/vowp.rb` is only a template.

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
scripts/audit/scan.sh --worktree
scripts/audit/licenses.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --locked
```

`scan.sh --worktree` must exit 0. If it reports the six unowned home-path files
on an older tree (before the fixture cleanup), apply the staged owner fix and
commit those paths (or make sure the follow-up job that owns them has been
merged):

```sh
scripts/audit/fix-unowned-private-paths.sh              # dry run; 0 changes once the cleanup is merged
scripts/audit/fix-unowned-private-paths.sh --execute    # owner action on an older tree
git diff                                                # review only these six files
git add crates/vowp-protocol/tests/domain.rs \
        crates/vowp-protocol/tests/fixtures/event_log_lines.jsonl \
        crates/vowp-protocol/tests/fixtures/job_n04_graph.json \
        crates/vowp-protocol/tests/report.rs \
        docs/design/adapters.md \
        docs/design/types.md
git commit -m "tests, docs: use the conventional /home/user placeholder"
```

### 1.2 Version audit

Every crate, the npm package and the release docs must say `0.1.0`; historical
design notes and the old internal candidate may mention earlier versions.

```sh
cargo metadata --no-deps --format-version 1 |
  python3 -c 'import json, sys
d = json.load(sys.stdin)
bad = [(p["name"], p["version"]) for p in d["packages"] if p["version"] != "0.1.0"]
print("all workspace packages are 0.1.0" if not bad else bad)
raise SystemExit(1 if bad else 0)'

if grep -RIn '0\.1\.0-' Cargo.toml crates/*/Cargo.toml crates/*/README.md \
    packaging/npm/package.json packaging/npm/README.md README.md \
    docs/guides docs/reference; then
  echo "unexpected pre-release version" >&2
  exit 1
fi

cargo run -q -p vowp-cli --bin vowp -- --version   # vowp 0.1.0
```

Also check the documentation leftovers before building the public branch:

```sh
# The package manifest is at the workspace root, not in crates/vowp-cli.
grep -RIn 'cargo install --path crates/vowp-cli' README.md docs/guides || true

# The dead, non-wired integration test was removed; it must not come back.
grep -RIn '0\.1\.0-rc' crates/vowp-cli/tests 2>/dev/null || true

# The roadmap's version sentence must say 0.1.0. The two links to the
# historical docs/release/0.1.0-rc.1.md stub are intentional and allowed.
grep -n 'identifies itself as' docs/roadmap.md
```

Expected: the first two commands print nothing. The third prints the roadmap
sentence containing `identifies itself as` followed by `0.1.0`; if it says
`0.1.0-rc.1`, fix the sentence before `public-main`. Do not remove the two historical
`0.1.0-rc.1` links in `docs/roadmap.md`; they point at the intentionally
preserved stub. The source-install command is `cargo install --path .` and the
dead `crates/vowp-cli/tests/version.rs` is gone on current `main`.

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
`README.md` and `LICENSE`.

### 1.4 Documentation and script checks

```sh
bash docs/tests/run.sh
bash scripts/tests/run.sh
```

Both must exit 0. The docs check covers relative links and the documented CLI
commands; it does **not** validate installer paths such as the leftovers in 1.2.

### 1.5 Manual private-data review

The scanner covers home paths, e-mails, tokens, private hosts, SSH hosts and
large files. It cannot judge prose. Review these before publication:

```sh
git grep -nI 'vowp-night' -- .
```

The product/workflow name in `AGENTS.md`, `CONTRIBUTING.md`, `framework/` and
the commit trailer is a naming decision, not a credential. The private
operational details are different: `docs/design/plan.md` section 16 ("The
overnight build") and `docs/dogfood/RESULTS.md` mention the private night
infrastructure and the tmux incident. Remove or rewrite those sections, or
record an explicit owner acceptance in the launch log. Do not publish private
hostnames, home directories, costs or session names.

```sh
sed -n '/## 16\./,/^## 17\./p' docs/design/plan.md
sed -n '1,60p' docs/dogfood/RESULTS.md
```

### 1.6 Binary attribution

MIT, Apache-2.0, BSD, ISC, Unicode and Zlib all require notices when their code
is redistributed in a binary. `scripts/audit/THIRD-PARTY-LICENSES.md` is the
generated list. The release workflow currently copies only `LICENSE` and
`README.md` into each archive, so either add the generated file to the archive
in `.github/workflows/release.yml` or add a reviewed root
`THIRD-PARTY-LICENSES.md` and include it. Do this before tagging, then update
the archive-contents sentence in `docs/release/0.1.0.md` if the file list
changes.

**Do not build `public-main` until every item in 1.1–1.6 passes or the owner
records a decision in the launch log.**

## 2. Build the public branch

```sh
scripts/audit/squash-for-public.sh
```

The script refuses on a dirty tree, a failed scan, failed fmt/clippy/tests, or a
top-level path outside the publication allow-list. It creates the orphan branch
`public-main` with exactly one commit and never pushes.

```sh
git log --oneline --decorate -1 public-main
git rev-list --count public-main                 # must be 1
git log -1 --format='%an <%ae>%n%s' public-main  # author and "VOWP 0.1.0"
git ls-tree -r --name-only public-main | wc -l
scripts/audit/scan.sh --rev public-main          # must exit 0
```

Expected: one commit, the owner's author identity, message `VOWP 0.1.0`, and a
clean scan. Record the `public-main` commit hash.

## 3. Rename the old private repository

This frees the name `vowp` and keeps the old history reachable only to the
owner. It does not change visibility or delete anything.

```sh
gh repo rename vowp-private --repo syver777/vowp --yes
gh repo view syver777/vowp-private --json name,visibility,url
```

Expected: `name` is `vowp-private`, `visibility` is `PRIVATE`, and the old URL
redirects. Update any local clone whose `origin` points at the old name:

```sh
git remote set-url origin git@github.com:syver777/vowp-private.git
```

## 4. Create the public repository and its settings

Create an **empty** repository: no README, license, `.gitignore` or initial
commit, so the first commit is exactly `public-main`.

```sh
gh repo create syver777/vowp --public \
  --description "Local-first supervisor for AI coding agents"

gh repo view syver777/vowp --json name,visibility,url,defaultBranchRef
```

Expected: `visibility` is `PUBLIC` and the repository is empty. Set the
description and topics:

```sh
gh repo edit syver777/vowp \
  --description "Local-first supervisor for AI coding agents" \
  --add-topic ai-agents --add-topic coding-agents --add-topic rust \
  --add-topic supervisor --add-topic tmux

gh repo view syver777/vowp --json description,repositoryTopics
```

Enable private vulnerability reporting in **Settings → Code security and
analysis → Private vulnerability reporting**, then verify it:

```sh
gh api -X PUT repos/syver777/vowp/private-vulnerability-reporting
gh api repos/syver777/vowp/private-vulnerability-reporting
```

The response must contain `"enabled": true`. If the PUT endpoint is unavailable
in the installed `gh`, enable it in the web UI and keep the verification call.

In the same settings page, enable **secret scanning** and **push protection**
if GitHub has not enabled them for the public repository, then check:

```sh
gh api repos/syver777/vowp \
  --jq '.security_and_analysis.secret_scanning.status,
        .security_and_analysis.secret_scanning_push_protection.status'
```

Expected: both are `enabled`.

Discussions: leave them **off** unless the owner decided otherwise in step 0.
If they are wanted:

```sh
gh api -X PATCH repos/syver777/vowp -F has_discussions=true
gh repo view syver777/vowp --json hasDiscussionsEnabled
```

## 5. Push `public-main` as `main`

```sh
git remote add public git@github.com:syver777/vowp.git
# If the remote already exists:
# git remote set-url public git@github.com:syver777/vowp.git

git push public public-main:main
```

Verify that the remote branch is exactly the public commit:

```sh
git ls-remote public refs/heads/main
git rev-parse public-main
```

The two hashes must match. Open the repository in a private browser window; the
file list must match `git ls-tree -r --name-only public-main`, and there must be
no old branches or tags.

## 6. Optional: validate the release workflow before tagging

This runs the same build/checksum jobs without creating a release. It costs CI
time but catches a broken workflow before the tag exists.

```sh
gh workflow run release.yml --repo syver777/vowp --ref main -f version=0.1.0

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

## 7. Tag `v0.1.0` and run the release workflow

```sh
git tag -a v0.1.0 -m "VOWP 0.1.0" public-main
git push public v0.1.0
git ls-remote --tags public v0.1.0
```

The tag push starts `.github/workflows/release.yml`. Watch the tag run (not the
earlier manual-dispatch run):

```sh
gh run list --repo syver777/vowp --workflow Release --event push --limit 5
TAG_SHA=$(git rev-parse v0.1.0)
RUN_ID=$(gh run list --repo syver777/vowp --workflow Release --event push \
  --commit "$TAG_SHA" --limit 1 --json databaseId --jq '.[0].databaseId')
test "$RUN_ID" != "null"
gh run watch "$RUN_ID" --repo syver777/vowp --compact --exit-status
```

Expected: the workflow is green and created a **draft** release named `v0.1.0`.

```sh
gh release view v0.1.0 --repo syver777/vowp \
  --json isDraft,name,tagName,url,assets
```

`isDraft` must be `true`. Do not publish it yet.

## 8. Verify the draft release, then publish it

```sh
rm -rf /tmp/vowp-release && mkdir -p /tmp/vowp-release
gh release download v0.1.0 --repo syver777/vowp --dir /tmp/vowp-release
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

tar -xzf vowp-0.1.0-x86_64-unknown-linux-gnu.tar.gz
./vowp-0.1.0-x86_64-unknown-linux-gnu/vowp --version   # vowp 0.1.0
```

On macOS extract and run the matching `apple-darwin` archive instead. The
non-native archives cannot run here; the workflow smoke-tested each binary on a
native runner and the checksums cover all four. Every archive must contain
`vowp`, `LICENSE` and `README.md`.

Publish the draft only after those checks pass:

```sh
gh release edit v0.1.0 --repo syver777/vowp --draft=false --latest
gh release view v0.1.0 --repo syver777/vowp --json isDraft,url,assets
gh api repos/syver777/vowp/releases/latest --jq '.tag_name, .draft'

cd "$RELEASE_ROOT"
```

Expected: `isDraft` is `false`, `/releases/latest` reports `v0.1.0` and
`false`, and the notes from `docs/release/0.1.0.md` render correctly.

## 9. Publish the seven crates to crates.io

Use the script. It is dry-run by default and reads the token only from
`CARGO_REGISTRY_TOKEN`.

```sh
cd "$RELEASE_ROOT"
scripts/release/publish-crates.sh          # dry run: checks and planned order

# Preferred: inject CARGO_REGISTRY_TOKEN from the orchestrator's secret store.
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

## 10. Publish the npm wrapper

The npm script refuses unless the public GitHub release assets for `0.1.0`
exist and their checksums match. Publish the GitHub release (step 8) first.

```sh
cd "$RELEASE_ROOT"
packaging/npm/scripts/publish.sh           # dry run: assets + npm publish --dry-run

# Preferred: inject NODE_AUTH_TOKEN from the orchestrator's secret store.
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
npm view vowp version                      # 0.1.0
npm view vowp dist.tarball
```

## 11. Post-publish verification in clean containers

Run these in a clean environment, not the release checkout.

### 11.1 `cargo install vowp-cli`

```sh
docker run --rm rust:1.85-bookworm bash -lc '
  export CARGO_HOME=/tmp/cargo
  cargo install vowp-cli --version 0.1.0 --locked
  "$CARGO_HOME/bin/vowp" --version
'
```

Expected: `vowp 0.1.0`. If `--locked` fails because the published package has no
lockfile, record the exact error and retry without `--locked`; then fix the
package before announcing the release.

### 11.2 `npm install -g vowp`

```sh
docker run --rm node:22-bookworm bash -lc '
  npm install -g vowp
  vowp --version
'
```

Expected: `vowp 0.1.0`. Repeat on arm64 only if an arm64 host or emulation is
available; the npm package supports both Linux architectures.

### 11.3 Quickstart and example

Set `VOWP_BIN` to the **absolute path** of the release binary you installed:
`/tmp/cargo/bin/vowp` from 11.1, the npm-installed binary from 11.2, or the
`vowp` file from a prebuilt archive. The block refuses a missing or
non-executable path.

```sh
VOWP_BIN=${VOWP_BIN:?set VOWP_BIN to an installed vowp 0.1.0 binary}
test -x "$VOWP_BIN"
"$VOWP_BIN" --version

export HOME=$(mktemp -d)
WORK=$(mktemp -d)
git init -q -b main "$WORK/repo"
git init -q --bare "$WORK/origin.git"
git -C "$WORK/repo" remote add origin "$WORK/origin.git"
cd "$WORK/repo"
"$VOWP_BIN" init --name hello-vowp
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

## 12. Repository settings and final checks

```sh
cd "$RELEASE_ROOT"
gh repo view syver777/vowp \
  --json name,visibility,description,repositoryTopics,defaultBranchRef,hasDiscussionsEnabled
gh api repos/syver777/vowp/private-vulnerability-reporting
gh release view v0.1.0 --repo syver777/vowp --json isDraft,url,assets
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
- [ ] Release `v0.1.0` is published, is the latest release, and every asset
      downloads.
- [ ] crates.io shows all seven crates at `0.1.0`; npm shows `vowp@0.1.0`.
- [ ] The README says this is an early release and does not link to a site.
- [ ] The repository has no old history: a fresh clone's `git log --oneline`
      shows the single `VOWP 0.1.0` commit plus any later release commits.
- [ ] The old private repository is still private and named `vowp-private`.
- [ ] `docs/release/0.1.0.md` is the published release body and its install
      commands match the artifacts that exist.
- [ ] `git status --short` is clean in the release checkout.

## 13. Rollback and irreversible actions

Before the draft is published, no release or package is public, but steps 4–7
have already created the public repository and pushed `main` and the `v0.1.0`
tag. Those are visible actions; use the table below if one of them is wrong.

| Action | Can it be undone? | What to do |
|---|---|---|
| GitHub repository rename | Yes | Rename it back; old URLs redirect. |
| New public repository | Yes, destructively | Delete it before anyone depends on it; forks may survive. |
| Pushed public commit/tag | Yes, but visible | Delete the tag/release and force-push only if the repository is new and nobody has cloned; this is destructive. |
| Draft release is wrong | Yes | `gh release delete v0.1.0 --repo syver777/vowp --yes`, fix the tree or workflow, re-tag. Deleting a draft does not delete the tag. |
| Tag points at the wrong commit, before publishing | Yes | `git push public :refs/tags/v0.1.0 && git tag -d v0.1.0`, fix `main`, then `git tag -a v0.1.0 -m "VOWP 0.1.0"` and `git push public v0.1.0`. |
| Published GitHub release is wrong | Yes, but visible | The normal path is to ship `0.1.1` with a clear note and leave `v0.1.0` and its tag in place; consumers may already have the assets. Deleting the release and its published tag with `gh release delete v0.1.0 --repo syver777/vowp --yes --cleanup-tag` is a last resort for a release that nobody could have downloaded. |
| crates.io version | **No** | `cargo yank --version 0.1.0 <crate>` only stops new resolutions from selecting it. The version stays downloadable by exact pin and the name is used forever. Publish `0.1.1` for a fix. |
| npm version | Limited | `npm deprecate vowp@0.1.0 "reason"` is the normal path. `npm unpublish vowp@0.1.0` is time-limited and disruptive; prefer deprecate plus `0.1.1`. |
| Homebrew tap | Yes | Revert the tap commit. 0.1.0 does not publish a tap. |
| Website | Not applicable | There is no site deploy, domain or Vercel project. |

The older tag-rollback sequence in
[`release-process.md`](release-process.md#rollback) omits `git tag -d v0.1.0`;
use the commands in this table until that page is updated.

Never rewrite a published tag or a published crates.io version. If `0.1.0` is
wrong, yank it where possible and publish `0.1.1` with a clear note.

### Credential hygiene

- `gh auth status` is safe; never pass `--show-token`.
- Use `CARGO_REGISTRY_TOKEN` and `NODE_AUTH_TOKEN` in the environment only.
  Never write them to `.env`, `.cargo/credentials`, `.npmrc` or shell history.
  The publish scripts create and delete temporary config files themselves.
- Do not paste command output that contains a token. The audit scanner masks
  findings for a reason.
- After publishing, unset the variables and close the shell.
- If a token leaks, revoke it at the provider, not in this repository.

## 14. Launch log

Fill this in as the steps complete; it is the release record.

| Item | Value |
|---|---|
| Pre-flight `main` commit | |
| `public-main` commit | |
| Manual workflow run id | |
| Tag run id | |
| GitHub release URL | |
| `SHA256SUMS` hash | |
| crates.io publish result | |
| npm publish result | |
| `cargo install vowp-cli` result | |
| `npm install -g vowp` result | |
| Quickstart/example result | |
| Private-data review decision | |
| Discussions on/off | |
| Homebrew tap (yes/no) | |
| Owner/date | |
