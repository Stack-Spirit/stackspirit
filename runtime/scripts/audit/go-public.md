# Go public: the owner/orchestrator runbook

This is the sequence for publishing VOWP 0.1.0 as a **new public repository without the old history**.
It is written for the owner or the orchestrator who holds the GitHub, crates.io and npm credentials.
p01 did not run any public action; it checked the tool help and ran only read-only or dry-run forms.

**Never put a token on a command line, in a file, in this document or in shell history.** Use the
environment variables named below and clear them when you are done. Nothing in this runbook pushes,
tags, publishes or changes visibility automatically.

Repository: `syver777/vowp` (the current private repository is renamed to `syver777/vowp-private`).
Public branch: `public-main`, one commit, author `Syver-August <92798051+syver777@users.noreply.github.com>`.
Release: tag `v0.1.0`. Versions: all crates, the npm package and the tag are `0.1.0`.

## 0. Pre-flight on the final merged `main`

Do this in a clean checkout of the final `main`, not in a job worktree with uncommitted changes.

```sh
git status --short                 # must be empty
git fetch --all
git rev-parse --short HEAD         # record this commit in the launch log

scripts/audit/scan.sh --public-tree           # must exit 0
scripts/audit/public-tree-markers.sh --worktree  # must exit 0
scripts/audit/licenses.sh                     # must exit 0
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If `scan.sh --public-tree` reports the six unowned files from the audit, the owner runs the staged fix:

```sh
scripts/audit/fix-unowned-private-paths.sh              # dry run: 6 files
scripts/audit/fix-unowned-private-paths.sh --execute    # owner action
git diff                                                # review only these six files
git add crates/vowp-protocol/tests/domain.rs \
        crates/vowp-protocol/tests/fixtures/event_log_lines.jsonl \
        crates/vowp-protocol/tests/fixtures/job_n04_graph.json \
        crates/vowp-protocol/tests/report.rs \
        docs/design/adapters.md \
        docs/design/types.md
git commit -m "tests, docs: use the conventional /home/user placeholder"
```

Then resolve the manual items in `docs/maintainers/public-audit.md` (the `docs/design/plan.md`
"overnight build" section and the `vowp-night` naming decision). Re-run the scan and the build after
every change.

**Verify:** all four checks exit 0 and `git status --short` is empty.

## 1. Build the public branch

```sh
scripts/audit/squash-for-public.sh
```

The script performs, in order: `cargo fmt --check`, clippy, the test suite, the staged public-tree
prune and content scrub, the top-level path allow-list, the orphan commit, the secret scan of
`public-main`, and the private build-marker scan. It refuses to continue on a failure. It never pushes.

When rehearsing the squash in a temporary clone, give Cargo a target directory for that clone (for
example `CARGO_TARGET_DIR=$PWD/target scripts/audit/squash-for-public.sh`) or run `cargo clean` in the
clone afterwards. A shared target directory can reuse test binaries compiled with the clone's
`CARGO_MANIFEST_DIR`; once the clone is deleted, tests that read fixtures from that path fail even
though the real checkout is clean.

```sh
git log --oneline --decorate -1 public-main
git rev-list --count public-main                 # must be 1
git log -1 --format='%an <%ae>%n%s' public-main  # author and "VOWP 0.1.0"
git ls-tree -r --name-only public-main | wc -l
scripts/audit/scan.sh --rev public-main          # must exit 0
scripts/audit/public-tree-markers.sh --rev public-main  # must exit 0
```

**Verify:** exactly one commit, the expected author, the message `VOWP 0.1.0`, and a clean scan. The
script prints the same push commands that follow.

## 2. Rename the old private repository

This frees the name `vowp` and keeps the old history reachable only to the owner. It does not change
visibility and does not delete anything.

```sh
gh repo rename vowp-private --repo syver777/vowp --yes
gh repo view syver777/vowp-private --json name,visibility,url
```

**Verify:** `name` is `vowp-private`, `visibility` is `PRIVATE`, and the old URL redirects. Update any
local clone's `origin` if it points at the old name:

```sh
git remote set-url origin git@github.com:syver777/vowp-private.git
```

## 3. Create the public repository

Create an **empty** repository: no README, no license, no `.gitignore`, so the first commit is exactly
`public-main`.

```sh
gh repo create syver777/vowp --public \
  --description "Local-first supervisor for AI coding agents"

