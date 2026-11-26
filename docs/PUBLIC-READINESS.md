# Public readiness — StackSpirit monorepo

- **Audit date:** 2026-11-26
- **Repository:** `Stack-Spirit/stackspirit` (MIT, public launch pending)
- **Audited tree:** the job branch `job/ss-137-public-launch-scrub-and-v060` based on `57fa1d6`
- **Scope:** every tracked text file in the working tree, including `apps/`, `packages/`,
  `crates/`, `runtime/`, `docs/`, `design/`, `engine/`, `scripts/`, `.github/` and the root files
- **Nothing was published, tagged, pushed, or made public by this job.**

## Verdict

**Not ready to flip the repository public yet.** The scan still finds **no secret, no public IP
address, no tracked `.env` or credential file, and no personal e-mail address**. It now finds
**20 non-placeholder `/home/<user>` paths**, down from 49 at the start of this job:

- this job replaced **29** internal home paths with `/home/user` in `docs/` and
  `runtime/docs/maintainers/` (the files it owns);
- **20** findings remain in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
  `crates/spirit-core/tests/history.rs`, which are outside this job's `owns` list. They need one
  follow-up scrub job, or an ownership change, before the tree is public.

The release version is now aligned at **`0.6.0`** across `runtime/Cargo.toml`, every inter-crate
dependency, `runtime/Cargo.lock`, the version test and the npm/PyPI packaging manifests.
`check-release.sh --version 0.6.0 --tag v0.6.0 --publish` passes.

| Area | Result |
|---|---|
| Provider API keys, GitHub/AWS/Slack/Stripe/PyPI/npm tokens, private-key headers | **0** |
| Globally routable IPv4 addresses | **0** |
| Tracked `.env`, `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, SSH private-key names | **0** |
| Personal e-mail addresses outside the reserved example domains | **0** |
| Non-placeholder `/home/<user>` or `/Users/<user>` paths | **20 blocked** (6 in `AGENTS.md`, 2 in `README.md`, 4 in `crates/spirit-core/src/`, 8 in `crates/spirit-core/tests/history.rs`) |
| Home paths scrubbed by this job | **29** (21 in `docs/`, 8 in `runtime/docs/maintainers/`) |
| Placeholder/service/home-fixture paths recognised by the scan | 456 home paths, 820 reserved/private IPs and 135 reserved e-mails allowed |
| Working-tree scan exit status | **1** (the 20 remaining home paths; the command is shown below) |
| Release workflow YAML | **Valid**; the 76 structural, mutation and version-checker checks recorded by the ss-135 audit pass |
| Git history | **Not scanned by this script**; a history-aware scan is a required pre-flight item |
| First release version state | **Aligned at `0.6.0`**: workspace, inter-crate dependencies, lockfile, version test and npm/PyPI packaging agree; `check-release.sh` passes |

The path blocker and the history/publication checks are independent. Fixing the last 20 paths does
not prove the git history is clean, and the history scan does not replace the path scrub.

## How to reproduce the scan

```sh
bash scripts/check-public-readiness.sh
```

The script is a working-tree scan of `git ls-files`. It reads text files up to 4 MiB, skips
binary files, masks every matched value after its first four characters, and exits `1` on any
blocked finding. It uses only `git` and Python's standard library; it makes no network calls and
does not read `.env` values or any other file outside the checkout.

It checks:

| Rule | What it reports |
|---|---|
| `private_key` | PEM private-key headers. |
| provider/registry tokens | `sk-…`, GitHub, GitLab, AWS, Google, Slack, Stripe live, PyPI, npm and Hugging Face token shapes. |
| `secret_path` | Tracked `.env` (except `.env.example`/`.sample`/`.template`), `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, `*.p12`, `*.pfx`, `*.key`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, and `.ssh/` paths. |
| `ipv4` | IPv4 addresses that are globally routable. Loopback, RFC 1918/6598 private space, link-local, multicast, unspecified and the RFC 5737 documentation ranges are recognised as non-public. |
| `home_path` | `/home/<user>` and `/Users/<user>` outside the documented placeholder/service accounts. The scan does not match `.../home/...` inside a relative fixture path. |
| `email` | Addresses outside reserved example domains, `@users.noreply.github.com`, the project's own `stackspirit.io` addresses, and obvious file/systemd-unit false positives. |

Intentional fixtures and placeholders are allowed by name: `/home/user`, `/home/you`,
`/home/owner`, `/home/fake`, `/Users/me`, `/Users/example`, the documented `/home/vowp` and
`/home/vowp-vault` service accounts, and the acceptance-rig accounts. A new real home path is
still reported.

## Exact scan output at the end of this job

```text
$ bash scripts/check-public-readiness.sh
public-readiness: repository <git top level>
public-readiness: scope: tracked working tree at HEAD
public-readiness: scanned 1604 of 1650 tracked text files
public-readiness: allowed placeholders: 456 home path(s), 820 reserved/private IP(s), 135 reserved e-mail(s)
public-readiness: personal /home or /Users path: 20 blocked
public-readiness: findings (values masked):
  AGENTS.md:7: home_path: /hom***
  AGENTS.md:11: home_path: /hom***
  AGENTS.md:12: home_path: /hom***
  AGENTS.md:12: home_path: /hom***
  AGENTS.md:13: home_path: /hom***
  AGENTS.md:14: home_path: /hom***
  README.md:198: home_path: /hom***
  README.md:201: home_path: /hom***
  crates/spirit-core/src/lib.rs:5: home_path: /hom***
  crates/spirit-core/src/vowp/client.rs:2633: home_path: /hom***
  crates/spirit-core/src/vowp/client.rs:2657: home_path: /hom***
  crates/spirit-core/src/wire/mod.rs:3: home_path: /hom***
  crates/spirit-core/tests/history.rs:110: home_path: /hom***
  crates/spirit-core/tests/history.rs:112: home_path: /hom***
  crates/spirit-core/tests/history.rs:132: home_path: /hom***
  crates/spirit-core/tests/history.rs:161: home_path: /hom***
  crates/spirit-core/tests/history.rs:162: home_path: /hom***
  crates/spirit-core/tests/history.rs:165: home_path: /hom***
  crates/spirit-core/tests/history.rs:166: home_path: /hom***
  crates/spirit-core/tests/history.rs:183: home_path: /hom***
public-readiness: FAIL: 20 blocked finding(s)
```

