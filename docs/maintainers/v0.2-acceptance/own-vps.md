# v0.2 acceptance — move the owner's VPS onto v0.2 (M1)

This is the acceptance record and runbook for §13.3 card `v2-m1-own-vps`: move
the owner's existing VPS onto VOWP 0.2, keep the owner's own tmux sessions and
NorthNoir workers untouched, and write and test the rollback before the switch.

**Result on the tested build: the move plan, the night-supervisor backup and
restore drill, the v0.1-home migration and rollback drill, and the fresh
v0.2 home, project and backup/restore drill PASS. The live switch on the
owner's VPS was NOT RUN.** It cannot run until two gates are met:

1. the published **v0.2.0 release** exists (the version bump and release notes
   are merged, but the tag and release assets are not published yet); and
2. the **owner has read this plan and said yes** before the switch.

At the time of this record the night supervisor still had two jobs running,
including this one, so stopping it would have destroyed the work it was
recording. No live service, database, repository, tmux session or NorthNoir
process was changed. Everything run against live state was read-only:
`systemctl --user show`/`list-timers`, `ps`, `gh release list`, `git status`,
`df`/`du`, and SQLite queries opened with `mode=ro`. The owner's default tmux
server was never addressed with a tmux command, and no NorthNoir process was
signalled. The rehearsal home, fixture projects, backup archive and restored
state all live under `/tmp/opencode/v2-m1-own-vps/`.