gh repo view syver777/vowp --json name,visibility,url,defaultBranchRef
```

**Verify:** `visibility` is `PUBLIC`, the repository is empty, and the default branch will become
`main` after the first push.

Enable private vulnerability reporting in **Settings → Code security and analysis → Private vulnerability
reporting**. Check it read-only with:

```sh
gh api repos/syver777/vowp/private-vulnerability-reporting
```

The response must contain `"enabled": true`. If the API call is unavailable in the installed `gh`
version, enable it in the web UI and confirm the security advisory form is reachable while logged out.
Also turn on secret scanning/push protection in the same settings page if GitHub has not enabled it for
the public repository.

Optional repository metadata (topics and description) can be set now or after the first release:

```sh
gh repo edit syver777/vowp \
  --description "Local-first supervisor for AI coding agents" \
  --add-topic ai-agents --add-topic coding-agents --add-topic rust \
  --add-topic supervisor --add-topic tmux
```

**Verify:** the repository page shows the description and topics, and private vulnerability reporting is
enabled.

## 4. Push `public-main` as `main`

```sh
git remote add public git@github.com:syver777/vowp.git
# If the remote already exists:
# git remote set-url public git@github.com:syver777/vowp.git

git push public public-main:main
```

**Verify:** the remote `main` is exactly the public commit:

```sh
git ls-remote public refs/heads/main
git rev-parse public-main
```

The two hashes must match. Open the repository in a private browser window; the file list must match
`git ls-tree -r --name-only public-main`, and there must be no old branches or tags.

## 5. Tag `v0.1.0` and start the release workflow

```sh
git tag -a v0.1.0 -m "VOWP 0.1.0" public-main
git push public v0.1.0

git ls-remote --tags public v0.1.0
```

The tag push triggers `.github/workflows/release.yml`. Watch it:

```sh
gh run list --repo syver777/vowp --workflow Release --limit 5
gh run watch <run-id> --repo syver777/vowp --compact --exit-status
```

**Verify:** the workflow is green and created a **draft** release named `v0.1.0`.

```sh
gh release view v0.1.0 --repo syver777/vowp --json isDraft,name,tagName,assets
```

`isDraft` must be `true`. Do not publish it yet.

## 6. Verify the release assets, then publish the release

```sh
rm -rf /tmp/vowp-release && mkdir -p /tmp/vowp-release
gh release download v0.1.0 --repo syver777/vowp --dir /tmp/vowp-release

cd /tmp/vowp-release
sha256sum -c SHA256SUMS            # Linux; on macOS: shasum -a 256 -c SHA256SUMS
# If the workflow publishes per-asset .sha256 files instead:
# sha256sum -c *.sha256            # Linux; macOS: shasum -a 256 -c *.sha256

