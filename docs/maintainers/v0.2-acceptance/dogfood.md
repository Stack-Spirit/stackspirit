# v0.2 acceptance — dogfood orchestrator (T5)

This is the acceptance record for §13.3 card `v2-t5-dogfood`: the current
orchestrator moves into a VOWP orchestrator session on the VPS home (§7.6) and
runs the rest of the release from there. It is also the runbook for the live
move.

**Result on the tested build: the automatable rehearsal PASSES. The live move
on the production home and the owner's laptop/phone joins were NOT RUN.** The
planner's 2026-11-11 **Owner-device scope** decision defers that owner-only
step to the owner, not waived; it is recorded in [PENDING OWNER
ACCEPTANCE](#pending-owner-acceptance) with exact commands, expected results
and "Not run". The rehearsal found one product gap: a project-bound
orchestrator's own directory is outside the project's cwd allow-list, so the
first takeover/capsule fails unless the orchestrators root is added. The
workaround is in this record and the fix belongs in a follow-up job; it is not
hidden in the public claim.

The automatable rehearsal covered, on a temporary VOWP home on the VPS:

- creation of a real `opencode` orchestrator with `vowp orchestrator new` and
  its desktop, phone and `vowp chat` join instructions (opencode v2.0.28);
- seeding that session from the current orchestrator's handoff, saved status
  and a `vowp session capsule` of a prior transcript;
- the orchestrator answering through `vowp chat` and confirming the seed;
- the orchestrator's `vowp mcp` server reading the fleet and adding a release
  job, with the session/surface recorded as the actor;
- the home supervisor running that job through gates and the merge queue, and
  the same session then reporting the live release view (two done jobs, the
  last merge commit and branch);
- `orchestrator stop`/`resume` keeping the same session id while refreshing
  the app join block.

## Environment

- Date: 2026-11-11 (UTC).
- Worktree: `job/v2-t5-dogfood`, rebased onto `origin/main` `e41459f`; this is
  a docs-only branch. The rehearsal binary was built from the earlier
  `ce4de07`; the later `f-project-path-default` commits only default
  `project.path`, which the fixture pins explicitly, so the evidence is
  unchanged.
- Binary under test: built with `cargo build -p vowp-cli --bin vowp`; it
  reports `vowp 0.1.0` (the package version during the v0.2 pre-release), SHA-256
  `a210589c34887af515d1c18d36f4d8aecfad868b092be473c47f507e06f9e795`.
- Host: Linux x86_64, kernel 6.8, 8 cores; the host also runs the night-shift
  jobs, so capacity observations below are real and intentionally conservative.
- opencode: `opencode v2.0.28` (the version the S3 spike pinned). tmux 3.4.
- Temporary home: `/tmp/opencode/t5-dogfood/vowp-home`. Private `HOME`
  `/tmp/opencode/t5-dogfood/user-home`; private `TMUX_TMPDIR`
  `/tmp/opencode/t5-dogfood/tmux`; the tmux socket was
  `vowp-b4751ee94b0c443d` (recorded by the home in `tmux-server`). `TMUX`,
  `TMUX_PANE` and `VOWP_CONTROL_HOME` were unset; no default tmux server was
  addressed. The real `~/.vowp` was not the home under test; every rehearsal
  command passed the temporary home explicitly. (Other night jobs wrote
  unrelated projects there after this run; they are not part of this
  evidence.)
- No real harness model turn or provider call was made by the rehearsal. The
  app host was the real opencode server; the executor was a deterministic CLI
  stand-in that reads the real `vowp fleet status --json` for its answers.

The rehearsal helper and its fixture live outside the repository in
`/tmp/opencode/t5-dogfood`; nothing from them is committed. The seed includes
the private handoff file, so only sanitized excerpts appear here.

## What the rehearsal ran

### 1. Home and fixture project

A throwaway bootstrap checkout created the home:

```sh
V=/path/to/vowp
export HOME=/tmp/opencode/t5-dogfood/user-home
export VOWP_HOME=/tmp/opencode/t5-dogfood/vowp-home
export TMUX_TMPDIR=/tmp/opencode/t5-dogfood/tmux
unset TMUX TMUX_PANE VOWP_CONTROL_HOME
mkdir -p "$HOME" "$VOWP_HOME" "$TMUX_TMPDIR"

cd /tmp/opencode/t5-dogfood/bootstrap
git init -q -b main
git -c user.name='T5 dogfood' -c user.email='t5@example.invalid' \
    commit -q --allow-empty -m 'bootstrap base'
"$V" --home "$VOWP_HOME" init --name bootstrap --no-framework --quiet
"$V" --home "$VOWP_HOME" project rm bootstrap --yes --quiet

bash scripts/acceptance/fixtures/project.sh hello /tmp/opencode/t5-dogfood/projects/release \
    --name release
cd /tmp/opencode/t5-dogfood/projects/release
"$V" --home "$VOWP_HOME" project add "$PWD" --quiet
```

The fixture is an ordinary git checkout with a local bare `origin` and a
deterministic fake worker/verifier. The rehearsal added four fixture-only
things to its `vowp.toml`, all committed in the fixture (not in the product
tree): `project.path` and `worktree_root` (the tested binary's session
allow-list needs them; the later `f-project-path-default` commit defaults
`project.path` from the checkout), a `cwd_allow` entry for the private
orchestrators root, and two fake CLI harnesses (`t5-orchestrator` for the
orchestrator turns, `t5-keeper` for the capsule prompt). The worker and
verifier prompt handling is the T0 fixture's existing fake worker.

### 2. The orchestrator session and joins

```sh
cd /tmp/opencode/t5-dogfood/projects/release
"$V" --home "$VOWP_HOME" --project release orchestrator new \
    --name dogfood --app opencode --executor cli:t5-orchestrator
```

Output (the port and native session are the real ones from the run):

```text
orchestrator dogfood (opencode, running) session 01M74Y7AA1C00NCVM2C454N7KR
  directory: /tmp/opencode/t5-dogfood/vowp-home/orchestrators/dogfood
  desktop: 1. On the control machine: vowp tunnel open 'dogfood' --remote-port 44253
2. On the home: cd '/tmp/opencode/t5-dogfood/vowp-home/orchestrators/dogfood' && XDG_STATE_HOME='.../.opencode/state' 'opencode' pair
3. Open the one-time link it prints (5 minutes, single use) in the opencode desktop app, replacing its port with the tunnel's local port.
  phone: ssh -t <home-ssh-target> 'cd '\''.../orchestrators/dogfood'\'' && '\''opencode'\'' --server http://127.0.0.1:44253 --session '\''ses_e361c4c21ffeAlF5FLyyhi1jMB'\'''
  chat: vowp chat 01M74Y7AA1C00NCVM2C454N7KR
```

The app host was a real `opencode serve` process. The run's
`.opencode/service.log` records the loopback URLs it used (`44253` and, after
the stop/resume check, `38987`), and the resumption capture
`evidence/opencode-host.json` / `evidence/opencode-listener.txt` records a live
`127.0.0.1:38835` listener and its `host.json` fields. (`host.json` is removed
by `orchestrator stop`, which is why the listener capture is kept separately.)
`opencode pair` was not run (that is the owner's one-time step). `vowp
orchestrator ls` reported:

```text
NAME                     APP          STATE      SESSION
dogfood                  opencode     running    01M74Y7AA1C00NCVM2C454N7KR
```

The orchestrator's `opencode.json` registered `vowp mcp` with the session,
surface and `lease.token` path, and `AGENTS.md` contained the instructions
template from `framework/templates/ORCHESTRATOR.md`.

### 3. Seed from the handoff, status and capsule

The preceding transcript was created as a VOWP session, given two fake turns,
and capped:

```sh
OLD=$("$V" --home "$VOWP_HOME" --project release --json session new \
    --title 'current orchestrator transcript' \
    --executor cli:t5-orchestrator --cwd "$PWD" | jq -r .id)
printf '%s\n%s\n/quit\n' 'turn one of the previous transcript' \
    'turn two of the previous transcript' |
  "$V" --home "$VOWP_HOME" --project release chat "$OLD"
"$V" --home "$VOWP_HOME" --project release session capsule "$OLD" --print \
  > /tmp/opencode/t5-dogfood/seed/capsule.md
```

The capsule wrote a `cap_...` id; the seed file then joined, on one line:

- the outgoing orchestrator's `ORCHESTRATOR-HANDOFF.md`;
- its saved `ORCHESTRATOR-STATUS.md`;
- the current `board.txt`;
- the capsule markdown from `session capsule --print`.

The actual prompt was sent with `--takeover` because the opencode host held the
lease:

```sh
tr '\n' ' ' < /tmp/opencode/t5-dogfood/seed/seed-prompt.md |
  { cat; printf '\n/quit\n'; } |
  "$V" --home "$VOWP_HOME" --project release chat \
     01M74Y7AA1C00NCVM2C454N7KR --takeover
```

The session answered:

```text
ACK. I am the VOWP orchestrator. I read the orchestrator handoff, the saved
status and the capsule of the previous transcript. I will read the snapshot at
the start of every turn, state what is running and waiting, propose a next step
and wait for the owner before applying anything.
```

The session log had **26 events**, 2 turns, 1 switch event and 1 capsule
(`cap_01M74Y7H3ZSBHERGMFD45FTDXN`, the takeover capsule). The first turn was
the takeover switch; the seed user message was
written with `note: handoff` and the capsule text; the ACK is the first
assistant event. This is the same session id the app and chat joins use.

The capsule in this rehearsal came from a controlled VOWP transcript, not from
the outgoing orchestrator's private opencode transcript. There is no
opencode-to-VOWP transcript import today; the live move must have the outgoing
orchestrator write its own capsule/handoff (or use a future import). The VOWP
capsule, switch and seed mechanics exercised here are the same.

The **single-line** step is not cosmetic. `vowp chat` reads one prompt per line;
sending the handoff as a multi-line block ran every line as a separate turn
(observed in an earlier attempt: 1604 events and one turn per line). The live
move must use the one-line form above or a future import command. See open item
2.

### 4. The orchestrator's MCP tools add a release job

The orchestrator's `vowp mcp` server was driven with newline-delimited JSON-RPC
using the session, surface and token file written by `orchestrator new`:

```sh
O="$VOWP_HOME/orchestrators/dogfood"
S=$(jq -r .session_id "$O/orchestrator.json")
SURF=$(jq -r .surface_id "$O/orchestrator.json")
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t5","version":"1"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fleet_status","arguments":{}}}' |
  "$V" --home "$VOWP_HOME" --project release mcp \
      --session "$S" --surface "$SURF" --token-file "$O/lease.token"
