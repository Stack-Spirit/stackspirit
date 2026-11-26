# Public-readiness audit

- **Audit date:** 2026-11-09
- **Last updated:** 2026-11-18 (current public-tree counts and the explainer-video allowlist)
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
| Current tree | Public-tree scan clean: `scripts/audit/scan.sh --public-tree` exits 0 with 475 findings allowlisted and none blocked. The worktree-only scan still reports 60 pre-existing findings in internal acceptance fixtures, internal docs and the allowlist's own explanatory text, all outside the public tree. See [Current public-tree status](#current-public-tree-status-2026-11-18). |
| Old history | 11,351 findings: 1,835 real home paths and 5 fake PEM/allowlist-text token matches. No real token, key, `.env`, SSH-key or large-blob finding. History is informational because `public-main` is a fresh commit; the old history is never published. |
| Licences | Clean: project MIT, `LICENSE` 2026 Syver-August Meyer, all 7 workspace crates MIT, 160 third-party packages within MIT/Apache-2.0/BSD/ISC/Unicode/Zlib. |
| Publication gate | `scripts/audit/scan.sh --public-tree` and `scripts/audit/scan.sh --rev public-main` must both exit 0. The public-tree gate is clean; the public branch is built and scanned separately. |

## Current public-tree status (2026-11-18)

`scripts/audit/scan.sh --public-tree` exits 0 on the current branch: 475 findings, 475 allowlisted,
0 not allowlisted. The public tree contains exactly one large blob above 1 MiB, the intentional
checked-in explainer video `docs/media/vowp-explainer.mp4` at 12,790,922 bytes. It is allowlisted by
its exact path in `scripts/audit/allowlist.txt`; no other large blob exists. An intentional large
media file that belongs to the public documentation may be allowlisted this way, by exact path,
without changing the scanner's 1 MiB default threshold.

`scripts/audit/scan.sh --worktree` still exits 1 on 60 non-allowlisted findings in internal
acceptance fixtures, internal docs and the audit allowlist's own explanatory text. They are a
pre-existing baseline outside the public tree and are deliberately not allowlisted here.

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
| `ssh_target` | JSON/TOML `ssh_target` fields with a concrete `user@host` value, even when the value is not written as an `ssh` command. |
| `secret_path` | Tracked paths named `.env*` (except `.env.example`), `*.pem`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, `authorized_keys`, or under `.ssh/`. |
| `large_blob` | Tracked blobs larger than 1 MiB (override with `--max-blob-bytes`). |

The scanner never prints a matched value in full: every finding shows `file:commit:line: rule:
<first four characters>***`. Values are checked against `scripts/audit/allowlist.txt`; every entry has a
`rule|path|value|reason` line and an empty reason is rejected. The current allowlist covers only
loopback, conventional placeholder paths, reserved `example.com`/`example.invalid` addresses, a Git SSH
`user@host` address, three false-positive Rust identifiers, and one intentional media file, the
explainer video allowlisted by its exact path.

## Findings: working tree

