---
job: v2-j1-public-tree-audit
status: done
branch: job/v2-j1-public-tree-audit
commits: [2a61d81, 584cce3, 16ef1b1, 483ecec, e8d2bc0]
tests: "bash scripts/audit/self-test.sh: PASS; bash docs/tests/run.sh: PASS (private and public tree); python3 docs/reference/check-reference.py: PASS; python3 docs/reference/generate.py --check: PASS (74 pages); cargo fmt/clippy: PASS; cargo test --workspace: PASS (0 failed, 2 ignored); scan.sh --public-tree: FAIL (1 blocked ssh_target, 233 allowlisted); scan.sh --rev public-main: PASS (235 allowlisted, 0 blocked); squash-for-public.sh: FAIL at the private-marker gate (35 findings, branch left for inspection)"
files_changed: [docs/maintainers/v0.2-acceptance/public-tree-audit.md, scripts/audit/allowlist.txt, scripts/audit/go-public.md, scripts/audit/public-tree.txt, scripts/audit/public-tree-markers.sh, scripts/audit/public-tree-scrub.py, scripts/audit/scan.sh, scripts/audit/self-test.sh, scripts/audit/squash-for-public.sh]
open_items: []
questions_for_owner:
  - id: f-public-tree-v02-docs
    owns: [docs/design/v0.2/README.md, docs/design/v0.2/home-and-surfaces.md, docs/concepts/capacity.md, docs/design/README.md, docs/design/sessions.md]
    why: "Replace the v2-* job ids in the examples, remove the delivery dependency graph and the 'merged 2026-11-10' spike notes, and rewrite the C0/C1/S1-S3/K3/T2/T3 build-phase wording in product terms, including the S2 measurement sentence in docs/design/sessions.md. Keep the behaviour, measurements and named tests; only the build-diary material changes."
  - id: f-public-tree-code-comments
    owns: [crates/vowp-cli/src/**, crates/vowp-cli/tests/**, crates/vowp-core/src/**, crates/vowp-core/tests/**, crates/vowp-protocol/src/**, crates/vowp-protocol/tests/**, crates/vowp-store/src/**, crates/vowp-store/tests/**]
    why: "Sweep the build-phase comments (C0/C1/D1/D2/E1/E2/F2/F5/G1 fills), the two docs/maintainers/v0.2-spikes references, the v2-g1-vault test id and the concrete ssh_target in crates/vowp-protocol/tests/domain.rs from shipping source and tests. The interfaces stay unchanged; this is a comment and fixture-value change."
  - id: f-public-tree-security-doc
    owns: [docs/security.md]
    why: "docs/security.md:173 points at scripts/audit/scan.sh, which is pruned from the public tree. Describe the secret scan without the private path."
---

# v0.2 public-tree audit (J1)

**Verdict: the public candidate is not ready to ship yet.** Three checks pass; the private-data
check fails on 35 build-metadata findings plus one concrete SSH target in the private test fixture
(the public candidate scrubs that value). The findings need the three `f-*` jobs under
`questions_for_owner`; J2 should rebuild `public-main` after they merge.

## Tree audited

- Merged `origin/main` at **`a430e32`** (`docs: refresh the T1 installer status and diff lists`).
  `main` moved during the audit from `e3e73b3` through `1864d68`, `0655c2a`, `d4ce1f9`,
  `ae14b2d`, `9b19b79`, `4423385`, `b4458a4`, `1bafd01` and `feffdf6` to `a430e32`; this report and
  the branch were rebased onto the final tree. The last commit only changes
  `docs/maintainers/fresh-setup.md`, which is pruned, so the public tree is unchanged. The tooling
  commit is **`e8d2bc0`**.

  ```text
  $ git diff --name-only feffdf6..a430e32
  docs/maintainers/v0.2-acceptance/fresh-setup.md
  ```

- Public candidate: the orphan `public-main` tree produced by
  `scripts/audit/squash-for-public.sh --force`, tree **`6383faa`**, **636 files**
  (commit `2ad3462`; this report is pruned, so a later report-only commit does not change the tree).
- No push, tag, publish or visibility change was made. The private working tree, the real home, the
  default tmux server and the network were not touched by the audit.
- The `commits` field lists the substantive commits before the report-only commit that contains
  this file; the worker report at `/home/user/vowp-night/jobs/v2-j1-public-tree-audit/report.md`
  lists the full chain.

## Commands and results

| Command | Result |
|---|---|
| `bash scripts/audit/self-test.sh` | PASS: scanner rules, masking, allowlist, history, public-tree prune, build markers and scrub idempotence |
| `bash scripts/audit/scan.sh --public-tree --summary` | **FAIL: 1 blocked `ssh_target`** (234 findings, 233 allowlisted) |
| `bash scripts/audit/scan.sh --rev public-main --summary` | PASS: 235 findings, 235 allowlisted, **0 blocked** (the scrubbed SSH fixture is allowlisted) |
| `bash scripts/audit/public-tree-check.sh --rev public-main` | PASS: 636 files, no pruned path, every required path |
| `bash scripts/audit/public-tree-markers.sh --rev public-main --summary` | **FAIL: 35 findings** (8 `job_id`, 3 `private_path`, 24 `build_diary`) |
| `bash docs/tests/run.sh` (private tree) | PASS: 537 links in 143 files, 26 headings; 768 references, 69 commands, 40 implemented, 0 planned |
| `bash docs/tests/run.sh` (public tree archive) | PASS: 520 links in 129 files, 26 headings; same command set |
| `python3 docs/reference/check-reference.py` | PASS: 39 top-level commands, 72 JSON Schema pages |
| `python3 docs/reference/generate.py --check` (`VOWP_BIN=…/debug/vowp`) | PASS: 74 generated pages are current |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS: 0 failed, 2 ignored |
| `bash scripts/audit/squash-for-public.sh --force` | Build, clippy, tests, prune, scrub and secret scan pass; **stops at the marker gate** and leaves `public-main` for inspection |

The squash test stage hit load-sensitive `setup_verify` tests twice
(`setup_verify_run_reports_every_check` at `crates/vowp-cli/tests/setup_verify.rs:441` and
`setup_verify_fixture_job_merges` at `:245`). Both passed when rerun alone and the next full run
passed the build and test stages. This is the known load-sensitive setup-verify group, not a
public-tree finding.

The supervisor's first gate run on this branch also hit a `loops_supervisor` failure under load
(the gate output was truncated before the test name). The exact gate command,
`cargo test --workspace`, passed on rerun on the same tree, and
`cargo test -p vowp-core --test loops_supervisor` passed three more times. The timing-sensitive
cases in that file use 1-second sleeps (`tick_does_not_block_on_slow_gate`,
`tick_does_not_block_on_slow_merge`); this is a load-sensitive test flake, not a public-tree or
product finding.

### Check 1 — no private data: **FAIL**

The secret scanner now covers eight content patterns. One private-tree finding is blocked:
`crates/vowp-protocol/tests/domain.rs:215` contains `"ssh_target": "dev@vps"` — the build Unix
user and the home node name. The old scanner only matched a command-form `ssh user@host`, so this
shipped unnoticed; the new `ssh_target` rule (JSON/TOML field plus value) catches it. The
public-tree scrub rewrites the value to `"ssh_target": "vowp@example.invalid"` in the staged
candidate, and that reserved placeholder is allowlisted, so `scan.sh --rev public-main` passes.
The private fixture still needs the `f-public-tree-code-comments` job (it owns
`crates/vowp-protocol/tests/**`). No other secret-shaped rule is blocked.

The build-metadata scan fails:

```text
$ bash scripts/audit/public-tree-markers.sh --rev public-main
job_id: crates/vowp-core/tests/vault.rs:480:                id: JobId::parse("v2-g1-vault").expect("job id"),
job_id: docs/design/v0.2/home-and-surfaces.md:1728:     "running": [{"job": "v2-b1-spec", "kind": "docs", "role": "worker",
job_id: docs/design/v0.2/home-and-surfaces.md:1731:     "blocked": [], "queue": [{"job": "v2-c0-skeleton", "title": "Freeze interfaces", "kind": "build",
job_id: docs/design/v0.2/home-and-surfaces.md:1734:     "last_merge": {"job": "v2-a0-docs-truth", "at": "2026-11-10T12:00:00.000Z",
job_id: docs/design/v0.2/home-and-surfaces.md:1735:                    "commit": "abc1234", "branch": "job/v2-a0-docs-truth"},
job_id: docs/design/v0.2/home-and-surfaces.md:3313:{"project":"vowp","job":"v2-c0-skeleton","title":"Freeze interfaces","kind":"build","role":"worker","priority":99,"state":"ready","age_secs":60,"waiting_reason":"waiting: RAM 1.2 GB short"}
job_id: docs/design/v0.2/home-and-surfaces.md:4127:follow-up spec amendment (`v2-b2-spec-amend`, which owns this file).
job_id: docs/design/v0.2/home-and-surfaces.md:4243:The card `v2-b1-spec` is complete when this document contains, and a reviewer can point to:
private_path: crates/vowp-core/src/capacity/scope.rs:4://! (`docs/maintainers/v0.2-spikes/capacity.md`). The cgroup path is preferred; the process
private_path: crates/vowp-core/src/surface/opencode_host.rs:4://! (`docs/maintainers/v0.2-spikes/app-surfaces.md`). The host runs opencode v2 as a managed
private_path: docs/security.md:173:scanner is `scripts/audit/scan.sh`.
build_diary: crates/vowp-cli/src/cli.rs:4://! C0 freezes every v0.2 top-level command, subcommand and flag here; each command's
build_diary: crates/vowp-protocol/src/capacity.rs:3://! Spec: `docs/design/v0.2/home-and-surfaces.md` §6.1–§6.5 and §13.1. C0 freezes the
build_diary: crates/vowp-protocol/src/catalog.rs:3://! Spec: `docs/design/v0.2/home-and-surfaces.md` §10 and §13.1. C0 freezes the shapes;
build_diary: crates/vowp-protocol/src/snapshot.rs:3://! Spec: `docs/design/v0.2/home-and-surfaces.md` §5 and §13.1. C0 freezes the shapes; D1
build_diary: crates/vowp-protocol/src/surface.rs:3://! Spec: `docs/design/v0.2/home-and-surfaces.md` §7.2–§7.6 and §13.1. C0 freezes the
build_diary: docs/design/v0.2/README.md:56:B1 spec (this directory)
build_diary: docs/design/v0.2/README.md:58:   ├─ C0 skeleton ──┬─ C2 node supervisor ── E2 admission ── E3 advisor
build_diary: docs/design/v0.2/README.md:72:C0 freezes every interface; C1 can run in parallel because the store split is inside
build_diary: docs/design/v0.2/README.md:74:`_node` literal in `vowp-protocol/src/ids.rs` (both C1-owned in the spec §14.1). The critical path
build_diary: docs/design/v0.2/README.md:75:is B1 → C0/C1 → F1 → F2 → F5/F6 → F4 → I1b → K1 → T1 → J1 → J2.
build_diary: docs/design/v0.2/home-and-surfaces.md:3:Status: current for the commands shown; **normative for v0.2**. This is the specification the v0.2 jobs implement. It covers plan
build_diary: docs/design/v0.2/home-and-surfaces.md:18:C0, C1 and the feature job ids in this document are the implementation-phase labels used by the
build_diary: docs/design/v0.2/home-and-surfaces.md:188:builds and its tests still pass with only two C1-owned changes outside `vowp-store`: the
build_diary: docs/design/v0.2/home-and-surfaces.md:1183:- `crates/vowp-core/src/deps.rs` is **C1-owned** for this body. The signature is unchanged, so C0
build_diary: docs/design/v0.2/home-and-surfaces.md:1220:`vowp-protocol/src/ids.rs`, C1-owned in §14.1, because the store, migration and session service
build_diary: docs/design/v0.2/home-and-surfaces.md:2281:**Spool, ack window and replay (S2, merged 2026-11-10).** `__exec` keeps the adapter running and
build_diary: docs/design/v0.2/home-and-surfaces.md:2925:install, no browser, no public port. The docs guide (`docs/guides/phone.md`) is K3's job; the spec
build_diary: docs/design/v0.2/home-and-surfaces.md:4127:follow-up spec amendment (`v2-b2-spec-amend`, which owns this file).
build_diary: docs/design/v0.2/home-and-surfaces.md:4129:1. **S1 capacity spike (merged 2026-11-10).** S1 proved cgroup v2 user scopes and the `/proc`
build_diary: docs/design/v0.2/home-and-surfaces.md:4141:2. **S2 remote-exec spike (merged 2026-11-10).** The transport is SSH with one `session ingest`
build_diary: docs/design/v0.2/home-and-surfaces.md:4151:3. **S3 app-surface spike (merged 2026-11-10).** opencode v2.0.28 is the proven contract: HTTP API
build_diary: docs/design/v0.2/home-and-surfaces.md:4165:   enum variant additively. T2 acceptance test 3 and any K4 wording must therefore look for a
build_diary: docs/design/v0.2/home-and-surfaces.md:4192:    `home.example` and `vowp@home.example`. The J1 scrub still runs over it.
build_diary: docs/design/v0.2/home-and-surfaces.md:4195:    remain the complete v0.2 story. T3 reports this honestly rather than blocking the release.
```

The same 35 findings are present in the public tree; `--rev public-main` and `--worktree` agree.

**Manual build-diary review (not all line-automatable).** The public design pages still read as a
delivery plan in places:

- `docs/design/v0.2/README.md:56-75` is the implementation dependency graph and critical path
  (`B1 spec` → `C0 skeleton` → … → `J1` → `J2`). The five lines the marker scan names are only the
  ones with the exact phases; the whole block is build-plan material.
- `docs/design/v0.2/home-and-surfaces.md:7` says the document freezes "what the later jobs need",
  and `:18-19` calls the labels "implementation-phase labels … None is user-facing" while they
  ship. `:1812`, `:1820`, `:1832`, `:1853`, `:2270`, `:2290`, `:2476`, `:2646`, `:2731`, `:2789`,
  `:3085`, `:3439`, `:3640-3727`, and `:4159` add `S1`/`S2`/`S3`/`F5`/`F6`/`H1`/`C2`/`E2` build
  notes.
- `docs/concepts/capacity.md:67`, `:73` and `:238` say `S1 measured` / `the S1 measurements`; the
  numbers can stay, the spike label should become product wording. `docs/design/sessions.md:245`
  has the same problem for `S2` ("S2 measured 99.6–265 ms live SSH, bound < 2 s"); keep the
  measurement, rewrite the spike label.
- `docs/design/README.md:26` and `:60` advertise `C0 hooks` in the v0.2 spec.
- Shipping source comments use the same phase labels: `C0` appears 128 times, `E2` 26, `G1` 31,
  `F5` 19, `F6` 14, `E1` 22, `S3` 17, `C1` 38 (line counts from
  `git grep -I -E '\bC0\b'` and the matching labels). The marker rule names the `C0 freezes` subset;
  the rest is the same cleanup.
- The public tests also carry the labels: `crates/vowp-store/tests/schema_enums.rs:294,318` (C0),
  `crates/vowp-store/tests/migration.rs:2`, `crates/vowp-store/tests/split.rs:2` and
  `crates/vowp-store/tests/crash_split.rs:2` (C1), `crates/vowp-cli/tests/core.rs:330` (C0),
  `crates/vowp-cli/tests/mcp.rs:6` (F6), `crates/vowp-cli/tests/up.rs:1572` (D1) and
  `crates/vowp-protocol/tests/schema.rs:29` (C0). The code-comment job owns all three test trees.

**Not findings:** `AI-Assisted: <model> via VOWP` is the documented product trailer
(`CONTRIBUTING.md`, the PR template and the framework templates) and is intentionally not matched;
there is no private `AI-Assisted: … via vowp-night` line in the candidate. The public repository
name and author identity are the owner's public project metadata, not private data.

### Check 2 — links and documented commands: **PASS**

`bash docs/tests/run.sh` passes on both the merged private tree and the public archive. The public
tree has 520 relative links in 129 Markdown files and all 26 anchored headings resolve. The command
check covers 768 references to 69 distinct commands; 40 are implemented and no reference is still
marked *(planned)*. The earlier broken `cost-dashboard.md → http-api.md` link and the stale
`check-commands.sh` are fixed on `origin/main` (`7a217a0`).

### Check 3 — docs match the shipped CLI (K4): **PASS**

`python3 docs/reference/check-reference.py` reports 39 top-level commands (the `V0_1_COMMANDS` +
`V0_2_COMMANDS` set, hidden interfaces included) and 72 parseable schema pages.
`python3 docs/reference/generate.py --check` proves the committed pages are the exact current help
tree and schema output: **74 generated pages are current**. A manual spot-check of
`vowp --help`, `vowp setup --help`, `vowp project --help`, `vowp fleet --help` and
`vowp capacity --help` shows the same command names and descriptions as `docs/reference/cli.md`.
The binary still reports `vowp 0.1.0`; the version bump is J2's release action, not a reference
drift.

### Check 4 — no private control-plane files in the public tree: **PASS**

`public-main` has 636 files and no `AGENTS.md`, `docs/maintainers/**`, `scripts/spikes/**`,
`scripts/acceptance/**`, `scripts/audit/**`, `scripts/dogfood/**`, `scripts/fresh-user/**` or
`site/**`. The manifest now prunes `scripts/acceptance/` and `scripts/spikes/` as well as the
existing `docs/maintainers/`, `scripts/audit/`, `scripts/dogfood/`, `scripts/fresh-user/` and
`site/`; it also requires `docs/design/v0.2/`, `docs/guides/phone.md` and `docs/setup/`.

Evidence:

```text
$ git ls-tree -r --name-only public-main | grep -E '^(AGENTS\.md|docs/maintainers/|scripts/(spikes|acceptance|audit|dogfood|fresh-user)/|site/)'
(no output)
$ bash scripts/audit/public-tree-check.sh --rev public-main
public-tree: OK: 2ad3462ad283d87c82bbeef64c9c0c2a454adf0e has 636 file(s), no pruned path and every required path.
```

## Tooling changed by this job

- `scripts/audit/public-tree.txt` prunes `scripts/acceptance/` and `scripts/spikes/`, and requires
  the v0.2 design and setup docs.
- `scripts/audit/scan.sh` gains `--public-tree`, which scans only the paths that would ship and
  skips every manifest prune path. It also initialises its count file (a clean tree previously made
  the summary awk read a missing file), ignores Rust field access such as
  `args.local_port` / `self.local.path()` as hostnames, and adds the `ssh_target` rule for
  JSON/TOML user@host fields.
- `scripts/audit/public-tree-scrub.py` rewrites the concrete `ssh_target` fixture in
  `crates/vowp-protocol/tests/domain.rs` to the reserved `vowp@example.invalid` value in the staged
  index only; the self-test covers the rewrite and the untouched private file.
- `scripts/audit/public-tree-markers.sh` is new: it scans a worktree, index or revision for
  `vowp-night`, private AI trailers, `v2-*` job ids, references to pruned private paths and
  build-diary wording. `scripts/audit/self-test.sh` covers it and `squash-for-public.sh` runs it on
  the new branch before reporting success.
- `scripts/audit/allowlist.txt` records the v0.2 product placeholders (`/home/vowp`,
  `/home/vowp-vault`, `/home/fake`, `setup@example.invalid`, `vowp@home.example`,
  `vowp@example.invalid`), the reserved `ssh_target` examples and `0.0.0.0` with reasons.
- `scripts/audit/squash-for-public.sh` scans the scrubbed staged tree, not the unscrubbed working
  tree, and `scripts/audit/go-public.md` lists the new marker guard.

## f-* jobs to create

The three proposals are in the front matter. They are deliberately separate so their `owns` do not
overlap: the v0.2 docs job, the code-comment sweep, and `docs/security.md`. After they merge, rerun:

```sh
bash scripts/audit/self-test.sh
bash scripts/audit/scan.sh --public-tree
bash scripts/audit/public-tree-markers.sh --worktree
bash docs/tests/run.sh
python3 docs/reference/check-reference.py
VOWP_BIN="${CARGO_TARGET_DIR:-target}/debug/vowp" python3 docs/reference/generate.py --check
bash scripts/audit/squash-for-public.sh --force
bash scripts/audit/scan.sh --rev public-main
bash scripts/audit/public-tree-check.sh --rev public-main
bash scripts/audit/public-tree-markers.sh --rev public-main
```

The last three commands must all exit 0 before J2 tags `v0.2.0`.
