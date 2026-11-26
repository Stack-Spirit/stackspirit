# Public readiness — StackSpirit monorepo

- **Audit date:** 2026-11-26
- **Repository:** `Stack-Spirit/stackspirit` (MIT, public launch pending)
- **Audited tree:** the job branch `job/ss-137-public-launch-scrub-and-v060` based on `57fa1d6`
- **Scope:** every tracked text file in the working tree, including `apps/`, `packages/`,
  `crates/`, `runtime/`, `docs/`, `design/`, `engine/`, `scripts/`, `.github/` and the root files
- **Nothing was published, tagged, pushed, or made public by this job.**

## Verdict

**The working-tree scan is clean and the release version is aligned.** The scan finds **no secret,
no public IP address, no tracked `.env` or credential file, no personal e-mail address, and no
non-placeholder `/home/<user>` path**. All **49** findings at the start of this job were replaced
with `/home/user`:

- **29** in the `docs/` and `runtime/docs/maintainers/` files this job already owned;
- **20** in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
  `crates/spirit-core/tests/history.rs`, added to this job's `owns` by the owner.

The release version is now aligned at **`0.6.0`** across `runtime/Cargo.toml`, every inter-crate
dependency, `runtime/Cargo.lock`, the version test, `runtime/CHANGELOG.md` and the npm/PyPI
packaging manifests. `check-release.sh --version 0.6.0 --tag v0.6.0 --publish` passes.

The remaining launch pre-flight items are the history-aware secret scan, the GitHub repository and
registry settings, and the dry release build; they are not working-tree findings.

| Area | Result |
|---|---|
| Provider API keys, GitHub/AWS/Slack/Stripe/PyPI/npm tokens, private-key headers | **0** |
| Globally routable IPv4 addresses | **0** |
| Tracked `.env`, `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, SSH private-key names | **0** |
| Personal e-mail addresses outside the reserved example domains | **0** |
| Non-placeholder `/home/<user>` or `/Users/<user>` paths | **0 blocked** (49 replaced with `/home/user` by this job) |
| Home paths scrubbed by this job | **49** (29 in `docs/` and `runtime/docs/`, 20 in `AGENTS.md`, `README.md` and `crates/spirit-core/`) |
| Placeholder/service/home-fixture paths recognised by the scan | 483 home paths, 820 reserved/private IPs and 135 reserved e-mails allowed |
| Working-tree scan exit status | **0 (PASS)**; the command and output are shown below |
| Release workflow YAML | **Valid**; the 76 structural, mutation and version-checker checks recorded by the ss-135 audit pass |
| Git history | **Not scanned by this script**; a history-aware scan is a required pre-flight item |
| First release version state | **Aligned at `0.6.0`**: workspace, inter-crate dependencies, lockfile, version test and npm/PyPI packaging agree; `check-release.sh` passes |

The working-tree scan and the history/publication checks are independent. A clean scan does not
prove the git history is clean, and the history scan does not replace the path scrub.

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
public-readiness: allowed placeholders: 483 home path(s), 820 reserved/private IP(s), 135 reserved e-mail(s)
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

Complete these in order. The history scan and the repository settings block the visibility
change; the dry-build items block the first release.

### 1. Remove private data from the tree

- [x] Scrub `docs/` and `runtime/docs/maintainers/` (29 paths replaced with `/home/user`).
- [x] Scrub the 20 paths in `AGENTS.md`, `README.md`, `crates/spirit-core/src/` and
      `crates/spirit-core/tests/history.rs` with `/home/user`; `bash scripts/check-public-readiness.sh`
      now prints `PASS` and exits `0`.
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

- It does not prove the repository history is clean.
- It does not inspect GitHub issues, pull requests, wikis, releases or package-registry
  metadata.
- It does not run the Playwright end-to-end suites or the full runtime workspace; this job's
  evidence is the public-readiness scan, the `spirit-core` tests, the version test,
  `check-release.sh` and the root TypeScript typecheck/lint/unit suite.
- It does not publish, tag, push, sign, notarize, or change repository visibility.
- A clean scan is necessary but not sufficient for a public launch; the manual review and the
  owner settings above remain required.
