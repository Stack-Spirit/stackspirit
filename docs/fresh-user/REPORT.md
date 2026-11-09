# Fresh-user test report — following the README in a clean container

Tested on 2026-11-09 from branch `job/n15-fresh-user`, based on `origin/main` at
`b080abd` (`examples: make hello-vowp WORK and inspection safe`).

The test is `bash scripts/fresh-user/run.sh`. It builds `rust:1-slim-bookworm`
plus `git` and `tmux`, copies the checkout in without `target/` or `.git`, starts
with no VOWP binary, and runs the documented commands extracted from the fenced
`sh` blocks. The run below is the final clean run (after the image was fixed
to pre-install the `stable` toolchain, so no runtime rustup download happens).

| Measurement | Result |
|---|---|
| Test result | **PASS**, exit 0, 31 documented commands |
| Time to first success (`vowp --version`, `README.md:76`) | **37 s** |
| Total documented-command time | **188 s** |
| Image build | 19 s with a warm Docker layer cache (74 s cold) |
| Container | `rust:1-slim-bookworm` (Debian 12), rustc/cargo 1.99.1, git 2.39.5, tmux 3.3a |
| hello-vowp example | 3 jobs `done`; files merged to `origin/main` |

Merge-target check: the same scripts overlaid on an archive of `origin/main`
(`006925c`) also passed on 2026-11-09: 31 commands, first success 29 s, total
171 s. The example README's main command moved from line 33 to 34; the
extractor followed it automatically, which is the anti-drift behaviour this
test needs.

The container has no secrets and no real harness sign-in. Two fixtures
(`scripts/fresh-user/fake-bin/claude` and `opencode`) answer only `--version`,
because `vowp doctor` probes the harnesses and the container cannot sign in to
Claude Code or opencode. They never fake a model turn. The hello-vowp example
uses its own fake worker and runs the real supervisor, gates, worktrees and
merge queue. Without the fixtures, deviation D1 below is what a user sees.

## Deviations

### D1 — README quickstart omits the harness prerequisite (blocker for README-only readers)

- **Doc says:** `README.md:71-72` lists only the pinned Rust toolchain, `git`
  and `tmux` as prerequisites. `README.md:89-90` then runs `vowp init` and
  `vowp doctor`.
- **Reality:** on a clean machine with no `claude` or `opencode`, `vowp doctor`
  exits 1 with `fail harness:claude-code`, `fail harness:opencode`,
  `fail profiles` and `doctor: failures found`. The install guide does say a
  harness is required (`docs/guides/install.md:17`) and that a missing harness
  is a `fail`, not a warning (`docs/guides/install.md:124`).
- **Reproduction:** build the binary, then in a scratch git repository with a
  remote:
  ```sh
  export HOME=$(mktemp -d)
  export PATH=/usr/bin:/bin          # hide ~/.local/bin/claude and opencode
  /path/to/target/debug/vowp init --name hello-vowp
  /path/to/target/debug/vowp doctor  # exit 1
  ```
  Observed: `fail harness:claude-code  binary not found on PATH`,
  `fail harness:opencode  binary not found on PATH`,
  `fail profiles  ... harness not runnable`, `doctor: failures found`.
- **Suggested fix:** add "a harness (Claude Code or opencode), installed and
  signed in" to the README quickstart prerequisites and link the install guide
  there, not only at the end of the section. The test can keep using fake
  `--version` fixtures.

### D2 — supervisor commands are marked planned but work (confusing)

- **Doc says:** `README.md:16-18` lists the working commands without
  `vowp up`/`vowp status`; `README.md:92-94` marks them `# (planned) — the
  supervisor commands land with n09b`. `docs/guides/quickstart.md:7-8` says the
  supervisor, board, attach, plan, inbox and down commands are marked
  *(planned)*, and the page has `**Planned section:**` headings at lines 151,
  207.