for archive in *.tar.gz; do
  dir=${archive%.tar.gz}
  mkdir -p "$dir"
  tar -xzf "$archive" -C "$dir"
  "$dir"/*/vowp --version          # must print: vowp 0.1.0
done
```

**Verify:** every checksum passes, every archive contains `vowp`, `LICENSE` and `README`, and every
binary prints `vowp 0.1.0`. If an archive is wrong, fix the workflow and re-run the tag build before
publishing; do not edit a release asset by hand.

Publish the draft:

```sh
gh release edit v0.1.0 --repo syver777/vowp --draft=false --latest
gh release view v0.1.0 --repo syver777/vowp --json isDraft,isLatest,url
```

**Verify:** `isDraft` is `false`, `isLatest` is `true`, and the release notes render correctly.

## 7. Publish the seven crates (plus the meta-crate if p04 adds one)

Use the p04 script. It is dry-run by default and reads the token only from the environment.

```sh
export CARGO_REGISTRY_TOKEN='...'          # owner action; never echo or save it
scripts/release/publish-crates.sh          # dry run: prints the order and checks
scripts/release/publish-crates.sh --execute
unset CARGO_REGISTRY_TOKEN
```

The order is protocol → store/graph/tmux → adapters → core → cli (and `vowp` last if it exists). The
script waits for each crate to appear in the index before the next one.

**Verify** in a clean directory/container:

```sh
cargo search vowp-protocol --limit 1
cargo install vowp-cli --version 0.1.0 --locked
vowp --version                             # vowp 0.1.0
```

Repeat `cargo search` for each crate. A published crates.io version cannot be replaced or deleted; it can
only be yanked. Do not publish a version you have not tested.

## 8. Publish the npm wrapper

Use the p05 script. It is dry-run by default and reads the token only from the environment.

```sh
export NODE_AUTH_TOKEN='...'               # owner action; never echo or save it
packaging/npm/scripts/publish.sh           # dry run
packaging/npm/scripts/publish.sh --execute
unset NODE_AUTH_TOKEN
```

The script refuses unless the GitHub release assets for `0.1.0` exist and their checksums match.

**Verify** in a clean directory/container:

```sh
npm view vowp version                      # 0.1.0
npm install -g vowp
vowp --version                             # vowp 0.1.0
```

## 9. Post-publish verification

Run the README quickstart from a clean directory, with no VOWP state:

```sh
export HOME=$(mktemp -d)
mkdir -p "$HOME"
vowp init --name hello-vowp
vowp doctor
vowp --version
```

Then run the example if the release notes link it:

```sh
FAKE=1 examples/hello-vowp/run.sh
```

Check the public repository settings:

- [ ] Private vulnerability reporting is enabled and the advisory link works while logged out.
- [ ] Secret scanning and push protection are enabled.
- [ ] The default branch is `main`; only `main` exists (no old branches).
- [ ] The release `v0.1.0` is published, is the latest release, and all assets download.
- [ ] crates.io shows all crates at `0.1.0`; npm shows `vowp@0.1.0`.
- [ ] The README says this is an early release and does not link to a site.
- [ ] The repository has no old history: `git log --oneline` on a fresh clone shows one commit before
      the release commits.
- [ ] The old private repository is still private and named `vowp-private`.

## 10. Rollback and irreversible actions

| Action | Can it be undone? | What to do |
|---|---|---|
| GitHub repository rename | Yes | Rename it back; old URLs redirect. |
| New public repository | Yes, destructively | Delete it before anyone depends on it; forks may survive. |
| Pushed public commit/tag | Yes, but visible | Delete the tag/release and force-push only if the repository is new and nobody has cloned; this is destructive. |
| crates.io version | **No** | Only `cargo yank --vers 0.1.0 <crate>`; the version stays downloadable by exact pin and the name is used forever. |
| npm version | Limited | `npm unpublish vowp@0.1.0` is time-limited and disruptive; prefer `npm deprecate`. |
| GitHub release | Yes | Edit or delete the release; the tag remains unless deleted. |

Never rewrite a published tag or a published crates.io version. If `0.1.0` is wrong, yank it and
publish `0.1.1` with a clear note.

## 11. Credential hygiene

- `gh auth status` is safe; never pass `--show-token`.
- Use `CARGO_REGISTRY_TOKEN` and `NODE_AUTH_TOKEN` in the environment; never write them to `.env`,
  `.cargo/credentials`, `.npmrc` or shell history. The p04/p05 scripts create and delete temporary
  config files themselves.
- Do not paste command output that contains a token. The audit scanner masks findings for a reason.
- After publishing, unset the variables and close the shell.
- If a token leaks, revoke it at the provider, not in this repository.
