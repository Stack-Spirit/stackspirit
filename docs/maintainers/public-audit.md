# Public-readiness audit

- **Audit date:** 2026-11-09
- **Audited commit:** `d5f357d` on `job/p01-public-audit` (based on `origin/main`)
- **Scope:** tracked files in the working tree; every commit reachable from `git rev-list --all`;
  dependency licences; the `LICENSE` year/owner line; `SECURITY.md`; `.gitignore`.
- **Owner decision:** VOWP goes public as a clean repository **without the old history**. The private
  repository and its history stay private. `scripts/audit/squash-for-public.sh` creates the public
  branch; nothing is pushed or published by this job.

## Verdict

| Area | Result |
|---|---|
| p01-owned paths | Clean. No secret, private path or token shape in the current versions of the files this job owns. |
| Current tree | **Not yet publishable.** The scan found 14 real home-path occurrences in 6 files outside p01's `owns` (test fixtures and design docs). `scripts/audit/fix-unowned-private-paths.sh` is the staged owner fix; without it the scan exits 1. |
| Old history | 10,944 findings, of which 1,793 are real home paths. No real token, key, `.env`, SSH-key or large-blob finding. History is informational because `public-main` is a fresh commit; the old history is never published. |
| Licences | Clean: project MIT, `LICENSE` 2026 Syver-August Meyer, all 7 workspace crates MIT, 160 third-party packages within MIT/Apache-2.0/BSD/ISC/Unicode/Zlib. |
| Publication gate | `scripts/audit/scan.sh --worktree` and `scripts/audit/scan.sh --rev public-main` must both exit 0. The first one is currently red only because of the six unowned files. |

## What the scanner checks

`scripts/audit/scan.sh` is POSIX shell, uses only local `git`/`grep`/`awk`/`sed`, and makes no network
calls. It scans tracked files in the working tree and every commit from `git rev-list --all` for:

| Rule | What it matches |
|---|---|
| `ipv4` | IPv4 addresses (`127.0.0.1` is allowlisted as loopback). |
| `home_path` | `/home/<user>` and `/Users/<user>` paths. |
| `email` | E-mail addresses; `@users.noreply.github.com` is allowed by the brief and is skipped. |
| `token` | `sk-`, `sb_secret_`, `ghp_`, `github_pat_`, `AKIA`, PEM private-key headers, `xox[abp]-`. |
| `private_host` | Host-like names ending in `.local`, `.lan`, `.internal`, `.home.arpa` or `.ts.net`. |
| `ssh_host` | `ssh <user>@<host>` commands. |
| `secret_path` | Tracked paths named `.env*` (except `.env.example`), `*.pem`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, `authorized_keys`, or under `.ssh/`. |
| `large_blob` | Tracked blobs larger than 1 MiB (override with `--max-blob-bytes`). |

The scanner never prints a matched value in full: every finding shows `file:commit:line: rule:
<first four characters>***`. Values are checked against `scripts/audit/allowlist.txt`; every entry has a
`rule|path|value|reason` line and an empty reason is rejected. The current allowlist covers only
loopback, conventional placeholder paths, reserved `example.com`/`example.invalid` addresses, a Git SSH
`user@host` address, three false-positive Rust identifiers, and one fake PEM header in an old test
fixture.

## Findings: working tree

| Category | Before owner fix | After p01 + owner fix | Notes |
|---|---:|---:|---|
| IPv4 | 8 | 8 allowlisted | All `127.0.0.1` in security/design docs. |
| Real `/home/<user>` / `/Users/<user>` | 14 occurrences in 13 lines, 6 files | 0 | Outside p01 `owns`; staged fix script. |
| Placeholder home paths | 82 | 96 allowlisted | `/home/user`, `/Users/me`, `/home/you`; the 14 fixed paths join this group. |
| E-mail addresses | 34 | 34 allowlisted | Reserved test addresses plus the Git SSH `user@host` form. |
| Token shapes | 0 | 0 | No key or token in the current tree. |
| Private hostnames | 0 real; 12 false positives | 12 allowlisted | Rust/filename identifiers ending in `.local`, plus allowlist text. |
| SSH-key paths | 0 | 0 | No tracked key files. |
| `.env` paths | 0 | 0 | `.env` and `.env.*` are ignored; no tracked file. |
| Large blobs (>1 MiB) | 0 | 0 | Largest tracked blob is about 100 KB. |
| **Total** | **150 findings, 136 allowlisted, 14 blocked** | **150 allowlisted, 0 blocked** | The 14 are the six unowned files below. |