- **Reality:** in the clean container, `vowp up` prints
  `supervisor started (pid …)` and `vowp status` exits 0 and prints
  `supervisor: running (pid …)` (`README.md:93-94`, run steps 7-8). `vowp down`
  also stops it (verified on the host and used by the test's cleanup).
- **Reproduction:** the run log, steps 7-8:
  ```
  [fresh-user] step 07 README.md:93 - vowp up
  supervisor started (pid 5251)
  [fresh-user] step 08 README.md:94 - vowp status
  supervisor: running (pid 5251, up 0s, last tick 0s ago)
  ```
- **Suggested fix:** remove the *(planned)* markers from `up`, `status` and
  `down` now that n09b has landed, and update the README working-command list.
  Re-test `board`, `attach`, `plan` and `inbox` before unmarking those.

### D3 — the hello-vowp example is marked planned but runs (confusing)

- **Doc says:** `docs/guides/quickstart.md:9-10`: "The example project
  `examples/hello-vowp` *(planned)* will offer a runnable three-job graph with
  fake adapters."
- **Reality:** `FAKE=1 examples/hello-vowp/run.sh` completed in about 8 s; all three
  jobs reached `done`; `origin/main` had the merged files (run step 31). The
  example's own README documents it as current.
- **Reproduction:** from the repository root, `FAKE=1 examples/hello-vowp/run.sh`;
  observed final lines `all three jobs reached done and origin/main has the result`.
- **Suggested fix:** remove *(planned)* and link the example from the quickstart
  (and see D4).

### D4 — README never points at the runnable example (confusing)

- **Doc says:** the README quickstart (`README.md:69-98`) ends at
  `vowp init`/`vowp doctor`/`vowp up`/`vowp status`.
- **Reality:** the only runnable end-to-end demo is `examples/hello-vowp`; the
  README does not mention it, so a new reader stops at a supervisor with no jobs
  and no next step. The example is reachable only from the quickstart guide's
  stale planned note and from the repository tree.
- **Reproduction:** `grep -n 'hello-vowp' README.md` finds only the
  `vowp init --name hello-vowp` command, not a link.
- **Suggested fix:** add a short "Try the demo" step after `vowp doctor` (or
  after the quickstart) with the exact command
  `FAKE=1 examples/hello-vowp/run.sh`, and link `examples/hello-vowp/README.md`.

### D5 — `vowp job show --gates --runs` does not show configured gates before a run (confusing)

- **Doc says:** `docs/guides/quickstart.md:136-137`: "`vowp job show` prints the
  job and its edges; `--gates --runs` adds its gates and (once it has run) its
  runs and gate evidence."
- **Reality:** right after `vowp job add`, `vowp job show hello-docs --gates
  --runs` prints `runs:` and `gates:` with no rows, even though the job has four
  configured gates. The command prints only `store.latest_gate_results`
  (`crates/vowp-cli/src/cmd/job.rs:380-398`), not the configured gate list.
- **Reproduction:** run the quickstart block at `docs/guides/quickstart.md:88-123`,
  then `vowp job show hello-docs --gates --runs`. Observed in run step 30:
  ```
  edges:
  runs:
  gates:
  ```
  `vowp --json job show hello-docs` shows `report`, `owns_respected`,
  `secret_scan` and `review` in `gates`.
- **Suggested fix:** print the configured gates with a "not run" marker before
  the results, or change the docs to say that `--gates` prints recorded gate
  **results** (empty until the job runs).

### D6 — install guide says `install.sh` is planned while it exists (nit)

- **Doc says:** `docs/guides/install.md:3-4`: "prebuilt release archives, an
  `install.sh`, Homebrew and an npm wrapper are planned for the first public
  release."
- **Reality:** `scripts/install.sh` exists, is executable and is documented as
  the per-user binary + systemd + cron-watchdog installer in
  `scripts/README.md:7,14-52`. A reader looking for an `install.sh` can miss it.
- **Reproduction:** `test -x scripts/install.sh && ./scripts/install.sh --help`.
- **Suggested fix:** clarify that a curl-able/downloaded installer is planned,
  and link `scripts/install.sh` from the install guide for people who already
  have a checkout.

### D7 — `cargo install` path is described as `~/.cargo/bin` unconditionally (nit)

- **Doc says:** `docs/guides/install.md:63`: "`cargo install` writes to
  `~/.cargo/bin` by default."
- **Reality:** that is true only for a default rustup layout. The official Rust
  image sets `CARGO_HOME=/usr/local/cargo`, so the documented command installed
  `/usr/local/cargo/bin/vowp` (run step 3). It works because that directory is
  on `PATH`, but the path in the prose is not universal.
- **Reproduction:** `docker run --rm rust:1-slim-bookworm env | grep CARGO_HOME`,
  then run the documented `cargo install --path crates/vowp-cli`.
- **Suggested fix:** say "under `$CARGO_HOME/bin` (usually `~/.cargo/bin`)" and
  keep the `PATH` advice.

### D8 — repository layout table omits `examples/` and `scripts/` (nit)

- **Doc says:** `README.md:126-138` lists the crates, `framework/` and
  `docs/design/`.
- **Reality:** the quickstart now depends on `examples/hello-vowp`, and
  `scripts/` contains the install/watchdog tooling, but neither directory has a
  row in the layout table.
- **Reproduction:** compare `ls` with `sed -n '126,138p' README.md`.
- **Suggested fix:** add two rows: `examples/` (runnable examples) and
  `scripts/` (install, watchdog, uninstall, hermetic tests).

## What the docs got right

These documented outcomes were checked in the clean container and matched:

| Documented claim | Where | Observed |
|---|---|---|
| Build from source, then print the version | `README.md:74-77` | `cargo build` finished; `vowp 0.1.0-dev` |
| `cargo install` then `vowp --version` | `README.md:81-84`, `install.md:56-61` | installed; `vowp 0.1.0-dev` |
| `vowp init` writes 11 files and skips none on a fresh repo | `install.md:78`, `quickstart.md:44` | three fresh demo repos: `11 files written, 0 skipped` |
| `vowp doctor` succeeds when the prerequisites are present | `quickstart.md:61-80` | `doctor: ok`; only `providers`/`systemd`/`watchdog` warnings |
| `vowp job add` accepts the heredoc brief and gate list | `quickstart.md:87-124` | `added job hello-docs (pending)` |
| `vowp graph` prints layer/state per job | `quickstart.md:132` | `L0 hello-docs [pending] Write the hello page` |
| `FAKE=1 examples/hello-vowp/run.sh` completes | `examples/hello-vowp/README.md:32-34` | 3 jobs `done`; `origin/main` has the result |
| `vowp up` detaches and `vowp status` sees it | `README.md:92-94` | `supervisor started`, then `supervisor: running` |

## Commands not run, and why

The extractor stops at the quickstart guide's planned section and skips only
commands the docs themselves mark as planned:

| Command | Doc line | Reason |
|---|---|---|
| `vowp plan "…" --dry-run` | `docs/guides/quickstart.md:143` | The block is marked `# (planned)`; it needs a planner model turn, and the fixture does not fake one. |
| `vowp up`/`status`/`board`/`attach` in quickstart §6 | `docs/guides/quickstart.md:151-179` | Marked planned. The `up`/`status` subset is exercised from the README block; `board`/`attach` are interactive and need a job window. |
| `vowp inbox ls`/`inbox answer` | `docs/guides/quickstart.md:197-201` | Marked planned; no inbox item exists in this run. |
| `vowp down` in quickstart §8 | `docs/guides/quickstart.md:209-212` | Marked planned, but used as the test's cleanup (verified on the host); not timed as a documented step. |
| `KEEP=1 FAKE=1 examples/hello-vowp/run.sh` | `examples/hello-vowp/README.md:61` | Optional inspection variant. |
| `cd /tmp/hello-vowp.XXXX/…` and inspection commands | `examples/hello-vowp/README.md:69-71` | Optional post-run inspection, not part of the first run. |

## Planner follow-ups

The deviations above are documentation fixes in files owned by the docs jobs;
this job did not edit them. Recommended order:

1. **D1** — add the harness prerequisite to the README quickstart (blocker for
   a README-only reader).
2. **D2/D3** — remove stale *(planned)* markers for `up`, `status`, `down` and
   `examples/hello-vowp`; keep the markers on anything not yet exercised.
3. **D4** — link the hello-vowp example from the README.
4. **D5** — either improve `vowp job show --gates` or tighten the sentence in
   `docs/guides/quickstart.md:136-137`.
5. **D6/D7/D8** — wording/layout nits.

`bash scripts/fresh-user/run.sh` is the regression test for all of this: it
reads the commands from the docs, so a docs change that breaks the flow fails
the script with the exact `file:line`.
