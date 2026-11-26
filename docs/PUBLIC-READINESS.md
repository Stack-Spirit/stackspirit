# Public readiness — StackSpirit monorepo

- **Audit date:** 2026-11-26
- **Repository:** `Stack-Spirit/stackspirit` (MIT, public launch pending)
- **Audited tree:** the job branch `job/ss-135-root-release-and-public-audit` based on `1d777b8`
- **Scope:** every tracked text file in the working tree, including `apps/`, `packages/`,
  `crates/`, `runtime/`, `docs/`, `design/`, `engine/`, `scripts/`, `.github/` and the root files
- **Nothing was published, tagged, pushed, or made public by this job.**

## Verdict

**Not ready to flip the repository public yet.** The scan found **no secret, no public IP
address, no tracked `.env` or credential file, and no personal e-mail address**. It did find
**41 non-placeholder `/home/<user>` paths** in tracked files. They are internal development and
acceptance paths, not credentials, but they would expose the private build environment if the
tree were published as-is.

| Area | Result |
|---|---|
| Provider API keys, GitHub/AWS/Slack/Stripe/PyPI/npm tokens, private-key headers | **0** |
| Globally routable IPv4 addresses | **0** |
| Tracked `.env`, `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `*.pem`, SSH private-key names | **0** |
| Personal e-mail addresses outside the reserved example domains | **0** |
| Non-placeholder `/home/<user>` or `/Users/<user>` paths | **41 blocked** (33 in root docs/comments, 8 in `runtime/docs/maintainers/`) |
| Placeholder/service/home-fixture paths recognised by the scan | 420 home paths, 820 reserved/private IPs and 135 reserved e-mails allowed |
| Working-tree scan exit status | **1** (blocked home paths; the command is shown below) |
| Release workflow YAML | **Valid**; 76 structural, mutation and version-checker checks pass |
| Git history | **Not scanned by this script**; a history-aware scan is a required pre-flight item |
| First release version state | **Not ready to tag**: `runtime/Cargo.toml` says `0.5.1-dev`, while the npm/PyPI packaging manifests say `0.6.0` |

The two release blockers and the publication blockers are independent. Fixing the 41 paths
does not make the version state consistent, and bumping the version does not remove the path
findings.

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

## Exact scan output

```text
$ bash scripts/check-public-readiness.sh
public-readiness: repository <git top level>
public-readiness: scope: tracked working tree at HEAD
public-readiness: scanned 1603 of 1649 tracked text files
public-readiness: allowed placeholders: 420 home path(s), 820 reserved/private IP(s), 135 reserved e-mail(s)
public-readiness: personal /home or /Users path: 41 blocked
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
  docs/CLUSTER.md:13: home_path: /hom***
  docs/ORCHESTRATOR-CHAT.md:50: home_path: /hom***
  docs/PLAN.md:7: home_path: /hom***
  docs/PLAN.md:36: home_path: /hom***
  docs/PLAN.md:135: home_path: /hom***
  docs/PLAN.md:153: home_path: /hom***
  docs/PLAN.md:293: home_path: /hom***
  docs/PLAN.md:1783: home_path: /hom***
  docs/PLAN.md:1787: home_path: /hom***
  docs/PLAN.md:2665: home_path: /hom***
  docs/PLAN.md:2667: home_path: /hom***
  docs/PLAN.md:2669: home_path: /hom***
  docs/VOWP-CONTRACT.md:18: home_path: /hom***
  docs/VOWP-CONTRACT.md:23: home_path: /hom***
  docs/VOWP-CONTRACT.md:25: home_path: /hom***
  docs/VOWP-CONTRACT.md:27: home_path: /hom***
  docs/VOWP-CONTRACT.md:28: home_path: /hom***
  docs/VOWP-CONTRACT.md:1984: home_path: /hom***
  docs/VOWP-CONTRACT.md:2201: home_path: /hom***
  docs/VOWP-CONTRACT.md:2203: home_path: /hom***
  docs/VOWP-CONTRACT.md:2205: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:481: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:508: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:512: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:538: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:621: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:722: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md:911: home_path: /hom***
  runtime/docs/maintainers/v0.2-acceptance/public-tree-audit.md:48: home_path: /hom***
