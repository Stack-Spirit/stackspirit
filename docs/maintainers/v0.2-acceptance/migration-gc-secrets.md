# v0.2 acceptance — migration, GC and secrets (T4)

This is the acceptance record for §13.5 tests **6 (migration)**, **7 (`vowp gc`)** and
**8 (secret redaction)**. It is the T4 card `v2-t4-migration-gc-secrets`.

**Result on the tested build: tests 6 and 7 pass; test 8 fails.** The failure is in
production redaction code, not in the rig. It is recorded in [Open items](#open-items)
with the exact files and commands needed to reproduce it.

## Environment

- Date: 2026-11-10.
- Worktree: `job/v2-t4-migration-gc-secrets`, based on `9c868d3` (origin/main at the
  time of the run).
- Binary under test: the workspace `vowp` built by
  `cargo build -p vowp-cli --bin vowp`, reported version `vowp 0.1.0` (the package
  version during the v0.2 pre-release), SHA-256
  `5bb6ff8b3c21bbc76bd3ae14f98388ee8f87d4a4d847179343cf496f25a6700e`.
- v0.1 generator binary: the v0.1.0 binary from the local release cache, checked by
  the generator to report exactly `vowp 0.1.0`; SHA-256
  `5fc76ec708641f9dca83045ca826ea589cee8438e83c2c15ff6159d8fa9dbc0b`. The
  generator was run with `--binary`, so its manifest records that SHA but does not
  compare it with the published release checksum.
- Rig: `bash scripts/acceptance/rig.sh up --name t4 --variant sudo --no-build`.
  The home is an Ubuntu 24.04 container (`vowp-acceptance-t4-home`) with sshd, git,
  tmux, sqlite3 and the fake harness fixtures.
- Every home under test is a temporary `VOWP_HOME` inside the container. The container
  has a private `TMUX_TMPDIR` (`/home/vowp/.acceptance/tmux`) and a private tmux socket
  (`vowp-acceptance-t4`). No host home, host tmux server, network harness or real
  model was used.
- The rig was removed with `bash scripts/acceptance/rig.sh down --name t4` after the
  evidence was collected.

The commands below use these shell variables inside the container:

```sh
V=/home/vowp/bin/vowp                 # the v0.2 build under test
V01=/home/vowp/bin/vowp-0.1.0         # v0.1.0 binary from the local release cache
E=/home/vowp/evidence                 # output and evidence, outside the repo
```

## Test 6 — v0.1 home migration, row counts, rollback and restore

### Fixture

The v0.1 home was generated with real v0.1 history (three completed jobs):

```sh
bash scripts/acceptance/rig.sh v01-home --name t4 \
    --dest /home/vowp/acceptance/v01 \
    --binary /home/vowp/bin/vowp-0.1.0 --jobs 3
```

The generator's manifest reported a v0.1 home at `/home/vowp/acceptance/v01/home`,
schema version 1, project `legacy`, three jobs all `done`, four sessions and four runs.
Its row counts are the source of truth for the checks below.

### Dry run and migration

```sh
H=/home/vowp/acceptance/v01/home
$V --home "$H" --json migrate --dry-run >"$E/migrate-dry-run.json"
$V --home "$H" --json migrate          >"$E/migrate.json"
```

Both exited 0. The dry run reported `from 1 to 2`, one project (`legacy`), 32 table
counts and a backup path that did **not** exist yet; it wrote nothing. The real
migration reported `verified: true`, `dropped_workers: 0`, the same project list and a
backup path that did exist. After it:

```text
PRAGMA user_version                 = 2
projects                            = _node, legacy
projects/_node/project.db           = present
projects/legacy/project.db          = present
migration.json                      = absent
```

The migration backup was
`/home/vowp/acceptance/v01/home/backups/pre-0.2-1794349506625.db`.

### Row-count comparison

The counts were checked independently against the v0.1 manifest, not only against the
migration report (script: `verify-counts.py`, run against both databases):

```text
global:kv                   11 ==   11 True
global:nodes                 1 ==    1 True
global:sessions              4 ==    4 True
global:session_events       28 ==   28 True
global:session_natives       0 ==    0 True
global:capsules              0 ==    0 True
global:switches              0 ==    0 True
global:limits                0 ==    0 True
global:projects              2 ==    2 True   (source + reserved _node)
global:node_inbox            0 ==    0 True
global:node_ledger          13 ==   13 True
project sum:jobs             3 ==    3 True
project sum:edges            0 ==    0 True
project sum:runs             4 ==    4 True
project sum:gate_results     9 ==    9 True
project sum:workers          3 ==    3 True
project sum:loops            9 ==    9 True
project sum:schedules        0 ==    0 True
project sum:inbox            0 ==    0 True
project sum:releases         0 ==    0 True
project sum:ledger          21 ==   21 True
integrity projects/_node/project.db: ok
foreign_key_check projects/_node/project.db: 0 row(s)
integrity projects/legacy/project.db: ok
foreign_key_check projects/legacy/project.db: 0 row(s)
cross-file refs in project DBs: []
RESULT: PASS
```

`global:projects` is source count + 1 because the migration adds the reserved `_node`
registry row. The split destinations keep no foreign key to the global `projects` or
`sessions` tables.

### Backup and restore drill

```sh
cd /home/vowp/acceptance/v01/project
$V --home "$H" --json backup >"$E/backup.json"
$V --home "$H" --json backup --restore <backup-dir> \
    --to-home /home/vowp/acceptance/restored >"$E/restore.json"
```

- The backup directory had 8 files and 635,487 bytes. Its `manifest.json` had
  `vowp-backup/1`, global schema 2, project schema 1 and both projects (`_node`,
  `legacy`). It explicitly skipped `secrets/` (`never backed up`).
- Restore exited 0 and created a fresh home with schema 2 and both project databases.
  A second independent count comparison (global tables, every project table, and the
  four session logs) matched the live migrated home exactly.
- `vowp --home /home/vowp/acceptance/restored status --check` showed the three jobs
  `done`, no running work and `supervisor: not running`.

### Rollback

```sh
BACKUP=/home/vowp/acceptance/v01/home/backups/pre-0.2-1794349506625.db
$V --home "$H" --json migrate --rollback "$BACKUP" >"$E/rollback.json"
```

The command exited 0 and reported `from 2 to 1`, `verified: true`. It moved the split
database aside to `backups/failed-0.2-...db`, restored the v0.1 file, removed
`projects/` and removed `migration.json`. Every table count matched the original
manifest again (19/19 checks true), `PRAGMA user_version` was 1,
`PRAGMA integrity_check` was `ok`, and the v0.1.0 binary still read the home:

```text
v01-job-1=done,v01-job-2=done,v01-job-3=done
RESULT: PASS
```

**Test 6: PASS.**

## Test 7 — `vowp gc` dry run and guarded `--apply`

### Fixture

A v0.2 home was created with `vowp init` and one fixture project (`t8c`). The project ran
one fake-harness job to `done`, which produced a closed session, run output, gate
results and host samples. The harness was a fake CLI; it never called a model or the
network. The project config named a production environment checkout at
`/home/vowp/projects/prod-t8c`. The test then added one file in each protected class:

```text
$H/gates/protected-evidence.txt          truth
$H/archive/protected-archive.txt         truth
$H/orchestrators/protected-orchestrator.txt  open_work
$H/tmp/acceptance-cache.txt              cache
$H/secrets/ACCEPTANCE_SECRET             secrets
/home/vowp/projects/prod-t8c/*           production
```

The global and project databases, the production checkout and the secret file are real
state created by the product commands.

### Dry run

```sh
cd /home/vowp/projects/t8c
$V --home "$H" gc
$V --home "$H" --json gc >"$E/t7-gc-dry-run.json"
```

Both exited 0 and changed nothing. The dry run listed these classes (the human output
and the JSON agree):

```text
CLASS           FILES  BYTES
production      1      49
truth           12     876588
secrets         1      32
open_work       5      6696
closed_session  2      20789
run_output      4      308
cache           1      17
```

The `sample` class was not listed at the current clock because the five host samples
were newer than the 24-hour retention. It appears in the future-clock apply below.
The dry run ended with `dry run: nothing deleted; pass --apply to collect eligible files`.

### Apply at a test clock

```sh
$V --home "$H" --json gc --apply --now 2027-01-01T00:00:00Z >"$E/t7-gc-apply.json"
```

It exited 0 with:

```text
dry_run           false
deleted_files     12
freed_bytes       21114
compacted_sessions 2
errors            []
```

The report included `sample 5` entries. After `--apply`:

- both closed sessions were compacted: their `log.jsonl` was removed and each has a
  `capsules/*.json` plus `summary.md`; the session and capsule index rows remain;
- the four run-output files and the `tmp/acceptance-cache.txt` file were gone;
- all five `host_samples` rows were deleted by the sample prune;
- the protected files were still present and their content hashes were unchanged,
  except `vowp.db` as explained next.

The protected paths and their before/after state:

| Path | Before | After |
|---|---|---|
| `$H/vowp.db` | `caded777...c735a5` | present (bytes changed) |
| `$H/projects/t8c/project.db` | `eae8c5e9...b25eb` | identical |
| `$H/gates/protected-evidence.txt` | `44a608b2...f246e` | identical |
| `$H/archive/protected-archive.txt` | `bbc1e4c7...7565f` | identical |
| `$H/orchestrators/protected-orchestrator.txt` | `d30a8985...fed0` | identical |
| `$H/secrets/ACCEPTANCE_SECRET` | `bab8c515...31d9` | identical |
| `/home/vowp/projects/prod-t8c/serving.txt` | `8e9a44c2...dcaf` | identical |
| `/home/vowp/projects/prod-t8c/release.txt` | `3b1ada4c...d3450` | identical |

`vowp.db` is protected truth and is never deleted. Its bytes change because GC records
catalog deletions and prunes `host_samples` rows, which is the explicitly allowed
`sample` work; the database path remains. The property that matters for the acceptance
test is that no protected path disappears.

**Test 7: PASS.**

## Test 8 — a secret injected into a harness

### Setup

The fixture project's harness was replaced by a wrapper that:

1. writes `name=ACCEPTANCE_SECRET len=… sha256=…` to an evidence file, proving the
   exact value was injected;
2. prints the value to stdout, so it would reach the session log and run mirror if
   redaction failed;
3. writes the value into the job's `report.md` after the fake worker finishes.

The project's `[harnesses.acceptance-hello]` had `env_pass = ["ACCEPTANCE_SECRET"]`,
and the adapter mapping `[command]` had `env = ["ACCEPTANCE_SECRET"]`. Both are needed:
the vault resolves the name, and the generic CLI adapter only passes names listed in the
mapping.

The secret was stored through the real one-time paste page, not through argv or a
piped hidden prompt:

```sh
$V --home "$H" --project t8c --json secret request ACCEPTANCE_SECRET
# prints http://127.0.0.1:<port>/s/<token>, expires in 10 minutes, single use
curl -fsS -X POST -H 'Content-Type: text/plain' --data-binary @- "$URL"
```

The POST used the value on stdin, never in `argv`. The returned page said `saved` and
did not contain the value. `vowp secret ls --json` showed only the name and last four
characters. The harness's evidence file then reported:

```text
name=ACCEPTANCE_SECRET len=32 sha256=bab8c515e02bbf8daf0c0b1f22cff8461174c4284f57d6cc892badbb6e0231d9
```

which matches the SHA-256 of the stored value. This proves injection rather than a
silent missing variable.

A first job ran to `done`. A second, controlled job was stopped at `gating` so its
worktree and `report.md` could be inspected before the merge queue removed them.

### What passed

- **Session log.** Before compaction the worker session log contained two
  `[REDACTED:ACCEPTANCE_SECRET]` markers and no literal value.
- **Report.** The `gating` job's `report.md` contained one
  `[REDACTED:ACCEPTANCE_SECRET]` marker and no literal value.
- **Events / index.** `session_events.summary`, ledger `detail_json` and
  `gate_results.evidence_json`/`gate_json` contained no literal value.
- **Snapshot.** `snapshot.json` and `board.json` contained no literal value.
- **Capsule and export.** The compacted session's capsule file, `summary.md`,
  `session show --capsule`, and `session export` bundle contained no literal value.
  (The capsule was built from the already-redacted log, so it contains no marker; the
  acceptance requirement is that it contains no secret.)

### What failed

Two production paths still expose the value:

1. **Jobless (planner/keeper) run raw mirrors are not redacted.** After a planner turn,
   `$H/runs/<planner-run-id>.jsonl` contained the literal value in its
   `leak-check stdout:` and `leak-check final:` lines. The worker run mirror was
   correctly redacted, but `vault::redact_job_artifacts` returns early when
   `job: Option<&JobId>` is `None`, and planner/keeper turns pass `req.job_id == None`.
   The leaked file was later removed by the test's `gc --apply` because run output is a
   disposable class; the leak was observed and recorded before that.

2. **`runs.detail` stores the unredacted last assistant text.** The project database
   held the literal value in `runs.detail` for both the worker and the planner rows.
   This is the `RunFinish.detail` copied from `summary.result_text` in
   `SessionService::run_turn`; the truncating classifier redacts token *shapes*, but it
   is not the vault exact-value redactor. After the GC pass this was the only
   remaining copy in the home, and the final whole-home scan still found it:

   ```text
   == projects/t8c/project.db
   LEAK runs.detail id=01M73SPVD1SAMPZZVNHTTMG49K
   LEAK runs.detail id=01M73STH4EGNQM2AX3ND49B1ZA
   LEAK runs.detail id=01M73SYTFCQ4FYXQDKGTPVKNRD
   ```

   The three ids are the worker run, the planner run, and the second worker run. The
   final scan of all files under the home and project, excluding `secrets/`, reported:

   ```text
   LEAK:
   /home/vowp/acceptance/t8c/home/projects/t8c/project.db
   /home/vowp/acceptance/t8c/home/projects/t8c/project.db-wal
   ```

No literal secret is printed in this document; the evidence above uses the SHA-256 and
the failing paths/ids only.

**Test 8: FAIL.**

## Open items

- **Fix job needed: redact jobless run mirrors and `runs.detail`.** Suggested owner
  paths are `crates/vowp-core/src/vault/mod.rs`,
  `crates/vowp-core/src/session/service.rs` and
  `crates/vowp-core/src/session/remote.rs` (plus tests). The fix should:
  1. redact the run raw mirror by run id for every turn, including planner/keeper turns
     where there is no job id;
  2. pass `RunFinish.detail` through the vault exact-value redactor before
     `Store::finish_run`, so `runs.detail` cannot retain a secret;
  3. cover both with a test that injects a secret, runs a jobless planner turn and a
     worker turn, and greps the run mirror and `runs.detail` for the literal value.
- **Observation, not part of test 8:** after a job's worktree was removed by the merge
  queue, `vowp session capsule <done-session>` failed with
  `refused: working directory ... is outside the allow-list` because the session's
  cwd no longer exists. GC compaction still produced a valid capsule through the
  internal `job_end` path, so tests 7 and 8 were not blocked by this. A follow-up may
  want `session capsule` to fall back to the project checkout when the worktree is
  gone.

## How to reproduce the failing check

On a fresh rig with the fixture project and the secret configured as above:

```sh
# wait until a planner/keeper turn has run, then before gc:
for f in "$H"/runs/*.jsonl; do
  grep -aF -- "$SECRET" "$f" && echo "LEAK $f"
done
sqlite3 "$H/projects/t8c/project.db" \
  "SELECT id, job_id, role, detail FROM runs WHERE detail LIKE '%$SECRET%';"
```

The first command finds the planner raw mirror; the second finds `runs.detail`. The
worker raw mirror is redacted and should not appear in the first result.