## Home-path scrub status

Every blocked value is the same internal development account: the masked value is `/hom***`.
No password, key, token, host, or IP is involved. The original ss-135 audit found 41 findings;
the history work merged after that audit added 8 more in `crates/spirit-core/tests/history.rs`,
so this job started from **49**. It scrubbed **29** and leaves **20**.

### Scrubbed by this job (29)

| File | Findings | Replacement |
|---|---:|---|
| `docs/PLAN.md` | 10 | `/home/user` |
| `docs/VOWP-CONTRACT.md` | 9 | `/home/user` |
| `docs/CLUSTER.md` | 1 | `/home/user` |
| `docs/ORCHESTRATOR-CHAT.md` | 1 | `/home/user` |
| `runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md` | 7 | `/home/user` |
| `runtime/docs/maintainers/v0.2-acceptance/public-tree-audit.md` | 1 | `/home/user` |

### Remaining after this job (20)

| File | Findings | What is left |
|---|---:|---|
| `AGENTS.md` | 6 | The companion VOWP checkout path in the worker instructions. |
| `README.md` | 2 | The same private cross-project checkout. |
| `crates/spirit-core/src/lib.rs`, `vowp/client.rs`, `wire/mod.rs` | 4 | Code comments and one unit-test input. |
| `crates/spirit-core/tests/history.rs` | 8 | History fixture worktree, archive and sample paths. |

These paths are in files outside this job's `owns` list, so this job did not edit them. The follow-up
worker should replace every remaining value with `/home/user` (the same substitution this job used)
and re-run the scan until it prints `PASS` and exits `0`. Do not copy a real path into this document,
a commit message, or a release note.

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

Remaining release-note work, outside this job's `owns`: `runtime/CHANGELOG.md` still describes
`0.5.1-dev` and names `0.4.0` as the latest public release. The release workflow uses
`runtime/docs/release/<version>.md` and falls back to `runtime/CHANGELOG.md`; add
`runtime/docs/release/0.6.0.md` or refresh the changelog before publishing the draft.

## Owner pre-flight checklist

Complete these in order. The remaining path scrub and the history scan block the visibility
change; the release-note and dry-build items block the first release.

### 1. Remove private data from the tree

- [x] Scrub `docs/` and `runtime/docs/maintainers/` (29 paths replaced with `/home/user`).
- [ ] Scrub the 20 paths in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
      `crates/spirit-core/tests/history.rs` with a follow-up job that owns those files. Re-run
      `bash scripts/check-public-readiness.sh` and require `PASS` and exit `0`.
- [ ] Decide whether `runtime/docs/maintainers/` ships at all. It no longer contains private home
      paths, but it still holds old acceptance transcripts and internal runbooks.
- [ ] Run a history-aware secret scan over every commit that will be public. This script only
      scans the tracked working tree. From `runtime/`, the existing
      `scripts/audit/scan.sh --history --strict-history` is one option; a dedicated scanner such
      as `gitleaks` is also acceptable. Require zero blocked findings.
- [ ] Confirm no `.env`, private key, token, SSH key, credential file, database, or local log is
      tracked in any commit that will be public.
- [ ] Review tracked media/large files and third-party notices (`LICENSE`,
      `THIRD_PARTY_NOTICES.md`, `runtime/THIRD-PARTY-LICENSES.md`). All runtime release
      archives carry the licence bundle.

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
- [ ] Check the packaging metadata no longer points at the old `syver777/vowp` repository
      (the PyPI backend URLs, platform `package.json` repository fields and Homebrew formulas);
      point them at `Stack-Spirit/stackspirit`.
- [ ] Confirm the default branch is `main`, no unintended branches or tags will be published,
      and issues/discussions settings match the launch decision.

### 3. Finish the first release version

- [x] Align `runtime/Cargo.toml`, every inter-crate dependency, `runtime/Cargo.lock` and the
      version test at `0.6.0`; the npm/PyPI manifests already said `0.6.0`.
- [x] Run `bash runtime/scripts/release/check-release.sh --version 0.6.0 --tag v0.6.0 --publish`
      and get `check-release: ok (0.6.0)`.
- [ ] Add `runtime/docs/release/0.6.0.md` or refresh `runtime/CHANGELOG.md` for the release
      workflow's notes step.
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

- It does not prove the repository history is clean.
- It does not inspect GitHub issues, pull requests, wikis, releases or package-registry
  metadata.
- It does not run the desktop, Web, Rust, runtime, npm or Playwright test suites; this job's
  evidence is the public-readiness scan, the version test, `check-release.sh` and the
  root TypeScript typecheck/lint/unit suite.
- It does not publish, tag, push, sign, notarize, or change repository visibility.
- A clean scan is necessary but not sufficient for a public launch; the manual review and the
  owner settings above remain required.