```

`fleet_status` returned the live snapshot (the orchestrator card and the
project's jobs). A second call added a job:

```json
{"name":"job_add","arguments":{
  "id":"t5-release-final",
  "title":"T5 final release check",
  "kind":"build",
  "role":"worker",
  "owns":["src/t5-release-final.txt"],
  "gates":[{"report":{"fields":["status","branch","commits","tests"]}},
           {"owns_respected":true}],
  "brief_text":"# Goal\n\nWrite `src/t5-release-final.txt` and the job report.\n"
}}
```

The response identified the actor as
`session:01M74Y7AA1C00NCVM2C454N7KR/surface:01M74Y7AAJEPDFQJ8B0T8VS2JB`, and the
job started `pending` with the two requested gates. The same token's
`fleet_status` read still worked after the chat surface took the lease; reads
are deliberately allowed in follow mode. The write gate/takeover notice is the
T3 acceptance evidence; this run did not re-run the refusal timing.

### 5. The release job runs and merges from the session

The home supervisor was already running (`vowp up --detach` with the private
`TMUX_TMPDIR`). It dispatched the job through the fixture worker and the real
merge queue. The states were polled with `vowp job show t5-release-final
--json`; the job was created at `2026-11-11T09:15:16.809Z` and reached `done`
at `2026-11-11T09:19:07.412Z` (3m51s on the loaded host). The fixture's local
`origin/main` gained:

```text
a896547 t5-release-final: fake acceptance worker
7b0db1e fixture: put the fleet refresh flag in the right place
fdbfbc6 fixture: allow the orchestrator directory for capsule writes
750a8fc fixture: check the capsule markers in the seed
```

`vowp fleet status --json` then reported:

```json
{
  "jobs_by_state": {"cancelled": 2, "done": 2},
  "last_merge": {
    "job": "t5-release-final",
    "at": "2026-11-11T09:18:07.774Z",
    "commit": "a896547e560fb4a1e8710f69616869cd4368f0fc",
    "branch": "job/t5-release-final"
  },
  "running": []
}
```

### 6. The same session reports the live release view

A second chat turn, one line, was the orchestrator's status question:

```sh
printf '%s\n/quit\n' 'T5 RELEASE STATUS' |
  "$V" --home "$VOWP_HOME" --project release chat \
     01M74Y7AA1C00NCVM2C454N7KR --takeover