The six files that need the owner's edit (no private value is repeated here):

| File | Lines | Fix |
|---|---:|---|
| `crates/vowp-protocol/tests/domain.rs` | 3 | Replace the real home prefix with `/home/user`. |
| `crates/vowp-protocol/tests/fixtures/event_log_lines.jsonl` | 1 | Same. |
| `crates/vowp-protocol/tests/fixtures/job_n04_graph.json` | 1 | Same. |
| `crates/vowp-protocol/tests/report.rs` | 1 | Same. |
| `docs/design/adapters.md` | 3 | Same. |
| `docs/design/types.md` | 4 lines (5 occurrences) | Same. |

`scripts/audit/fix-unowned-private-paths.sh` is dry-run by default and prints the six files. The owner
runs it with `--execute`, reviews `git diff`, and commits those paths (or creates a follow-up job that
owns them). p01 did not edit or commit them.

## Findings: old history (informational)

The old history is not published, so these counts are for the record and for the choice in the next
section. The final scan took about a minute for 142 commits. `--all` follows every branch in the shared
repository, so the count grows while other jobs commit; this is a snapshot, not a stable total.

| Category | Findings | Allowlisted | Blocked | Explanation |
|---|---:|---:|---:|---|
| IPv4 | 549 | 549 | 0 | Loopback only. |
| Home paths | 8,693 | 6,900 | 1,793 | 6,900 placeholders; 1,793 real home paths in old design docs and test fixtures. |
| E-mail | 1,297 | 1,297 | 0 | Reserved example domains only. |
| Token | 3 | 3 | 0 | A fake `BEGIN RSA PRIVATE KEY` header and the allowlist text; no key material. |
| Private host | 402 | 402 | 0 | The same two Rust/filename false positives and allowlist text. |
| SSH-key paths | 0 | 0 | 0 | — |
| `.env` paths | 0 | 0 | 0 | — |
| Large blobs | 0 | 0 | 0 | No history blob above 1 MiB. |
| **Total** | **10,944** | **9,151** | **1,793** | All blocked history findings are the old real home paths. |

No real credential, private key, token, `.env` file or large binary was found in history. The history
would still leak the old home directory and internal worktree names, which is why it is not published.

## Manual private-data review

The automated rules cannot judge prose. These items were found by reading and are **not** part of the
fatal scan:

- `docs/design/plan.md` section 16, "The overnight build", describes internal operations: the owner's
  Mac and a VPS, a private bootstrap directory, a private tmux session, a deploy-key setup, model costs
  and a morning handover. It contains no credential, but it is private operational data. Remove section
  16 or rewrite it as a generic design note before publication. This file is outside p01's `owns`.
- `docs/dogfood/RESULTS.md` and `docs/release/0.1.0-rc.1.md` mention the private `~/vowp-night` directory
  and the night supervisor. p09 owns `docs/release/**` and is rewriting the 0.1.0 notes; `docs/dogfood/`
  is unowned and needs an owner decision.
- `AGENTS.md`, `CONTRIBUTING.md`, `framework/templates/AGENTS.md`,
  `framework/skills/vowp-worker/SKILL.md` and `crates/vowp-core/src/loops/merge_queue.rs` use
  `vowp-night` in the `AI-Assisted:` commit trailer. That is a product/workflow name, not a hostname or
  credential. The owner should decide whether the public project keeps that name; if not, code, tests and
  docs must change together.
- `git grep -nI 'vowp-night' -- .` is the command that lists these references.

## Commands to repeat

Run from the repository root.