The exact owner-only steps are under [PENDING OWNER
APPROVAL](#pending-owner-approval); they are **not waived**.

## Environment

- Date: 2026-11-11 (UTC). The live inventory was taken at 18:48 UTC.
- Worktree: `job/v2-m1-own-vps`, based on `origin/main` `c843860`
  (`docs: keep internal card names out of the public release notes`). This is
  a docs-only branch.
- Binary under test: built from this tree with
  `cargo build --release -p vowp-cli --bin vowp`; it reports `vowp 0.2.0`.
  SHA-256:
  `22a99a3b0d4e9e48ad11645a9822900df81c549a7e472f22335c144ac524435b`
  (the same binary SHA as the release-preparation run at `c843860`).
- v0.1 binary for the migration/rollback drill: the v0.1.0 release binary,
  `vowp 0.1.0`, SHA-256
  `5fc76ec708641f9dca83045ca826ea589cee8438e83c2c15ff6159d8fa9dbc0b`.
- Host: Linux x86_64, kernel 6.8, 8 cores, 25,199,173,632 bytes of RAM
  (about 23 GiB), 184,986,853,376 bytes free on `/` (about 172 GiB).
- Temporary state: `TMUX_TMPDIR=/tmp/opencode/v2-m1-own-vps/rehearsal/tmux`,
  with `TMUX` and `TMUX_PANE` unset. Every tmux user in the rehearsal was the
  acceptance generator or the v0.1/v0.2 binaries using their own home-local
  socket. No command was sent to the default tmux server.
- No real harness model turn, provider call, secret, network download or
  Docker container was used by the rehearsal. The v0.1 home generator ran
  with `--binary`, so it did not download a release. The fixture projects use
  the acceptance fake harness. The only network reads were release/tag queries
  (`gh release list`, `git ls-remote`), which change nothing.
- The rehearsal used the rig's v0.1 generator and fixture scripts directly on
  the host rather than in a Docker container: M1's target is this host's own
  user layout and night state, which a fresh container does not contain. It
  still used a temporary `VOWP_HOME`, the rig's private tmux socket and fake
  harness, and no host home or live state was modified.

## Current state: read-only inventory

These are the facts the move plan is built on. Paths use `<home>` for the
owner's Unix home and `<vps>` for the SSH target; the real values are not
recorded here.

| Item | Observed state |
|---|---|
| VOWP checkout | `<home>/vowp`, branch `main`, clean, HEAD `c843860` |
| Project config | no `<home>/vowp/vowp.toml` and no `<home>/vowp/.vowp`; the repository has not been initialised as a VOWP project |
| Installed CLI | no `vowp` on `PATH`; no per-user install yet |
| Night supervisor | `vowp-night.service` active, MainPID 786117, started 09:16:54 CET; state in `<home>/vowp-night` |
| Night DB | 199 jobs: 186 done, 11 cancelled, 2 running; 1,956 runs |
| Night state size | 17,188,216,533 bytes total; `target/` is 16,582,997,488 bytes and `runs/` is 531,100,560 bytes. A state backup excludes `target/`; it includes `runs/`, jobs, reports, worktrees and scripts. |
| Owner tmux | the default tmux server was started at 07:49:27 on 2026-11-09 (PID 495552). The owner sessions live there; they were **not listed or addressed** with tmux, by policy. |
| NorthNoir | worker processes were visible in the process table (the Playwright MCP and `gcloud storage cp` workers). They were not signalled or modified. |
| Stale home | `<home>/.vowp` exists with schema version 2 and only the setup-verify fixtures `_node`, `k4-schema3-g005`, `k4-schema4-k99w` (3 sessions, last write 10:30 CET). It is not the owner's production truth and must be preserved, not deleted. |
| Control home | `<home>/.vowp-control` does not exist. |

### Release gate is not met

```sh
gh release list --repo syver777/vowp --limit 5 \
  --json tagName,isDraft,isLatest,publishedAt
# [{"isDraft":false,"isLatest":true,
#   "publishedAt":"2026-11-10T09:16:12Z","tagName":"v0.1.0"}]

git -C <home>/vowp tag -l
# v0.1.0-rc.1
```

The release-preparation work is merged at `c843860` (workspace version
`0.2.0`, release notes and launch checklist), but **no `v0.2.0` tag or release
assets exist**. The install phase below must therefore refuse to continue until
`gh release view v0.2.0 --repo syver777/vowp` succeeds and the x86_64 Linux
archive and `SHA256SUMS` are downloadable.

### The stop condition is not met

At inventory time the night supervisor was still running this job and
`f-release-version-sweep`. The plan's phase 2 stop condition is therefore
false now; that is expected. The owner/orchestrator runs the plan after the
last job has finished, not from inside a running job.

## Decisions the plan makes (for the owner to approve)

These are the choices that need one yes. They keep the owner's existing account
and workload rather than creating a second installation beside it.

1. **Use the existing login account, not a new `vowp` user.** The setup
   command `vowp setup home` creates a dedicated `vowp` user whenever
   passwordless sudo is available. On this VPS that would put the home under
   `/home/vowp` and leave the existing `<home>/vowp` checkout, night state and
   owner sessions in a different account. The setup contract itself says to
   stop and ask when the setup agent runs on the home itself. The plan
   therefore uses the documented manual install path as the existing account,
   with the same `vowp` commands. A `--account`/`--no-user` option for
   `setup home` is a product follow-up, not a reason to move the account.
2. **Preserve the stale `<home>/.vowp`.** It is v0.2 test data, not production.
   It is archived and moved to `<home>/.vowp.pre-m1-<stamp>`; it is not deleted.
   The new home is created fresh at `<home>/.vowp`.
3. **One project now: `vowp`.** Register `<home>/vowp` as project `vowp`.
   Other repositories (including NorthNoir) stay unregistered until the owner
   asks; the card says "and later other projects".
4. **Stop the old supervisor safely.** Disable its watchdog timer first, then
   `systemctl --user stop vowp-night`. Never `tmux kill-server`, never
   `kill-session`, never a default-server tmux command. The service has
   `KillMode=process`, so stopping it does not stop the owner's tmux sessions.
5. **Keep the owner's model policy.** The generated config is edited to use
   `opencode:deepseek/deepseek-flash#max` for planner, keeper, worker and
   verifier; the default `claude-code` planner rung is removed because Claude
   is disabled on this VPS by the owner's decision. The owner may choose a
   different executor before the switch.
6. **Orchestrator name and app.** Create an opencode orchestrator named
   `night` with executor `opencode:deepseek/deepseek-flash#max`. The owner can
   rename it; the join instructions are printed by the command and must be
   used verbatim for the Mac and phone.
7. **Review `push` before starting.** `vowp init` writes `push = true` for the
   project. The checkout's `origin` is its existing private remote. If the
   owner wants merged work to stay local during the first days, set
   `push = false` in `vowp.toml` before `up`; the owner can turn it on later.

## The move plan (show this to the owner once)

Run every phase as the owner's login account. Use a private `TMUX_TMPDIR` for
any command that might ask tmux; the VOWP binary uses its home-local socket, but
the habit protects the owner's default server. The whole plan is one approval:
do not ask for a yes per phase.

### Phase 0 — Gate and freeze

```sh
set -eu
H=<home>
T=<ssh-target>
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
B=<backup-root>/m1-$STAMP
mkdir -p "$B"

# 1. The published release must exist and be non-draft.
gh release view v0.2.0 --repo syver777/vowp \
  --json tagName,isDraft,assets >/dev/null

# 2. Key-only SSH from the control machine must still work.
ssh -o BatchMode=yes -o ConnectTimeout=10 "$T" true

# 3. No night job may be active. An empty output is the pass condition.
python3 "$H/vowp-night/night.py" job ls \
  | awk '$2 ~ /^(running|gating|verifying|merging)$/ {print}'

# 4. The old supervisor is running and its watchdog timer is armed.
systemctl --user is-active vowp-night
systemctl --user list-timers vowp-night-watchdog.timer --no-pager

# 5. Enough disk for a state backup plus the new home.
df -h "$H"
```

Stop if the release is missing or a draft, any job is active, SSH does not
work, or free disk is below 10 GiB. Record the old state (HEAD, service PID and
start time, default tmux server PID and start time) for the post-switch check.

### Phase 1 — Take a live safety snapshot of the night state

The supervisor is still running here. Take a consistent SQLite snapshot with
the backup API (it is safe with a live writer) plus copies of the small state
files. Do not copy the live database with plain `cp`.

```sh
python3 - "$H/vowp-night/night.db" "$B/night.db" <<'PY'
import sqlite3, sys
src = sqlite3.connect(sys.argv[1], timeout=30)
dst = sqlite3.connect(sys.argv[2])
src.backup(dst)
dst.execute("pragma journal_mode=delete")
dst.commit()
print("integrity", dst.execute("pragma integrity_check").fetchone()[0])
print("jobs", dst.execute("select count(*) from jobs").fetchone()[0])
print("runs", dst.execute("select count(*) from runs").fetchone()[0])
PY

cp -a "$H/vowp-night/board.txt" \
      "$H/vowp-night/NIGHT-REPORT.md" \
      "$H/vowp-night/PLANNER-STATE.md" "$B"/
cp -a "$H/vowp-night/jobs" "$H/vowp-night/briefs" "$B"/
sha256sum "$B/night.db" > "$B/night-db.sha256"
```

Expected: `integrity ok`, and the job/run counts match the read-only inventory.
This snapshot is the "before stop" copy the card asks for. It is not the final
archive; the final archive is taken after the stop so no file changes under
`tar`.

### Phase 2 — Stop only the night supervisor, then take the clean backup

The watchdog restarts the supervisor when its board is stale, so disarm the
watchdog **before** stopping the service. Never touch the default tmux server.

```sh
systemctl --user disable --now vowp-night-watchdog.timer
systemctl --user stop vowp-night
sleep 2
systemctl --user is-active vowp-night          # must print: inactive
```

Confirm the owner's tmux server is still the same process:

```sh
ps -eo pid,lstart,args | grep 'tmux new-session -d -s vowp-night' | grep -v grep
```

That process may remain because the service has `KillMode=process`; it is the
board window, not a job. Do not stop it with tmux. The owner can close the
window later.

Now take the clean archive. This is the rollback source.

```sh
tar -czf "$B/night-state.tar.gz" -C "$H/vowp-night" \
  --exclude=./target --exclude=./__pycache__ \
  --exclude='./night.db.bak-*' .
gzip -t "$B/night-state.tar.gz"
sha256sum "$B/night-state.tar.gz" > "$B/night-state.sha256"
tar -tzf "$B/night-state.tar.gz" | wc -l
```

`target/` is build output and is excluded; `runs/` is included because it is
the raw supervisor history. This is a private backup: it contains job briefs,
reports, prompts and run logs, so keep it outside the repository and never
copy it into a public tree. The VOWP secrets store lives in the new home, not
in `vowp-night`, and is not part of this archive. Record the archive size,
entry count and SHA-256.

Verify the restore before changing anything:

```sh
mkdir -p "$B/restore-check"
tar -xzf "$B/night-state.tar.gz" -C "$B/restore-check"
NIGHT_BASE="$B/restore-check" python3 "$H/vowp-night/night.py" job ls
python3 - "$B/restore-check/night.db" <<'PY'
import sqlite3, sys
c = sqlite3.connect("file:" + sys.argv[1] + "?mode=ro", uri=True)
print("integrity", c.execute("pragma integrity_check").fetchone()[0])
print("jobs", c.execute("select count(*) from jobs").fetchone()[0])
print("runs", c.execute("select count(*) from runs").fetchone()[0])
PY
```

The `night.py job ls` must read the restored database and print the same job
counts as the live snapshot. If it does not, stop and do not proceed.

### Phase 3 — Preserve the old home and install 0.2

Archive the stale home, then move it aside. The new home will be created by
`vowp init`; nothing is deleted.

```sh
tar -czf "$B/dot-vowp.pre-m1.tar.gz" -C "$H" .vowp
mv "$H/.vowp" "$H/.vowp.pre-m1-$STAMP"

cd "$H/vowp"
git status --short                 # must be empty
scripts/install.sh --download --version 0.2.0 \
  --no-service --no-cron --no-watchdog
"$H/.local/bin/vowp" --version     # must print: vowp 0.2.0
test -f "$H/.config/systemd/user/vowp.service"
```

`--no-service` installs the unit but leaves it disabled; the switch happens in
phase 5. If the release download or checksum fails, stop: do not fall back to
an unreleased local build for the live move.

### Phase 4 — Create the home and register the VOWP project

```sh
cd "$H/vowp"
"$H/.local/bin/vowp" init --name vowp --no-framework
git diff -- .gitignore vowp.toml
```

Edit `vowp.toml` before committing. At minimum:

```toml
[profiles.planner]
ladder = ["opencode:deepseek/deepseek-flash#max"]
on_quota = "next_then_probe_back"

[profiles.keeper]
ladder = ["opencode:deepseek/deepseek-flash#max"]
on_quota = "next_then_probe_back"

[profiles.worker]
ladder = ["opencode:deepseek/deepseek-flash#max"]
on_quota = "next_then_probe_back"

[profiles.verifier]
ladder = ["opencode:deepseek/deepseek-flash#max"]
on_quota = "next_then_probe_back"
```

Also review `[node] max_workers`/`max_verifiers`, `cwd_allow`, and
`[project] push`. The generated file defaults to a `claude-code` planner rung;
replace it with the owner's chosen executor. Commit only the config files:

```sh
git add .gitignore vowp.toml
git commit -m "vowp: initialise the v0.2 home project"
git status --short                 # must be empty
"$H/.local/bin/vowp" --home "$H/.vowp" project ls
```

`project ls` must show `vowp` (and the reserved `_node` only with `--all`).

### Phase 5 — Start the home and verify it before moving the orchestrator

```sh
systemctl --user daemon-reload
systemctl --user enable --now vowp.service
systemctl --user is-active vowp.service    # must print: active

vowp doctor
vowp fleet status --refresh
vowp status
vowp project ls
```

Accept the switch only when `doctor` exits 0 and reports the home, database,
schema, project, git and tmux checks; `fleet status` names the `vowp` project;
and `project ls` is correct. Warnings for a missing watchdog are expected
because the 0.2 watchdog is a separate install choice; record them rather than
hiding them.

### Phase 6 — Move the orchestrator (owner-only)

Run this only after phase 5 passes. The outgoing orchestrator must stay alive
until the new session has answered once.

```sh
cd "$H/vowp"
vowp orchestrator new --name night --app opencode \
  --executor opencode:deepseek/deepseek-flash#max
```

The command prints the VOWP session id, the orchestrator directory, the
desktop instructions (`vowp tunnel open night --remote-port <port>` on the
control machine, then `opencode pair` on the home and the one-time link), the
phone command (`ssh -t <target> ... opencode --server ... --session ...`), and
the `vowp chat` command. Use those printed values verbatim; do not invent a
port or session id.

Seed the new session from the outgoing orchestrator's own handoff. The
outgoing orchestrator writes its `ORCHESTRATOR-HANDOFF.md`, its saved status,
and (optionally) a `vowp session capsule` of a prior VOWP transcript. Send the
seed as **one line**, because `vowp chat` is line-oriented:

```sh
tr '\n' ' ' < handoff-and-status.md \
  | { cat; printf '\n/quit\n'; } \
  | vowp --project vowp chat <session-id> --takeover
```

Then join the same session from the Mac opencode desktop app and the phone,
send one turn from each, and confirm both see the other. The exact T5 join and
seed commands, including the single-line gotcha and the fact that there is no
opencode-to-VOWP transcript importer, are in
[dogfood.md](dogfood.md#pending-owner-acceptance). Only after the new session
acknowledges the handoff and reads `vowp fleet status` does the outgoing
orchestrator stop acting. Never run two orchestrators at once.

### Phase 7 — Verify the move, then retire the old supervisor

```sh
vowp fleet status --refresh
vowp status
vowp orchestrator ls
systemctl --user is-active vowp.service
systemctl --user is-active vowp-night              # must print: inactive
systemctl --user is-active vowp-night-watchdog.timer  # must print: inactive

# The old unit stays on disk for rollback, but must not start at boot.
systemctl --user disable vowp-night
```

Check that the default tmux server PID and start time are unchanged from phase
0 and that NorthNoir workers were not signalled. Do not list sessions with
tmux; the process table and the owner's own windows are the check. Keep
`<home>/vowp-night` and the backup archive until the owner has used the new
home for a few days. The first real orchestrator turn should read the fleet,
state what is running and waiting, and add the next release job through the
normal job/MCP path, not by editing the database.

## Rollback (tested on copies)

Rollback is allowed at any phase. It never deletes the failed attempt.

1. Stop and disable the new home:

   ```sh
   systemctl --user stop vowp.service
   systemctl --user disable vowp.service
   ```

2. Preserve the new home, and restore the old stale home if the owner wants
   the machine exactly as it was:

   ```sh
   mv "$H/.vowp" "$H/.vowp.failed-m1-$STAMP"
   mv "$H/.vowp.pre-m1-$STAMP" "$H/.vowp"
   ```

   The stale home is only test data; restoring it is optional. Do not delete
   the failed home: it is the diagnosis copy.

3. Restore the night state from the clean archive if the old directory was
   changed (phase 2 normally leaves it untouched):

   ```sh
   mv "$H/vowp-night" "$H/vowp-night.failed-m1-$(date -u +%Y%m%dT%H%M%SZ)"
   mkdir -p "$H/vowp-night"
   tar -xzf "$B/night-state.tar.gz" -C "$H/vowp-night"
   ```

4. Re-enable the old supervisor and its watchdog:

   ```sh
   systemctl --user daemon-reload
   systemctl --user enable --now vowp-night-watchdog.timer
   systemctl --user enable --now vowp-night
   systemctl --user is-active vowp-night
   NIGHT_BASE="$H/vowp-night" python3 "$H/vowp-night/night.py" job ls
   ```

   The job list and counts must match the backup.

5. If the `vowp init` commit is the only change in the checkout, keep it or
   revert it with `git revert <sha>`; it is harmless to keep for a later
   retry. Do not push a revert unless the owner asks.

6. Reopen the old orchestrator on the Mac and stop the new session. The old
   orchestrator was never asked to stop acting until the new one had answered,
   so this is a normal return to the previous state.

For a v0.1 home that is migrated rather than replaced, the same record uses
`vowp migrate --rollback <pre-0.2-backup>`; that path is exercised below. A
fresh v0.2 home (the case here) rolls back by stopping the service and moving
the directory aside, as above.

## Rehearsal evidence

### 1. v0.1 home migration and rollback

A real v0.1 home with two completed jobs was generated with the acceptance
generator and the v0.1.0 binary:

```sh
export TMUX_TMPDIR=/tmp/opencode/v2-m1-own-vps/rehearsal/tmux
unset TMUX TMUX_PANE
V01=<release-cache>/vowp-0.1.0-x86_64-unknown-linux-gnu/vowp
bash scripts/acceptance/make-v01-home.sh \
  --dest /tmp/opencode/v2-m1-own-vps/rehearsal/v01 \
  --binary "$V01" --jobs 2
```

The manifest recorded schema 1, 2 jobs both `done`, 3 sessions, 3 runs, 21
session events, 6 gate results and 3 workers. Migration then ran on the
temporary home:

```sh
V=$CARGO_TARGET_DIR/release/vowp
H=/tmp/opencode/v2-m1-own-vps/rehearsal/v01/home
"$V" --home "$H" --json migrate --dry-run > migrate-dry-run.json
"$V" --home "$H" --json migrate --yes > migrate.json
```

Both exited 0. The dry run reported `from_version 1`, `to_version 2`, 32
global/project table pairs with matching before/after counts, `verified: true`,
`dropped_workers: 0`, project `legacy`, and a backup path that did not exist
yet. The real migration reported the same, plus `projects` including `_node`
and `legacy`. After it:

```text
PRAGMA user_version                 = 2
global projects                     = _node, legacy
projects/_node/project.db           = integrity ok
projects/legacy/project.db          = integrity ok, jobs v01-job-1/v01-job-2 done,
                                      runs 3, gate_results 6
migration backup                    = home/backups/pre-0.2-1794422657075.db
```

Rollback was run on a copy, then the restored v0.1 binary read the history:

```sh
cp -a "$H" "$H-rollback"
"$V" --home "$H-rollback" --json migrate --rollback <pre-0.2-backup>
```

Exit 0. The result reported `from_version 2`, `to_version 1`,
`verified: true`, and the warning that the split database was moved to
`backups/failed-0.2-1794422668657.db`. `PRAGMA user_version` was 1 again and
the restored global `jobs` table had both jobs `done`. From the project
checkout the v0.1.0 binary reported:

```text
supervisor: stopped
legacy @ 2026-11-11T18:44:31.632Z
jobs: done 2
```

A second `vowp migrate --dry-run`/`--yes` on the rolled-back home exited 0 and
returned to `user_version 2` with both jobs still `done`, so the rollback is
recoverable and not a one-way trap.

### 2. Night-supervisor state backup and restore

The live state was snapshotted with the SQLite backup API plus a full archive
that excluded only `target/`, `__pycache__` and the historical `night.db.bak-*`
copies. The live database had 199 jobs (186 done, 11 cancelled, 2 running) and
1,956 runs. The archive was 101,988,051 bytes with 14,734 entries:

```text
night-state.tar.gz  sha256 2f5431d2b17dc98cbcddc8160d753f469337458c99d0eb67a5c7d0d251fcd7b1
night.db            sha256 6e9d3dc2e0e0caa715b46b6f4531590831a41f1867c739e87acfed66f5ef5ca9
```

`gzip -t` exited 0, extraction exited 0, and `NIGHT_BASE=<restored>
python3 <home>/vowp-night/night.py job ls` exited 0 and printed the same job
list. The restored database reported `integrity ok`, 199 jobs, 186 done and
1,956 runs — identical to the snapshot. The live `tar` printed "file changed as
we read it" once because `night.log` was being appended; the archive still
validated and restored. The clean archive in phase 2 is taken after the stop
and does not have that race. This is why the plan takes both the live snapshot
and the post-stop archive.

### 3. Fresh v0.2 home, two projects and backup/restore

A throwaway home was created with the v0.2 binary and the acceptance fixture:

```sh
V=$CARGO_TARGET_DIR/release/vowp
W=/tmp/opencode/v2-m1-own-vps/rehearsal/home-rehearsal
"$V" --home "$W/home" init --name alpha --no-framework
bash scripts/acceptance/fixtures/project.sh hello "$W/beta" --name beta
"$V" --home "$W/home" project add "$W/beta"
"$V" --home "$W/home" project ls --json
"$V" --home "$W/home" fleet status --refresh --json
"$V" --home "$W/home" doctor
```

`project ls` returned `alpha` and `beta`; `fleet status` returned schema
`vowp-snapshot/1` with both projects; `doctor` exited 0 with the home, DB,
schema, projects, git, tmux, harness and disk checks. The generated project
config from `init` had `push = true`, `max_workers = 4`, `max_verifiers = 2`
and a default planner ladder beginning with `claude-code`; that is the reason
the plan edits the profiles before the switch.

A node backup and restore also passed:

```sh
"$V" --home "$W/home" --project alpha --json backup --to "$W/backups"
"$V" --home "$W/home" --project alpha --json backup \
  --restore "$W/backups" --to-home "$W/home-restored" --force
"$V" --home "$W/home-restored" project ls --json
"$V" --home "$W/home-restored" doctor
```

The backup wrote `global.db`, `projects/_node.db`, `projects/alpha.db`,
`projects/beta.db` and `catalog.json` (770,208 bytes) and explicitly skipped
`secrets/`. The restore exited 0, the restored home listed both projects, and
its `doctor` exited 0. `vowp backup` required `--project alpha` even for a
node-wide backup; without it the command returned `invalid_input: no project
selected`. That is recorded as a product follow-up below, not worked around in
the live plan.

### 4. Install and repository initialisation

The install path was exercised with a throwaway `HOME` so the real
`~/.local/bin` and user units were untouched:

```sh
HOME=/tmp/opencode/v2-m1-own-vps/rehearsal/fake-home \
  bash scripts/install.sh --bin "$V" --no-service --no-cron --no-watchdog
```

Exit 0; it installed `vowp` and the disabled `vowp.service`, and the installed
binary printed `vowp 0.2.0`. A local clone of the production checkout was then
initialised exactly as the plan says:

```sh
git clone -q --no-hardlinks <home>/vowp repo-rehearsal
cd repo-rehearsal
"$V" --home ../home-for-repo init --name vowp --no-framework
git add .gitignore vowp.toml
git commit -m "rehearsal: initialise the VOWP project"
```

`init` exited 0 and wrote 2 files (`vowp.toml` and one `.gitignore` line,
`.vowp/tmp/`). The generated config was the same one described above. After the
commit the tree was clean and `project ls` listed `vowp`. This proves the
production checkout's only needed source change is the generated config, and
that it can be committed before the home starts.

### 5. Owner sessions and NorthNoir were not touched

The default tmux server and the night supervisor were observed with `ps` and
`systemctl --user show` only; no tmux command was run against the default
server and no process was signalled. In a five-second before/after inventory
the default server PID 495552 (started 2026-11-09 07:49:27) and the night
MainPID 786117 (started 09:16:54) were unchanged. The only process difference
was a transient process belonging to another night job, not to the owner's
sessions or NorthNoir. The rehearsal used its own `TMUX_TMPDIR` and home-local
tmux sockets.

## PENDING OWNER APPROVAL

These are the only steps this record cannot perform. They are **not run** and
must not be treated as passed.

1. **Publish the release.** The orchestrator/owner must create and publish the
   `v0.2.0` GitHub release with the four archives and `SHA256SUMS`, then verify
   `gh release view v0.2.0 --repo syver777/vowp` and `scripts/install.sh
   --download --version 0.2.0` on the VPS. The current record is based on a
   locally built 0.2.0 binary, not the release artifact.
2. **Approve this plan.** The owner reads the whole plan, confirms the account
   choice, the `vowp` project name, the executor/profile edits, the `push`
   setting, the stale-home move, and the orchestrator name/app. One yes covers
   phases 1–7.
3. **Run the switch after the last night job.** The owner/orchestrator runs
   phase 0 only when the active-job query is empty. Do not stop the supervisor
   while a job is running; this record was written while two jobs were still
   running.
4. **Join the new orchestrator.** The owner joins the opencode desktop app on
   the Mac through the printed tunnel/pair link and the phone over SSH, sends
   one turn from each, and confirms both see the session. The exact commands
   are in [dogfood.md](dogfood.md#pending-owner-acceptance). The live move and
   the device joins are **Not run**.
5. **Confirm the first real job.** The new orchestrator reads the fleet, adds
   the next release job through `vowp mcp`/the normal job path, and the home
   supervisor merges it. The outgoing orchestrator stops acting only after
   this. If the owner wants to retry with a different executor or app, rerun
   `orchestrator new --force` and repeat the join; no VOWP home reset is
   needed.

## Open items and follow-ups

1. **`setup home` has no "use the existing account" option.** With
   passwordless sudo it creates a dedicated `vowp` user; the M1 plan uses the
   documented manual install instead. A future `setup home --account` or
   `--no-user` mode would let the existing-VPS path use the AI-first setup
   without moving the owner's checkout and night state.
2. **Node-wide `vowp backup` requires a project.** The release notes describe a
   backup of the global database plus every project; the tested build returns
   `invalid_input: no project selected` unless `--project` is passed. The
   rehearsal used `--project alpha`; the product command should accept a
   node-wide backup from the home.
3. **The stale `<home>/.vowp` is v0.2 test data.** It contains only the
   setup-verify fixtures and was not produced by a production run. The plan
   preserves it rather than deleting it, but the setup-verify tests should
   leave no home behind; this is an acceptance hygiene item.
4. **The release gate is still open.** No `v0.2.0` tag or assets exist at
   `c843860`. This is not an M1 code change; it is the J2 publication step and
   the owner's crates.io/npm publication, both outside this card's owned path.
5. **The default-generated profile includes Claude.** `vowp init` puts
   `claude-code` first in the planner ladder. The plan edits it to the owner's
   opencode/DeepSeek policy; a future default that follows the node's actual
   harness policy would remove this manual step.
6. **The live switch and owner-device joins remain pending.** They are recorded
   above with exact commands, expected results and "Not run"; they are not
   claims of this acceptance record.

## Gates and reproduction

The branch changes only this document. The repository commands were run from
the worktree with a private `TMUX_TMPDIR` and `TMUX`/`TMUX_PANE` unset:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash docs/tests/run.sh
git diff --check
```

Results on this branch: `cargo fmt --all` exit 0; clippy exit 0;
`cargo test --workspace` exit 0 with 130 test-result groups, 1,146 passed,
0 failed and 2 ignored; `bash docs/tests/run.sh` PASS (548 relative links in
145 files, 768 command references, 69 distinct commands, 0 still marked
planned); `git diff --check` exit 0. The full test output is in the supervisor
run log, and the job report records the commands. The rehearsal artifacts are
under `/tmp/opencode/v2-m1-own-vps/rehearsal/` and are not committed; they include
the v0.1 home and its manifest, the migration and rollback JSON, the night
state archive and its checksums, the restored state, the fresh home and its
backup/restore output, the install output, and the before/after inventory.