```

Output:

```text
Release view at 2026-11-11T09:19:12Z: supervisor running=true; project release
jobs: cancelled=2, done=2; last merge: {
  "job": "t5-release-final",
  "at": "2026-11-11T09:18:07.774Z",
  "commit": "a896547e560fb4a1e8710f69616869cd4368f0fc",
  "branch": "job/t5-release-final"
}.
```

The orchestrator read the real fleet view; it did not invent the state. This is
the "runs the rest of the release from there" path in miniature: the session
added a job through its tools, the home ran and merged it, and the session
reported the result.

### 7. Stop and resume keep the session

```sh
"$V" --home "$VOWP_HOME" --project release orchestrator stop dogfood
"$V" --home "$VOWP_HOME" --project release orchestrator resume dogfood
```

`stop` wrote `stopped`, released the lease and kept the session. `resume`
started the opencode host again on a new port (44253 → 38987) and a new native
opencode session, but the VOWP session id stayed
`01M74Y7AA1C00NCVM2C454N7KR` and the `chat:` join stayed the same.

## Findings

### 1. Project-bound orchestrator cwd is outside the allow-list (open item)

The first takeover with a capsule failed:

```text
capsule invalid after repair: refused: working directory
/tmp/opencode/t5-dogfood/vowp-home/orchestrators/night is outside the allow-list
```

`Orchestrator::create` puts the session cwd in
`~/.vowp/orchestrators/<name>/`. `SessionService::check_cwd` allows only
`project.path`, `project.worktree_root` and `[node].cwd_allow`. A project-bound
orchestrator therefore has a cwd that is outside every default allow-list root,
so the switch capsule on the first takeover fails. The rehearsal added the
orchestrators root to the fixture's `[node].cwd_allow` and the next takeover
succeeded; the production home must do the same until this is fixed.

Suggested follow-up (outside this job's owns): include the orchestrators root
in the default allow-list in `crates/vowp-core/src/session/service.rs` (or have
`vowp setup home`/`orchestrator new` write it), with a test that creates a
project-bound orchestrator and takes it over. This is the only product gap the
rehearsal found.

### 2. Seeding a chat session is line-oriented (operational gotcha)

`vowp chat` treats each line of stdin as one prompt. A handoff file pasted as
many lines becomes many turns, which is slow and pollutes the session log. The
one-line form (`tr '\n' ' '`) works and is used above. A future `vowp session
import` or a `chat --prompt-file` that sends one turn would make the live move
less error-prone; that is a follow-up, not a blocker.

### 3. Capacity brake held a verifier on the loaded host (expected behaviour)

The first release job, `t5-release-check`, used the default review gate. Its
verifier job waited with:

```text
waiting: swap growing 505 MiB/min
```

`vowp capacity report` showed the `swap_growth` brake active while other night
jobs were running. The brake behaved as designed (it never killed a run). The
rehearsal cancelled that job and used a no-review fixture job for the release
action; review-gate end-to-end coverage is T2/T3's evidence, not this card's.
On a busy home the orchestrator should report a brake and ask the owner before
overriding it.

### 4. The rehearsal used a deterministic executor, not the DeepSeek model

The real app host was opencode, but the turn executor was a deterministic CLI
stand-in that read the real fleet. A night worker cannot safely start the
current DeepSeek orchestrator's real model session on a temporary home: that
would require the owner's provider auth and a second live orchestrator. The
live move in PENDING OWNER ACCEPTANCE uses the real
`opencode:deepseek/deepseek-flash#max` executor; the mechanics it depends on
(session, app host, seed, capsule, MCP, supervisor, merge) are the ones
rehearsed here.

