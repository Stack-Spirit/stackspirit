# Issue drafts for the first public release

These drafts can be pasted into GitHub Issues after the repository is public. They are ordered by
difficulty, not priority. Labels assume the repository has at least `good first issue`, `bug`,
`enhancement`, `cli`, `core`, `sessions`, `graph`, `promote`, `tests` and `docs`; adjust them to the
labels actually created.

Each draft names repository-relative files only. No draft asks for credentials, a hostname, a real
harness account or a network test.

---

## 1. `vowp graph --json` should print JSON

**Labels:** `good first issue`, `cli`, `bug`

### Context

The CLI contract says every command supports `--json`
(`docs/design/architecture.md` and `docs/design/cli.md` §1). `vowp graph` currently ignores the global
flag: the normal read path always prints ASCII, and `--mermaid` always prints Mermaid. A script that
asks for machine-readable graph data gets text.

### What to do

In `crates/vowp-cli/src/cmd/graph.rs`, make the normal read path emit one JSON document when
`ctx.json()` is true:

```json
{"jobs": [/* Job objects */], "edges": [/* Edge objects */]}
```

Keep `--export` and `--import` behaviour unchanged. If `--mermaid --json` is ambiguous, either make it
an input error or include the Mermaid text in the document; document the choice in
`docs/design/cli.md` §3.

### Acceptance

- `vowp graph --json` prints valid JSON and no human text.
- The jobs are the same objects as `vowp job ls --json`, and the edges include every kind.
- `vowp graph` and `vowp graph --mermaid` still print exactly the current output.
- Add an integration test in `crates/vowp-cli/tests/graph.rs`.

---

## 2. Show configured gates in `vowp job show --gates`

**Labels:** `good first issue`, `cli`

### Context

`vowp job show <id> --gates` prints only recorded gate results
(`crates/vowp-cli/src/cmd/job.rs`, around the `if args.gates` block). Before the first run the section
is empty, so a reader cannot tell which gates the job will run or whether the configured set is what
they expected.

### What to do

Print every gate in `job.gates` first, each with a `not run` marker, then merge in the latest recorded
result for a gate that has one. Keep the existing evidence tail for recorded results. Do not change
the JSON shape; `job show --json` already returns the configured `gates` array.

Suggested human shape (keep it simple and stable):

```text
gates:
  report        not run
  secret_scan   not run
  cmd           pass  120ms  exit 0
```

### Acceptance

- A freshly added job lists every configured gate as `not run`.
- After a run, each gate shows its recorded pass/fail, duration and evidence tail.
- Add a test in `crates/vowp-cli/tests/job.rs` that covers both states.

---

## 3. Do not require API keys for harness-owned logins

**Labels:** `good first issue`, `cli`, `bug`

### Context

`check_providers` in `crates/vowp-cli/src/cmd/doctor.rs` collects every provider from every profile and
warns when `<PROVIDER>_API_KEY` is absent. Claude Code normally uses its own signed-in login, so a
healthy setup gets a `CLAUDE_API_KEY: absent` warning even though no key is needed. The warning is
misleading for new users.

### What to do

Classify a provider as harness-owned when every rung that uses it is `claude-code` (consider opencode's
own login separately). Report that as `ok` with a detail such as
`claude-code uses its own login; no API key required`. Keep the existing `present`/`absent` report for
providers that a key-driven harness needs, and never read or print the value.

If the distinction is not reliable for opencode, document that and warn only for providers with no
harness-owned rung. Do not weaken `doctor_never_prints_env_values`.

### Acceptance

- A project whose only ladder is `claude-code:...` and whose environment has no `CLAUDE_API_KEY` gets
  an `ok` providers check.
- A project with an opencode rung still reports the missing key.
- Add a test to `crates/vowp-cli/tests/doctor.rs` using the fake `PATH`.

---

## 4. Make `session watch` incremental and add `--follow`

**Labels:** `good first issue`, `sessions`, `enhancement`

### Context

`vowp session watch --once` imports Claude Code transcripts, but the implementation in
`crates/vowp-cli/src/cmd/session.rs` stores a processed **line count**, re-reads the whole file every
time, and rejects `--follow` for a TTY. A line inserted anywhere in the transcript shifts the count.
`docs/design/sessions.md` §7 specifies a byte offset, whole-line increments and a two-second poll.

### What to do

- Store the byte offset and the last complete line (for example under the existing
  `watch:<native>` key, or a small `watch.json` next to the node state).
- Read only the suffix after the offset; do not consume a partial final line until it ends with `\n`.
- Add `--follow`: poll every two seconds and exit cleanly on Ctrl-C. Keep `--once` behaviour for
  scripts and tests.
- Never write to Claude's transcript directory; open it read-only.

The optional `ClaudeTranscriptImporter` sketch in `docs/design/crates/vowp-core.md` is not shipped; do
not add a stub that pretends to work.

### Acceptance

- Append lines between two `--once` runs: only the new events are appended.
- A growing fixture imported with `--follow` produces no duplicates.
- A partial final line is not imported before its newline arrives.
- Add tests in `crates/vowp-cli/tests/session.rs` with a temporary transcript directory; no real
  `~/.claude` and no network.

---

## 5. Anchor interval schedules to their creation time

**Labels:** `good first issue`, `core`, `bug`

### Context

`ScheduleSpec::parse` has no anchor and `interval_next_after` aligns `every <n>` to the Unix epoch
(`crates/vowp-core/src/schedules/spec.rs`). The design (`docs/design/loops.md` §11) says the anchor is
the schedule's `created_at`. A schedule created at 10:07 with `every 30m` currently fires at 10:30,
not 10:37.