The counts below are the 2026-11-09 p01 snapshot. The large-blob row was updated on 2026-11-18 for
the checked-in explainer video; see [Current public-tree status](#current-public-tree-status-2026-11-18)
for the current scanner totals.

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
| Large blobs (>1 MiB) | 0 | 1 allowlisted | Current tree: one large blob, the intentional explainer video `docs/media/vowp-explainer.mp4`, 12,790,922 bytes, allowlisted by its exact path; no other large blob. The p01 audit found none, when the largest tracked blob was about 100 KB. |
| **Total** | **150 findings, 136 allowlisted, 14 blocked** | **150 allowlisted, 0 blocked** | The 14 are the six unowned files below. |

The six files that needed the owner's edit (fixed after this audit; no private value is repeated here):

| File | Lines | Fix |
|---|---:|---|
| `crates/vowp-protocol/tests/domain.rs` | 3 | Replace the real home prefix with `/home/user`. |
| `crates/vowp-protocol/tests/fixtures/event_log_lines.jsonl` | 1 | Same. |
| `crates/vowp-protocol/tests/fixtures/job_n04_graph.json` | 1 | Same. |
| `crates/vowp-protocol/tests/report.rs` | 1 | Same. |
| `docs/design/adapters.md` | 3 | Same. |
| `docs/design/types.md` | 4 lines (5 occurrences) | Same. |

`scripts/audit/fix-unowned-private-paths.sh` is dry-run by default and prints the six files. The paths
were fixed by the follow-up code and docs jobs; `scan.sh --public-tree` now exits 0. A worktree-only
scan still reports the pre-existing findings described in the current-status section above. p01 did
not edit or commit the six files.

## Findings: old history (informational)

The old history is not published, so these counts are for the record and for the choice in the next
section. The final scan took about a minute for 145 commits. `--all` follows every branch in the shared
repository, so the count grows while other jobs commit; this is a snapshot, not a stable total.

| Category | Findings | Allowlisted | Blocked | Explanation |
|---|---:|---:|---:|---|
| IPv4 | 569 | 569 | 0 | Loopback only. |
| Home paths | 8,965 | 7,130 | 1,835 | 7,130 placeholders; 1,835 real home paths in old design docs and test fixtures. |
| E-mail | 1,382 | 1,382 | 0 | Reserved example domains only. |
| Token | 5 | 0 | 5 | Fake PEM headers in historical test fixtures and the audit allowlist's own historical text; no key material. |
| Private host | 430 | 430 | 0 | The same two Rust/filename false positives and allowlist text. |
| SSH-key paths | 0 | 0 | 0 | — |
| `.env` paths | 0 | 0 | 0 | — |
| Large blobs | 0 | 0 | 0 | No history blob above 1 MiB at the audit date. The baseline predates the explainer video, added 2026-11-18, which is now one allowlisted large blob in later commits. |
| **Total** | **11,351** | **9,511** | **1,840** | The blocked history findings are old real home paths and fake PEM text. |

No real credential, private key, token, `.env` file or large binary was found in history. The history
would still leak the old home directory and internal worktree names, which is why it is not published.

## Manual private-data review

The automated rules cannot judge prose. These items were found by reading and are **not** part of the
fatal scan:

- The build-history pages that described internal operations (`docs/design/plan.md` section 16, "The
  overnight build", and `docs/dogfood/RESULTS.md`) have been removed. The current design docs describe
  the product, not the private build environment.
- `AGENTS.md` uses `vowp-night` in the `AI-Assisted:` commit trailer; `CONTRIBUTING.md`, `framework/`
  and the merge queue use `VOWP`. That is a product/workflow name, not a hostname or credential. The
  owner should decide whether the public project keeps the name; if not, code, tests and docs must
  change together.
- `git grep -nI 'vowp-night' -- .` lists that name reference and the audit text that searches for it.

## Commands to repeat

Run from the repository root.

```sh
# Public-tree gate (fatal). This is the first command to run before publication.
scripts/audit/scan.sh --public-tree

# Whole worktree, including internal acceptance paths. Also fatal; it currently
# reports the pre-existing internal baseline described above.
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
repository itself, where each Cargo package carries its own licence file. The audit's package table is
`scripts/audit/THIRD-PARTY-LICENSES.md`; the reviewed bundle that ships in every archive is the root
`THIRD-PARTY-LICENSES.md`, generated from the same locked metadata by
`scripts/release/generate-third-party-licenses.py`. `scripts/release/package-archive.sh` refuses to
package an archive when the bundle, `LICENSE`, `README.md` or the `vowp` binary is missing or empty.

## Checklist: before you click "Make public"

- [ ] `scripts/audit/scan.sh --public-tree` exits 0.
- [ ] The manual review above is resolved: the owner accepts or renames the remaining `vowp-night`
      reference.
- [ ] `scripts/audit/scan.sh --rev public-main` exits 0 after `squash-for-public.sh`.
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
      `cargo test --workspace` pass on the public tree.
- [ ] `scripts/audit/licenses.sh` exits 0; third-party attribution is included in release archives.
- [ ] `SECURITY.md` points at GitHub private vulnerability reporting and the feature is enabled in
      repository settings.
- [ ] Version 0.1.0 is used by every crate, the npm package and the release tag; README says it is an
      early release and does not link to a site.
- [ ] No `.env`, private key, token or private hostname is in the public tree; every large blob is
      allowlisted by its exact path.
- [ ] The old private repository is renamed (or backed up) before the new public repository is created.
- [ ] The release workflow's draft release is checked (assets, checksums, `vowp --version`) before it is
      published.
- [ ] The orchestrator follows the runbook in `scripts/audit/go-public.md`; p01 has not pushed, tagged
      or published anything. (The runbook lives under `scripts/audit/` because p01's owned paths do not
      include the separate `docs/maintainers/` runbook path.)