### 5. Owner joins were not run

The Mac desktop app, the phone over SSH and the real app-to-app continuation
need the owner's devices and a `vowp control init`/tunnel on the Mac. T3 already
proved the opencode transport and the lease refusal; this record does not
re-claim them. The exact T5 joins are below.

## PENDING OWNER ACCEPTANCE

These steps were **not run**. They are the live move and the owner's part of
the card; a worker must not replace or simulate either. The planner's
2026-11-11 **Owner-device scope** decision defers this step to the owner, not
waived. Use the final build from this branch: it is docs-only, the rehearsal
evidence is from the earlier `ce4de07` binary, and the workspace gates run on
the rebased base `e41459f`.

### 1. Move the current orchestrator into the live VOWP home

The outgoing orchestrator and the owner do this together, between jobs, on the
VPS home that runs the release. The home must already have VOWP 0.2 and the
VOWP project registered (`vowp project add <vowp-checkout>`). This is the
dogfood move, not M1's production migration: M1 later keeps the same home with
its own backup and rollback plan.

Until open item 1 is fixed, add the orchestrators root to the project config so
the first takeover capsule can be written:

```toml
[node]
cwd_allow = ["/home/<user>/.vowp/orchestrators"]
```

then re-register the checkout (`vowp project add <vowp-checkout>`) and commit
the config. Create the session and start the real app host:

```sh
cd <vowp-checkout>
vowp orchestrator new --name release --project <project> --app opencode \
    --executor opencode:deepseek/deepseek-flash#max
```

The outgoing orchestrator writes `ORCHESTRATOR-SEED.md` with its handoff, its
ACK/status and a capsule summary it writes of its opencode transcript, all on
**one line** (or uses the one-line form below). There is no automated
opencode-to-VOWP transcript import today (Finding 4), so the outgoing
orchestrator supplies that summary. Then the new session is the only
orchestrator:

```sh
tr '\n' ' ' < ORCHESTRATOR-SEED.md |
  { cat; printf '\n/quit\n'; } |
  vowp chat <session> --takeover
```

Expected result: the ACK in the session log, a capsule id on the switch, and
no `outside the allow-list` error. If the error appears, open item 1's
`cwd_allow` workaround is missing.

### 2. opencode desktop app on the Mac

Use the join block printed by `orchestrator new`:

```sh
# On the control machine (the Mac):
vowp tunnel open release --remote-port <port>

# On the home, in the orchestrator directory:
cd /home/<user>/.vowp/orchestrators/release
XDG_STATE_HOME=.../.opencode/state opencode pair

# Open the printed one-time http://127.0.0.1:<port>/auth/connect/<code> link
# in the opencode desktop app, replacing its port with the tunnel's local port.
# Send: owner laptop turn
```

Expected result: the orchestrator project opens in the desktop app; the turn
appears in the live session; the phone check below sees it and its reply.

**Not run.**

### 3. The same live session on the phone over SSH

```sh
ssh -t <home-ssh-target> \
  'cd /home/<user>/.vowp/orchestrators/release && \
   opencode --server http://127.0.0.1:<port> --session <ses_...>'
# Send: owner phone turn
```

Expected result: the terminal UI opens on the same native session; the laptop
turn is visible; the phone turn appears in the desktop app. This is the phone's
SSH app path, not a browser or a VOWP-specific app.

**Not run.**