### What to do

- Carry an anchor into `ScheduleSpec::next_after` (or add `next_after_anchored`) and pass
  `schedule.created_at` from `sync_from_config`, `catch_up` and `tick`.
- Keep cron behaviour unchanged.
- Existing rows already have `created_at` in the `schedules` table; no schema migration is needed.
- Update the module comment that documents the epoch deviation.

### Acceptance

- A schedule created at 10:07 with `every 30m` next runs at 10:37.
- A restart before 10:37 does not move the next run.
- Add tests in `crates/vowp-core/tests/schedules_spec.rs` and
  `crates/vowp-core/tests/schedules_engine.rs` using a fake clock.

---

## 6. Add state colours to `vowp graph --mermaid`

**Labels:** `good first issue`, `graph`, `enhancement`

### Context

`render_mermaid` in `crates/vowp-graph/src/render.rs` emits plain nodes and edges. State colours are
marked as v0.2 in `docs/design/cli.md` and `docs/design/crates/vowp-graph.md`. This is a self-contained
first step towards a readable graph in the web window.

### What to do

- Emit deterministic `classDef` lines for state groups, for example running, waiting
  (pending/ready/reviewing/fixing/merging/gating), failed/parked and done.
- Assign each node a class.
- Keep node keys, labels and edge order unchanged; escape labels as today.
- Do not add colours to the ASCII renderer.

### Acceptance

- The Mermaid output contains the class definitions and assignments in a stable order.
- The existing golden test is updated deliberately, not regenerated blindly.
- Add a test in `crates/vowp-graph/tests/render.rs` that covers at least one state from each group.

---

## 7. Add an `[env.<name>].build` command

**Labels:** `good first issue`, `promote`, `config`

### Context

The promotion `build` guard in `crates/vowp-core/src/promote/guards.rs` falls back to
`[merge].check_cmds`, then `[project].check_cmds`. `EnvConfig` in
`crates/vowp-protocol/src/config.rs` has no `build` field, so `build = [...]` is rejected as an
unknown key and an environment cannot name its own build command.

### What to do

- Add an optional `build: Vec<String>` to `EnvConfig`.
- In the `build` guard, run the environment's commands in the checkout when set; otherwise keep the
  current fallback and its recorded note.
- Document the precedence in `docs/design/architecture.md` §5.
- Keep `deny_unknown_fields` and the existing schema export intact.

### Acceptance

- `[env.prod].build = ["make build"]` parses and the guard runs that command.
- Without `build`, the existing fallback is unchanged.
- Add a config parse test and a guard test with a fake command script.

---

## 8. Copy a job's brief into its worktree

**Labels:** `good first issue`, `core`, `bug`

### Context

A brief written by `vowp job add --brief-file` lives in the project checkout. `write_brief` in
`crates/vowp-core/src/loops/dispatch.rs` only checks the worktree copy and otherwise writes a generic
fallback, so the worker can read a different brief than the planner wrote.

### What to do

- When the worktree brief is absent, read the project-checkout copy at `<repo>/<job.brief>` and write
  that content into the worktree.
- Use the generic fallback only when neither file exists.
- Never overwrite an existing worktree brief.

### Acceptance

- A job added with a custom brief and then dispatched has that exact brief in its worktree.
- A job with no source brief still gets the generated fallback.
- Add a test in `crates/vowp-core/tests/loops_supervisor.rs` (or the dispatch test module) that
  creates a project brief, dispatches the job and compares the worktree file.

---

## 9. Probe a remote node before a cross-node switch

**Labels:** `help wanted`, `core`, `sessions`

### Context

`crates/vowp-core/src/session/switch.rs` has a documented `TODO` at the target-probe step: a switch
whose `to_node` is another node only probes the local adapter registry. The design
(`docs/design/sessions.md` §5 step 3) calls for `vowp --node <n> doctor --json` before attaching, so a
missing harness on the target aborts the switch instead of failing mid-turn.

### What to do

- When `to_node` differs from this node, run the remote doctor check through the injected
  `CommandRunner`, using the node's SSH target and the same `ssh -o BatchMode=yes … vowp …` argv shape
  as `--node`. Keep the argv construction inside core or in a small shared helper; do not make core
  depend on `vowp-cli`.
- Abort with `AdapterUnavailable` and leave the session on the source when the probe is not ok.
- Keep local switches unchanged.
- Do not read tokens or print environment values.

### Acceptance

- A fake SSH that reports a failed doctor prevents the attach and leaves the session on the source.
- A healthy fake SSH allows the switch.
- Add a test in `crates/vowp-core/tests/switch.rs`; no real network, no real `~/.vowp`.
- Remove the `TODO` and update the release-notes limitation.

---

## 10. Let `vowp schema` work without an initialised home

**Labels:** `good first issue`, `cli`, `bug`

### Context

`vowp schema` reads only static protocol types, but `Ctx::build` rejects a fresh `--home` before the
command runs (`crates/vowp-cli/src/lib.rs`, `crates/vowp-cli/src/ctx.rs`). A contributor cannot list
the exported schemas until `vowp init`, which is unnecessary for a read-only type listing.

### What to do

- Run `schema` before the project/home guard, like `doctor`, and pass it only the global `--json` flag
  and the streams.
- Keep the existing output shape, type names and exit codes.
- Leave commands that need a project or database unchanged.

### Acceptance

- `vowp --home <fresh-temp-dir> schema` exits 0 and lists the type names.
- `vowp schema Job` prints the same document as before.
- A command that needs a project still fails with the `vowp init` hint.
- Add a test in `crates/vowp-cli/tests/maint.rs` with an empty temporary home.