```sh
# Working-tree gate (fatal). This is the first command to run before publication.
scripts/audit/scan.sh --worktree

# The whole audit: working tree plus old history. History is informational by default.
scripts/audit/scan.sh --summary

# The public branch gate, after squash-for-public.sh has created public-main.
scripts/audit/scan.sh --rev public-main

# Only if old history will ever be published (it will not, with the owner's choice).
scripts/audit/scan.sh --history --strict-history

# Licence table and project licence checks.
scripts/audit/licenses.sh
scripts/audit/licenses.sh --markdown
scripts/audit/licenses.sh --third-party

# Stage the six unowned path fixes (dry run first; --execute writes).
scripts/audit/fix-unowned-private-paths.sh
scripts/audit/fix-unowned-private-paths.sh --execute

# The publication branch (owner/orchestrator; never pushes).
scripts/audit/squash-for-public.sh

# Build gate used by the squash script.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The scanner's exit codes are: `0` no non-allowlisted finding in a fatal scope; `1` a non-allowlisted
finding; `2` usage error, non-git directory or malformed allowlist. `--worktree`, `--rev` and
`--strict-history` are fatal; `--all` and `--history` report old-history findings as informational.

## History decision for the owner

The owner chose the clean-history option on 2026-11-09.

| Option | Effect | Trade-off |
|---|---|---|
| **Clean orphan (chosen)** | `public-main` has one commit of the current tree, author `Syver-August <92798051+syver777@users.noreply.github.com>`, message `VOWP 0.1.0`. | History, blame and old commit links are lost in the public repo. The private repository keeps them. No private path is published. |
| Keep the old history | Push the existing commits to the new public repository. | Every old commit would publish the real home paths and internal worktree names. It would require rewriting every commit (a history rewrite and a force-push) and is not the chosen path. Rewriting `main` is forbidden by the repository rules. |

`scripts/audit/squash-for-public.sh` is therefore the publication path, not an option. It refuses to
continue if the working tree scan, the build, the top-level path allow-list or the scan of the new
`public-main` fails. It prints the push commands but never runs them. The old private repository is
renamed to `vowp-private`; its history is not lost, only not public.

## Licence summary

- **Project:** MIT. `LICENSE` is the standard MIT text with
  `Copyright (c) 2026 Syver-August Meyer`. All 7 workspace crates resolve to `license = "MIT"` through
  `[workspace.package]` in `Cargo.toml`.
- **Dependencies:** 160 third-party packages. All are within the allowed set:

| Licence expression | Packages |
|---|---:|
| `MIT OR Apache-2.0` | 112 |
| `MIT` | 18 |
| `Apache-2.0 OR MIT` | 9 |
| `MIT/Apache-2.0` | 6 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 4 |
| `Unlicense OR MIT` | 3 |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 2 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `Apache-2.0 / MIT` | 1 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `Zlib` | 1 |

The two `LGPL-2.1-or-later` rows are `r-efi`, which is also offered under MIT or Apache-2.0; VOWP uses
the MIT/Apache option. `ryu` is Apache-2.0 OR BSL-1.0 and is used under Apache-2.0. `unicode-ident` is
MIT/Apache-2.0 plus Unicode-3.0. No copyleft-only, unknown or missing licence was found.
`scripts/audit/licenses.sh` exits 0 and `scripts/audit/THIRD-PARTY-LICENSES.md` is the generated table.

**Attribution is needed for binary distributions**, because MIT, Apache-2.0, BSD, ISC, Unicode and Zlib
all require notices when their code is redistributed in a binary. It is not needed for the source
repository itself, where each Cargo package carries its own licence file. p01 owns only
`scripts/audit/**`, so the generated list is at `scripts/audit/THIRD-PARTY-LICENSES.md`; p06 should copy
it into each release archive (or the owner can add a root `THIRD-PARTY-LICENSES.md`).

## Checklist: before you click "Make public"

- [ ] `scripts/audit/scan.sh --worktree` exits 0 (the six unowned files are fixed and committed).
- [ ] The manual review above is resolved: `docs/design/plan.md` section 16 and the `vowp-night`
      references are removed or the owner accepts them.
- [ ] `scripts/audit/scan.sh --rev public-main` exits 0 after `squash-for-public.sh`.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
      `cargo test --workspace` pass on the public tree.
- [ ] `scripts/audit/licenses.sh` exits 0; third-party attribution is included in release archives.
- [ ] `SECURITY.md` points at GitHub private vulnerability reporting and the feature is enabled in
      repository settings.
- [ ] Version 0.1.0 is used by every crate, the npm package and the release tag; README says it is an
      early release and does not link to a site.
- [ ] No `.env`, private key, token, private hostname or large binary is in the tree.
- [ ] The old private repository is renamed (or backed up) before the new public repository is created.
- [ ] The release workflow's draft release is checked (assets, checksums, `vowp --version`) before it is
      published.
- [ ] The orchestrator follows `docs/maintainers/go-public.md`; p01 has not pushed, tagged or published
      anything.