### 4. The chat fallback on the same session

```sh
ssh -t <home-ssh-target> 'vowp chat <session>'
# Or, on the home:
vowp chat <session>
```

Expected result: the same session, with the new turns visible; `/status` shows
the lease and `/capsule` writes a capsule. This is the guaranteed path when no
app is installed.

**Not run.**

### 5. Continue the release from the session

Ask the orchestrator for the real picture and one action:

```text
read the fleet status, queue and inbox; report what is running, waiting and
blocked; propose the next release step and wait for my yes
```

Expected result: the orchestrator reads `vowp fleet status --json`,
`vowp fleet queue --json` and `vowp fleet inbox --json` (or the equivalent MCP
tools), reports the actual times/state, and after the owner's explicit yes uses
`vowp mcp`/the matching CLI to apply one action. The write must record the
session/surface actor, as the rehearsal's `job_add` did.

**Not run.**

## Gates

Docs-only branch. The record was written after the rehearsal. Commands and
results are in the report front matter; the final gate logs are
`/tmp/opencode/t5-dogfood/evidence/clippy.log` and
`/tmp/opencode/t5-dogfood/evidence/cargo-test.log` on the build host.

| Command | Result |
|---|---|
| `cargo fmt --all` and `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 (`Finished`, no warnings) |
| `cargo test --workspace` | exit 0; every reported test group passed, 0 failed (2 ignored) |
| `git diff --check` | exit 0 |
| `bash docs/tests/check-links.sh` | exit 1 with the same four pre-existing broken links outside this file (`cost-dashboard.md` and three `docs/roadmap.md` links to moved v0.2 design pages); this file adds no broken link |

Gate note: the full workspace check flaked once in
`setup_verify_fixture_runs_twice_on_one_home` under parallel load. This is the
known fixture flake recorded in `docs/roadmap.md` (item 12, "Make the second
`setup verify` fixture run deterministic under load"); the isolated
`cargo test -p vowp-cli --test setup_verify` target passes 11/11 and the full
workspace passes in local reruns. It is not caused by this docs-only change.

The only changed repository path is
`docs/maintainers/v0.2-acceptance/dogfood.md`; the fixture and the rehearsal
helpers live under `/tmp/opencode/t5-dogfood` and are not committed.

## Evidence

All paths are on the build host; they are the raw logs, not committed.

- `/tmp/opencode/t5-dogfood/evidence/orchestrator-new-clean.txt` — session id,
  directory, desktop/phone/chat join block.
- `/tmp/opencode/t5-dogfood/evidence/opencode-host.json` and
  `opencode-listener.txt` — a preserved live `host.json` capture and the
  `ss -ltn` listener row for the resumed app host.
- `/tmp/opencode/t5-dogfood/evidence/seed-chat-clean.txt` — ACK from the seeded
  session.
- `/tmp/opencode/t5-dogfood/evidence/session-events.jsonl` and
  `session-summary.json` — 26 events, 2 turns, 1 capsule.
- `/tmp/opencode/t5-dogfood/evidence/mcp-job-add-final.jsonl` — the
  `fleet_status` read and `job_add` response with the session/surface actor.
- `/tmp/opencode/t5-dogfood/evidence/status-chat-clean.txt` — the live release
  view with two done jobs and the merge commit.
- `/tmp/opencode/t5-dogfood/evidence/environment.txt` — binary hash, versions,
  private socket, fleet/capacity/job states and the fixture's origin log.
- `/tmp/opencode/t5-dogfood/evidence/orchestrator-stop-resume.txt` — stop and
  resume with the same session id.
- `/tmp/opencode/t5-dogfood/evidence/cwd-allow-error.txt` — the allow-list
  failure from the first project-bound orchestrator.
- `/tmp/opencode/t5-dogfood/evidence/clippy.log` and `cargo-test.log` — the
  final gate logs (clippy exit 0; every test group passed, 0 failed).
- `/tmp/opencode/t5-dogfood/seed/` — the handoff/status/capsule seed and the
  one-line form. These contain the private handoff and are not committed.
