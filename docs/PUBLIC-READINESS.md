# Public readiness — StackSpirit monorepo

- **Audit date:** 2026-11-26
- **History scan:** 2026-11-27, job `ss-144-history-secret-scan`, at commit `3c8ac2f`
- **Public-history candidate:** 2026-11-27, job `ss-149-public-history-candidate`, at commit
  `68b81cd`; candidate commit `880e483` scans clean
- **Repository:** `Stack-Spirit/stackspirit` (MIT, public launch pending)
- **Audited tree:** the job branch `job/ss-137-public-launch-scrub-and-v060` based on `57fa1d6`
- **Scope:** every tracked text file in the working tree, including `apps/`, `packages/`,
  `crates/`, `runtime/`, `docs/`, `design/`, `engine/`, `scripts/`, `.github/` and the root files
- **Nothing was published, tagged, pushed, or made public by this job.**

## Verdict

**The private history stays private; the public branch is one clean orphan commit.** The
working-tree scan finds **no secret, no public IP address, no tracked `.env` or credential file,
no personal e-mail address, and no non-placeholder `/home/<user>` path**. The history-aware scan
walks every commit reachable from `git rev-list --all` and still finds **9 secret-class matches**
and **486 privacy-class matches** in old commits; see
[History scan](#history-scan-job-ss-144-history-secret-scan-2026-11-27) under §1. That history
must not be published, so this job implements the owner-approved model (2026-11-09): the private
repository and its history stay private, and the public repository starts with a single commit.

`scripts/prepare-public-history.sh` builds that commit as one parentless commit of the current
`HEAD` tree and immediately scans the exact tree with `scripts/check-public-readiness.sh --rev`.
At commit `68b81cd` the candidate commit `880e483` scans **clean: 0 blocked findings, exit 0**
(1717 blobs, 1 over the size cap and 51 binary/undecodable files skipped by the same limits as the
working-tree scan). The exact `--rev public-main` run is recorded in
[Candidate public commit](#candidate-public-commit-job-ss-149-2026-11-27) under §1. Pushing is the
owner's action; this job pushed, tagged, and published nothing, so the publication checkbox stays
unchecked.

All **49** working-tree findings at the start of the ss-137 job were replaced with
`/home/user`:

- **29** in the `docs/` and `runtime/docs/maintainers/` files this job already owned;
- **20** in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
  `crates/spirit-core/tests/history.rs`, added to this job's `owns` by the owner.

The release version is now aligned at **`0.6.0`** across `runtime/Cargo.toml`, every inter-crate
dependency, `runtime/Cargo.lock`, the version test, `runtime/CHANGELOG.md` and the npm/PyPI
packaging manifests. `check-release.sh --version 0.6.0 --tag v0.6.0 --publish` passes.

The remaining launch pre-flight items are the owner's GitHub repository and registry settings,
the dry release build, and the actual push of the reviewed `public-main` commit; they are not
working-tree findings.

| Area | Result |
|---|---|
| Provider API keys, GitHub/AWS/Slack/Stripe/PyPI/npm tokens, private-key headers | **0** in the working tree and in the candidate public commit; **9** in the private history (do not publish; see §1) |
| Globally routable IPv4 addresses | **0** |
| Tracked `.env`, `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, SSH private-key names | **0** |
| Personal e-mail addresses outside the reserved example domains | **0** |
| Non-placeholder `/home/<user>` or `/Users/<user>` paths | **0 blocked** in the working tree and in the candidate public commit (49 replaced with `/home/user` by the ss-137 job); **486** in the private history (do not publish) |
| Home paths scrubbed by this job | **49** (29 in `docs/` and `runtime/docs/`, 20 in `AGENTS.md`, `README.md` and `crates/spirit-core/`) |
| Placeholder/service/home-fixture paths recognised by the scan | 487 home paths, 856 reserved/private IPs and 160 reserved e-mails allowed at the ss-144 re-run; 487/856/162 at the ss-149 candidate run |
| Working-tree scan exit status | **0 (PASS)** at the ss-144 and ss-149 re-runs; the command and output are shown below |
| Release workflow YAML | **Valid**; the 76 structural, mutation and version-checker checks recorded by the ss-135 audit pass |
| Private git history | **Do not publish**: the ss-144 history scan found **9 secret-class** and **486 privacy-class** findings; see the History scan subsection in §1 |
| Candidate public commit (`public-main`, `880e483`) | **PASS**: `--rev` scans the exact tree that would be published and finds **0** blocked findings (1717 blobs, exit 0); see the Candidate public commit subsection in §1 |
| First release version state | **Aligned at `0.6.0`**: workspace, inter-crate dependencies, lockfile, version test and npm/PyPI packaging agree; `check-release.sh` passes |

The working-tree scan and the history/publication checks are independent. A clean scan does not
prove the git history is clean, and the history scan does not replace the path scrub.

## How to reproduce the scan

```sh
bash scripts/check-public-readiness.sh                 # working tree at HEAD
bash scripts/check-public-readiness.sh --history       # every commit in git rev-list --all
bash scripts/check-public-readiness.sh --history --strict-history
bash scripts/check-public-readiness.sh --rev <commit>  # the tree of one commit
bash scripts/prepare-public-history.sh --dry-run       # compute the candidate, create no branch
bash scripts/prepare-public-history.sh                 # create/update local public-main
bash scripts/check-public-readiness-history.test.sh    # hermetic self-test
```

The default mode is a working-tree scan of `git ls-files`. `--history` walks every commit
reachable from `git rev-list --all`, deduplicates blobs by object id, and applies the same rules
and the same 4 MiB/binary limits. Secret-class findings (`private_key`, provider/registry tokens
and `secret_path`) are fatal in `--history`; privacy-class findings (`home_path`, `ipv4` and
`email`) are reported with counts and masked rows but are fatal only with `--strict-history`.
`--strict-history` implies `--history`.

`--rev <commit>` scans the tree of exactly one commit, not the working tree, with the same rules,
masking, size/binary limits and placeholder allowlists. Every finding class is fatal there because
that tree is what a public branch would publish; `--rev` is mutually exclusive with `--history`
and `--strict-history`. It prints the full commit and tree ids and the file/blob counts so the
evidence names the exact object that was scanned.

`scripts/prepare-public-history.sh` refuses a dirty working tree, a missing `HEAD` and a missing
scanner. It builds one parentless commit from the current `HEAD` tree with the public identity
`Syver-August <92798051+syver777@users.noreply.github.com>` and the message `StackSpirit 0.6.0`
(`--message` overrides it). Without `--dry-run` it creates the local `public-main` branch, or
replaces it only with `--force`, then runs `check-public-readiness.sh --rev public-main` and exits
non-zero if the scan fails. It never pushes, runs `git filter-branch`, rewrites `main`, or deletes
a branch or tag.

The script reads text files up to 4 MiB, skips binary files, masks every matched value after its
first four characters, and uses only `git` and Python's standard library; it makes no network
calls and does not read `.env` values or any other file outside the checkout.

It checks:

| Rule | What it reports |
|---|---|
| `private_key` | PEM private-key headers. |
| provider/registry tokens | `sk-…`, GitHub, GitLab, AWS, Google, Slack, Stripe live, PyPI, npm and Hugging Face token shapes. |
| `secret_path` | Tracked `.env` (except `.env.example`/`.sample`/`.template`), `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, `*.p12`, `*.pfx`, `*.key`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, and `.ssh/` paths. |
| `ipv4` | IPv4 addresses that are globally routable. Loopback, RFC 1918/6598 private space, link-local, multicast, unspecified and the RFC 5737 documentation ranges are recognised as non-public. Decimal runs inside SVG `d`/`points` path data are not addresses. |
| `home_path` | `/home/<user>` and `/Users/<user>` outside the documented placeholder/service accounts. The scan does not match `.../home/...` inside a relative fixture path. |
| `email` | Addresses outside reserved example domains, `@users.noreply.github.com`, the project's own `stackspirit.io` addresses (including subdomains), and obvious file/systemd-unit false positives. |

Intentional fixtures and placeholders are allowed by name: `/home/user`, `/home/you`,
`/home/owner`, `/home/fake`, `/Users/me`, `/Users/example`, the documented `/home/stackspirit`,
`/home/vowp` and `/home/vowp-vault` service accounts, and the acceptance-rig accounts. A new real
home path is still reported.

## Exact working-tree scan output (ss-144 re-run)

```text
$ bash scripts/check-public-readiness.sh
public-readiness: repository <git top level>
public-readiness: scope: tracked working tree at HEAD
public-readiness: scanned 1663 of 1715 tracked text files
public-readiness: allowed placeholders: 487 home path(s), 856 reserved/private IP(s), 160 reserved e-mail(s)
public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found
```

## Home-path scrub status

Every blocked value was the same internal development account: the masked value was `/hom***`.
No password, key, token, host, or IP was involved. The original ss-135 audit found 41 findings;
the history work merged after that audit added 8 more in `crates/spirit-core/tests/history.rs`,
so this job started from **49**. All **49** now use the `/home/user` placeholder.

### Documentation (29)

| File | Findings | Replacement |
|---|---:|---|
| `docs/PLAN.md` | 10 | `/home/user` |
| `docs/VOWP-CONTRACT.md` | 9 | `/home/user` |
| `docs/CLUSTER.md` | 1 | `/home/user` |
| `docs/ORCHESTRATOR-CHAT.md` | 1 | `/home/user` |
| `runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md` | 7 | `/home/user` |
| `runtime/docs/maintainers/v0.2-acceptance/public-tree-audit.md` | 1 | `/home/user` |

### Root docs, comments and fixtures (20)

| File | Findings | Replacement |
|---|---:|---|
| `AGENTS.md` | 6 | `/home/user` |
| `README.md` | 2 | `/home/user` |
| `crates/spirit-core/src/lib.rs`, `vowp/client.rs`, `wire/mod.rs` | 4 | `/home/user` |
| `crates/spirit-core/tests/history.rs` | 8 | `/home/user` |

No real home path remains in the tracked tree. Keep using the `/home/user` placeholder in new docs,
test fixtures and commit messages rather than copying a path from a developer machine.

## Release workflow

The root [`.github/workflows/release.yml`](../.github/workflows/release.yml) is adapted from
`runtime/.github/workflows/release.yml` for the monorepo:

- a `v*` tag builds the four native runtime targets, both CLI names and both daemons, the
  `stackspirit` and legacy `vowp` archives, the npm platform/root tarballs and the four
  platform wheels;
- the build creates a **draft** GitHub Release only – no registry is touched on a tag;
- publishing that draft triggers `publish-npm` (`environment: npm`) and `publish-pypi`
  (`environment: pypi`) with GitHub OIDC trusted publishing, plus the `npx`/`uvx` install
  smoke on clean runners;
- a manual `workflow_dispatch` is a dry build; `publish=true` only publishes an
  already-published release and still waits for environment approval;
- every third-party action is pinned to a 40-character commit SHA; the workflow contains no
  `secrets.*` reference and no registry token, password, or API key.

The release version is read from `runtime/Cargo.toml`, and
`runtime/scripts/release/check-release.sh` must agree with the tag, every npm package,
the PyPI project and the Homebrew placeholders. The workflow builds from `runtime/` with
`working-directory: runtime` and uploads/downloads artifact paths with the `runtime/` prefix.

### Workflow validation (recorded by the ss-135 audit)

```sh
python3 -I - <<'PY'
import yaml
with open('.github/workflows/release.yml', encoding='utf-8') as f:
    wf = yaml.safe_load(f)
print('yaml-ok', sorted(wf['jobs']))
PY

WORKFLOW=$PWD/.github/workflows/release.yml \
  bash runtime/scripts/release/test-workflow.sh
```

Results: the YAML parse printed
`yaml-ok ['build', 'checksums', 'github-release', 'prepare', 'publish-npm', 'publish-pypi', 'verify-install']`;
the workflow test printed `test-workflow: all checks passed` and exited `0`. The test covers
the OIDC-only permissions, absence of registry credentials, protected environments, the
draft-only release, SHA-pinned actions, the four-target matrix, the `npx`/`uvx` smoke tests and
the `check-release.sh` mutation cases. This job did not change the workflow and did not re-run
those checks.

## Version alignment (done by this job)

Before this job, `runtime/Cargo.toml` and its inter-crate dependencies said `0.5.1-dev`, while
the npm platform/root templates and the PyPI `pyproject.toml` said `0.6.0`; a `v0.6.0` tag would
have failed `check-release.sh`.

Now the workspace version, every inter-crate dependency, `runtime/Cargo.lock` and
`runtime/scripts/release/version-test.rs` say `0.6.0`, matching the npm/PyPI packaging. The
lockfile was refreshed with Cargo (not hand-edited).

```text
$ bash runtime/scripts/release/check-release.sh --version 0.6.0 --tag v0.6.0 --publish
check-release: ok (0.6.0)

$ (cd runtime && cargo test -p vowp-cli --test version)
running 1 test
test version_flag_prints_package_version ... ok
test result: ok. 1 passed; 0 failed
```

`runtime/CHANGELOG.md` now reports the `0.6.0` workspace version, so the release workflow's
`runtime/docs/release/<version>.md` → `runtime/CHANGELOG.md` fallback is current. A versioned
`runtime/docs/release/0.6.0.md` note remains optional; the changelog is the fallback the workflow
uses when that file is absent.

## Owner pre-flight checklist

Complete these in order. The private history stays private; the candidate public commit and the
repository settings gate the visibility change, and the dry-build items block the first release.

### 1. Remove private data from the tree

- [x] Scrub `docs/` and `runtime/docs/maintainers/` (29 paths replaced with `/home/user`).
- [x] Scrub the 20 paths in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
      `crates/spirit-core/tests/history.rs` with `/home/user`; `bash scripts/check-public-readiness.sh`
      now prints `PASS` and exits `0`.
- [ ] Decide whether `runtime/docs/maintainers/` ships at all. It no longer contains private home
      paths, but it still holds old acceptance transcripts and internal runbooks.
- [ ] Run a history-aware secret scan over every commit that will be public. This applies only if
      the private history is ever published; it must not be. The ss-144 `--history` scan found
      9 secret-class and 486 privacy-class rows (below), so this box stays unchecked. From
      `runtime/`, the existing `scripts/audit/scan.sh --history --strict-history` remains another
      option; a dedicated scanner such as `gitleaks` is also acceptable. Require zero blocked
      findings.
- [x] Build the candidate public history as one parentless commit and scan that exact tree:
      `bash scripts/prepare-public-history.sh`; `--rev public-main` finds 0 blocked findings at
      candidate commit `880e483` (see the Candidate public commit subsection below).
- [ ] Review the `public-main` branch and push it as the public `main` when the repository settings
      are ready. This is the owner action; the script never pushes, and the private history stays
      private. This box stays unchecked.
- [ ] Confirm no `.env`, private key, token, SSH key, credential file, database, or local log is
      tracked in the candidate public commit (the `--rev` scan reports 0).
- [ ] Review tracked media/large files and third-party notices (`LICENSE`,
      `THIRD_PARTY_NOTICES.md`, `runtime/THIRD-PARTY-LICENSES.md`). All runtime release
      archives carry the licence bundle.

#### History scan (job `ss-144-history-secret-scan`, 2026-11-27)

The scanner now has a history mode. It walks every commit reachable from `git rev-list --all`,
deduplicates blobs by object id, and applies the same rules and 4 MiB/binary limits as the
working-tree scan. Secret-class findings are fatal in `--history`; privacy-class findings are
reported with counts and masked rows but are fatal only with `--strict-history`.

Exact commands and results at commit `3c8ac2f` (wall-clock time 95.5 s for `--history`, 90.9 s
for the strict re-run):

```sh
bash scripts/check-public-readiness-history.test.sh
# history-test: PASS (exit 0)

bash scripts/check-public-readiness.sh
# scanned 1663 of 1715 tracked text files
# PASS (exit 0)

bash scripts/check-public-readiness.sh --history
# commits walked: 1180
# unique blobs: 6646 (6592 text scanned, 2 over size cap, 52 binary/undecodable)
# secret-class findings: 9 (private_key 3, provider_token 6)
# privacy-class findings: 486 (home_path 486, ipv4 0, email 0)
# exit status: 1

bash scripts/check-public-readiness.sh --history --strict-history
# same counts; exit status: 1
```

**Do not publish this private history.** Nine secret-class rows were found (all values masked
exactly as the scanner prints them):

```text
19ade78542bd crates/vowp-adapters/tests/classify.rs:237: private_key: ----***
a47b7aa9385c scripts/audit/allowlist.txt:44: private_key: ----***
a47b7aa9385c scripts/audit/allowlist.txt:48: private_key: ----***
f14d4cb4401e crates/spirit-core/tests/chat_permissions.rs:28: provider_token: sk-l***
ec75071ead97 crates/spirit-core/tests/chat_permissions.rs:28: provider_token: sk-l***
09ed8dd00b97 crates/spirit-core/tests/chat_permissions.rs:28: provider_token: sk-l***
ded09727f725 docs/ORCHESTRATOR-CHAT.md:2595: provider_token: sk-o***
e7ba19716402 crates/spirit-core/tests/worker_session.rs:130: provider_token: sk-a***
b951d5921c3a crates/spirit-core/tests/worker_session.rs:131: provider_token: sk-a***
```

These look like synthetic test fixtures, audit-allowlist text and an orchestrator-chat example
rather than live credentials, but the scanner cannot treat secret-shaped history as safe without
an owner decision. Four of the nine rows are in `main` ancestry (`19ade78542bd`, `a47b7aa9385c`
twice and `ded09727f725`); the other five are on local job branches
(`job/ss-81-chat-tool-activity-and-permissions` and
`job/ss-125-worker-live-session-and-takeover`) that are not ancestors of `main`. Because the scan
uses `--all`, those refs are included; they matter only if those branches are published.

The 486 privacy-class findings are all `home_path`; there is no public IPv4 address and no
personal e-mail in history. The scanner prints the first 200 rows and counts the rest. They are
the pre-0.6 `/home/<user>` and `/Users/<user>` paths in old docs, tests and fixtures, including
the ones later scrubbed from the working tree. Publishing the existing history would still expose
those paths, so the private history stays private and the public repository starts from a fresh
single commit.

`runtime/scripts/audit/squash-for-public.sh` is the runtime-only version of that path. The root
monorepo now has `scripts/prepare-public-history.sh`, which builds the local `public-main` branch
from the current tree without rewriting, rebasing or filtering the private history. This job ran
it; nothing was pushed.

#### Candidate public commit (job `ss-149-public-history-candidate`, 2026-11-27)

`scripts/prepare-public-history.sh` refuses a dirty working tree, a missing `HEAD` and a missing
scanner. It builds one parentless commit from the current `HEAD` tree with the public identity
`Syver-August <92798051+syver777@users.noreply.github.com>`, creates or force-replaces the local
`public-main` branch, and immediately runs the `--rev` scan. `--dry-run` computes the same tree and
commit and runs the same scan without creating the branch. The script never pushes, rewrites
`main`, runs `git filter-branch`, or deletes a branch or tag.

Exact commands and output at commit `68b81cd`:

```text
$ bash scripts/prepare-public-history.sh
prepare-public-history: repository <git top level>
prepare-public-history: source HEAD 68b81cda87f7baa37f2f28ba32b99a7550c094cf
prepare-public-history: tree 66f7f7a43dd8ddf959e819d800720e5fe472eb14 (1717 files)
prepare-public-history: commit 880e483eb5d8ae302a0d917161483714154956f9 (parentless)
prepare-public-history: author Syver-August <92798051+syver777@users.noreply.github.com>
prepare-public-history: committer Syver-August <92798051+syver777@users.noreply.github.com>
prepare-public-history: message: StackSpirit 0.6.0
prepare-public-history: branch public-main -> 880e483eb5d8ae302a0d917161483714154956f9
prepare-public-history: scanning public-main with scripts/check-public-readiness.sh --rev
public-readiness: repository <git top level>
public-readiness: scope: tree of one commit
public-readiness: revision: 880e483eb5d8ae302a0d917161483714154956f9
public-readiness: tree: 66f7f7a43dd8ddf959e819d800720e5fe472eb14 (1717 entries, 1717 blobs)
public-readiness: scanned 1665 of 1717 blobs in 880e483eb5d8 (1 over size cap, 51 binary/undecodable, 0 unreadable)
public-readiness: allowed placeholders: 487 home path(s), 856 reserved/private IP(s), 162 reserved e-mail(s)
public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found in the commit tree
prepare-public-history: public-readiness PASS
prepare-public-history: the working tree, main and every other ref are unchanged; nothing was pushed
prepare-public-history: inspect (owner):
  git log -1 --format=fuller public-main
  git show --stat public-main
  git diff --stat HEAD public-main
prepare-public-history: push (owner action, after review; this script never pushes):
  git push <remote> public-main:main
```

```text
$ bash scripts/check-public-readiness.sh --rev public-main
public-readiness: repository <git top level>
public-readiness: scope: tree of one commit
public-readiness: revision: 880e483eb5d8ae302a0d917161483714154956f9
public-readiness: tree: 66f7f7a43dd8ddf959e819d800720e5fe472eb14 (1717 entries, 1717 blobs)
public-readiness: scanned 1665 of 1717 blobs in 880e483eb5d8 (1 over size cap, 51 binary/undecodable, 0 unreadable)
public-readiness: allowed placeholders: 487 home path(s), 856 reserved/private IP(s), 162 reserved e-mail(s)
public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found in the commit tree
```

```text
$ bash scripts/check-public-readiness.sh --rev main
public-readiness: repository <git top level>
public-readiness: scope: tree of one commit
public-readiness: revision: e55e1f7523b211243f30ba020db21ad11d2934b5
public-readiness: tree: 045b8072c719b800e07c4d0b96f6dda1a4ef535d (1716 entries, 1716 blobs)
public-readiness: scanned 1664 of 1716 blobs in e55e1f7523b2 (1 over size cap, 51 binary/undecodable, 0 unreadable)
public-readiness: allowed placeholders: 487 home path(s), 856 reserved/private IP(s), 160 reserved e-mail(s)
public-readiness: PASS: no secret, public IP, personal home path, tracked .env or personal e-mail found in the commit tree
```

The candidate commit is parentless with the stated public identity and the `StackSpirit 0.6.0`
message, and its tree scans clean at exit 0. The 9 secret-class and 486 privacy-class rows remain
only in the private history; none of them is in this tree. The branch is local: this job pushed,
tagged, and published nothing, and did not delete a branch or tag. After this documentation update,
`bash scripts/prepare-public-history.sh --force` followed by
`bash scripts/check-public-readiness.sh --rev public-main` prints the same **0 blocked findings**
and `PASS` at the final job HEAD. The documentation commit adds no blocked finding; the permitted
e-mail placeholder count rises because this section names the public noreply identity.

### 2. Repository and account settings

- [ ] Set the final repository description, homepage, topics and social preview; keep the MIT
      licence visible.
- [ ] Enable private vulnerability reporting and check that `SECURITY.md` gives the current
      reporting route.
- [ ] Enable secret scanning and push protection; restrict Actions to the intended workflows.
- [ ] Create the `release`, `npm` and `pypi` GitHub environments with required reviewers and
      restrict them to `main`/release tags.
- [ ] Configure npm trusted publishers for `stackspirit` and the four `@stackspirit/*`
      platform packages, naming workflow `release.yml` and environment `npm`.
- [ ] Configure the PyPI trusted publisher for `stackspirit`, naming workflow `release.yml`
      and environment `pypi`.
- [x] Check the packaging metadata no longer points at the old `syver777/vowp` repository
      (the PyPI backend URLs, platform `package.json` repository fields and Homebrew formulas);
      point them at `Stack-Spirit/stackspirit`. Landed by `ss-145` in `9c2f379`.
- [ ] Confirm the default branch is `main`, no unintended branches or tags will be published,
      and issues/discussions settings match the launch decision.

### 3. Finish the first release version

- [x] Align `runtime/Cargo.toml`, every inter-crate dependency, `runtime/Cargo.lock` and the
      version test at `0.6.0`; the npm/PyPI manifests already said `0.6.0`.
- [x] Run `bash runtime/scripts/release/check-release.sh --version 0.6.0 --tag v0.6.0 --publish`
      and get `check-release: ok (0.6.0)`.
- [x] Refresh `runtime/CHANGELOG.md` for `0.6.0`; the release workflow's changelog fallback is
      current. A separate `runtime/docs/release/0.6.0.md` note is optional.
- [ ] Run the release workflow as a dry build:
      `gh workflow run release.yml --ref main -f version=0.6.0`; inspect the archives,
      npm tarballs, wheels, `SHA256SUMS` and both `--version` outputs. A dry build must not
      create a tag or release.

### 4. Tag, draft, publish, verify

- [ ] Tag `v0.6.0` on a commit already on `main`; confirm the run creates a **draft** release
      named `StackSpirit 0.6.0`.
- [ ] Download the draft assets, verify `SHA256SUMS`, inspect both archive layouts, and run
      the matching binary (`stackspirit --version` and `vowp --version`).
- [ ] Publish the draft only after that review. Approve the `pypi` and `npm` environments so
      the OIDC publish jobs run.
- [ ] Verify `https://pypi.org/pypi/stackspirit/0.6.0/json` and
      `npm view stackspirit@0.6.0 version` (plus the four platform packages).
- [ ] Confirm the clean-runner `npx`/`uvx` smoke jobs passed, and that the release is marked
      latest.
- [ ] Only after the release and checks are green should the repository visibility be changed.

## What this audit does not claim

- It does not make the private repository history clean: the ss-144 scan found **9 secret-class**
  and **486 privacy-class** matches in commits reachable from local refs (see §1), and that
  history must not be published. The clean result applies to the candidate orphan commit
  `880e483` built from `68b81cd`; rebuild and rescan `public-main` after any later change. It does
  not inspect remote-only refs that are not fetched locally.
- It does not push, publish, tag or change visibility. Pushing the reviewed `public-main` commit
  as the public `main` is the owner's action.
- It does not inspect GitHub issues, pull requests, wikis, releases or package-registry
  metadata.
- It does not run the Playwright end-to-end suites or the full runtime workspace; this job's
  evidence is the public-readiness scan, the `spirit-core` tests, the version test,
  `check-release.sh` and the root TypeScript typecheck/lint/unit suite.
- It does not publish, tag, push, sign, notarize, or change repository visibility.
- A clean scan is necessary but not sufficient for a public launch; the manual review and the
  owner settings above remain required.