public-readiness: FAIL: 41 blocked finding(s)
```

## The 41 blocked paths

Every blocked value is the same internal development account: the masked value is `/hom***`.
No password, key, token, host, or IP is involved. They fall into two groups.

| Location | Findings | Why it is a blocker |
|---|---:|---|
| `AGENTS.md` | 6 | Points at the companion VOWP checkout on the private build machine. |
| `README.md` | 2 | Documents the same private cross-project checkout. |
| `crates/spirit-core/src/lib.rs`, `crates/spirit-core/src/vowp/client.rs`, `crates/spirit-core/src/wire/mod.rs` | 4 | Code comments and test inputs use the private checkout/user path. |
| `docs/CLUSTER.md`, `docs/ORCHESTRATOR-CHAT.md` | 2 | Describe the local build machine and its installed CLI. |
| `docs/PLAN.md` | 10 | Private cross-project dependency notes. |
| `docs/VOWP-CONTRACT.md` | 9 | Private cross-project contract references. |
| `runtime/docs/maintainers/v0.2-acceptance/fresh-setup.md` | 7 | Acceptance run transcript with the login account and home. |
| `runtime/docs/maintainers/v0.2-acceptance/public-tree-audit.md` | 1 | Old internal worker-report path. |

**Fix before publication:** replace the internal account with the conventional
`/home/user` placeholder, or remove/prune the internal acceptance and cross-project
planning pages. Do not copy the real path into this document, a commit message, or a release
note. Re-run the scan until it exits `0`. The owning jobs for those files are outside this
job's `owns` list, so this job records the findings rather than editing them.

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

### Workflow validation run

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
the `check-release.sh` mutation cases.

### Current version state (must be fixed before the first tag)

`runtime/Cargo.toml` and its inter-crate dependencies currently say `0.5.1-dev`; the npm
platform/root templates and the PyPI `pyproject.toml` say `0.6.0`. A `v0.6.0` tag would fail
`check-release.sh` until they are aligned. Set one final `X.Y.Z` everywhere, refresh
`runtime/Cargo.lock`, add `runtime/docs/release/X.Y.Z.md` (or accept the `CHANGELOG.md`
fallback), then run:

```sh
bash runtime/scripts/release/check-release.sh --version X.Y.Z --tag vX.Y.Z --publish
```

## Owner pre-flight checklist

Complete these in order. The first two groups block the visibility change; the version and
publish groups block the first release.

### 1. Remove private data from the tree

- [ ] Resolve all 41 `home_path` findings listed above (use `/home/user` or remove the
      internal planning/acceptance pages). Re-run `bash scripts/check-public-readiness.sh`
      and require `PASS` and exit `0`.
- [ ] Decide whether `runtime/docs/maintainers/` ships at all. It contains old acceptance
      transcripts and internal runbooks; removing or pruning it resolves the eight runtime
      findings and most of the operational prose.
- [ ] Replace or remove the internal cross-project references in `AGENTS.md`, `README.md`,
      `docs/PLAN.md`, `docs/VOWP-CONTRACT.md`, `docs/CLUSTER.md` and the core comments.
- [ ] Run a history-aware secret scan over every commit that will be public. This script only
      scans the tracked working tree. From `runtime/`, the existing
      `scripts/audit/scan.sh --history --strict-history` is one option; a dedicated scanner
      such as `gitleaks` is also acceptable. Require zero blocked findings.
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

### 3. Align the first release version

- [ ] Choose the first public version (`X.Y.Z`). Align `runtime/Cargo.toml`, every inter-crate
      dependency, `runtime/packaging/npm/stackspirit`, all four
      `runtime/packaging/npm/platforms/*`, `runtime/packaging/pypi/stackspirit/pyproject.toml`,
      the Homebrew placeholders and `runtime/docs/release/X.Y.Z.md`.
- [ ] Refresh `runtime/Cargo.lock` with a normal build; do not hand-edit it.
- [ ] Run `bash runtime/scripts/release/check-release.sh --version X.Y.Z --tag vX.Y.Z --publish`
      and require `check-release: ok (X.Y.Z)`.
- [ ] Run the release workflow as a dry build:
      `gh workflow run release.yml --ref main -f version=X.Y.Z`; inspect the archives,
      npm tarballs, wheels, `SHA256SUMS` and both `--version` outputs. A dry build must not
      create a tag or release.

### 4. Tag, draft, publish, verify

- [ ] Tag `vX.Y.Z` on a commit already on `main`; confirm the run creates a **draft** release
      named `StackSpirit X.Y.Z`.
- [ ] Download the draft assets, verify `SHA256SUMS`, inspect both archive layouts, and run
      the matching binary (`stackspirit --version` and `vowp --version`).
- [ ] Publish the draft only after that review. Approve the `pypi` and `npm` environments so
      the OIDC publish jobs run.
- [ ] Verify `https://pypi.org/pypi/stackspirit/X.Y.Z/json` and
      `npm view stackspirit@X.Y.Z version` (plus the four platform packages).
- [ ] Confirm the clean-runner `npx`/`uvx` smoke jobs passed, and that the release is marked
      latest.
- [ ] Only after the release and checks are green should the repository visibility be changed.

## What this audit does not claim

- It does not prove the repository history is clean.
- It does not inspect GitHub issues, pull requests, wikis, releases or package-registry
  metadata.
- It does not run the desktop, Web, Rust, runtime, npm or Playwright test suites; its release
  evidence is the workflow structure test and the version checker shown above.
- It does not publish, tag, push, sign, notarize, or change repository visibility.
- A clean scan is necessary but not sufficient for a public launch; the manual review and the
  owner settings above remain required.
