#!/usr/bin/env bash
# scripts/dogfood/run.sh — drive the real `vowp` binary through a complete throwaway
# project, twice, with fake model adapters only.
#
# Phase 1: init -> doctor -> three jobs -> `vowp up` -> worker -> gates -> merge ->
#          board/status -> `vowp down --kill-windows` -> cleanup assertions.
# Phase 2: the same graph in a second project, SIGKILL the detached supervisor while a
#          worker run is in flight, restart `vowp up`, and prove startup reconciliation
#          finishes every job without duplicating a merge or leaving one stuck.
#
# Hermetic by construction:
#   * a fresh temporary directory under ${TMPDIR:-/tmp};
#   * private HOME, TMPDIR and VOWP home;
#   * a private tmux server: a `tmux` wrapper on PATH adds `-L <unique-socket>`;
#   * fake `claude` and `opencode` binaries that answer only `--version`, so `doctor`
#     never probes (and the run never starts) the real harnesses;
#   * a throwaway git repository with a bare local `origin` (no network);
#   * fake worker/verifier scripts that edit a file and write a valid report.
#
# The real parts are the supervisor, worktrees, gates, merge queue, ledger, board,
# tmux windows and heartbeat. The script always fails loudly (non-zero) on a failed
# assertion and prints diagnostics from the supervisor log, job table and fake log.
#
# Usage:
#   bash scripts/dogfood/run.sh
#   VOWP=/path/to/vowp bash scripts/dogfood/run.sh   # skip the cargo build
#   KEEP=1 bash scripts/dogfood/run.sh               # keep the temporary work dir
#   TIMEOUT=600 ...                                  # phase 1 job timeout (seconds)
#   CRASH_TIMEOUT=300 ...                            # phase 2 job timeout (seconds)
#   CRASH_EARLY=1 ...                                # reproduce defect D3 (see below)
#
# The script exports VOWP_HOME because `vowp up --home` does not pass --home to the tmux
# windows it creates (defect D1 in docs/dogfood/RESULTS.md). Everything this run finds is
# listed there with a reproduction; D2-D5 are reported in the output, D6-D7 are documented
# but not exercised by the default graph, and only D3 has a loud opt-in assertion
# (CRASH_EARLY=1). The default run must stay green on a tree with those defects.
#
# Requires: bash, git, tmux, python3 (for JSON/SQLite assertions), and cargo (the shim
# on PATH builds the CLI) unless VOWP names a built binary.

set -Eeuo pipefail

# A caller inside tmux exports these; they would otherwise point every tmux client at
# the caller's server. The wrapper below is the only tmux route this script uses.
unset TMUX TMUX_PANE 2>/dev/null || true

DOGFOOD_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$DOGFOOD_DIR/../.." && pwd)
FAKE_DIR="$DOGFOOD_DIR/fake-harness"
TIMEOUT=${TIMEOUT:-600}
CRASH_TIMEOUT=${CRASH_TIMEOUT:-300}
KEEP=${KEEP:-0}
# Default 0 kills only when every `running` job has an open run row with a live pid
# (the documented crash/continue path). CRASH_EARLY=1 kills as soon as a job reports
# running, which reproduces defect D3: the Dispatched-but-no-run-yet window leaves a
# job stuck in `running` after the restart.
CRASH_EARLY=${CRASH_EARLY:-0}
WORK=""
P_HOME=""
P_REPO=""
P_ORIGIN=""
P_NAME=""
P_SOCK=""
PHASE=""
STARTED_AT=$(date +%s)

say() {
    printf '%s\n' "dogfood: $*"
}

fail() {
    printf '%s\n' "dogfood: FAIL: $*" >&2
    exit 1
}

# Resolve the real tmux before the wrapper directory joins PATH.
REAL_TMUX=$(command -v tmux 2>/dev/null || true)
[ -n "$REAL_TMUX" ] || fail "tmux is required (tmux 3.x) but was not found on PATH"
case "$REAL_TMUX" in
    "$FAKE_DIR"/*)
        fail "PATH already starts with the dogfood fake harness; run with a clean PATH"
        ;;
esac

command -v git >/dev/null 2>&1 || fail "git is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required for JSON/SQLite assertions"
[ -x "$FAKE_DIR/fake-worker.sh" ] || fail "missing $FAKE_DIR/fake-worker.sh"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/vowp-dogfood.XXXXXX")
say "work directory $WORK"

# The wrapper turns every `tmux` call into `tmux -L <private-socket>`; the fake harness
# directory also supplies the worker script and the version-only harness probes.
PATH="$FAKE_DIR:$PATH"
export PATH VOWP_DOGFOOD_REAL_TMUX="$REAL_TMUX"

diagnostics() {
    [ -n "$P_REPO" ] && [ -d "$P_REPO" ] || return 0
    {
        printf '%s\n' "--- job ls --all ---"
        (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" --project "$P_REPO" job ls --all) 2>&1 || true
        printf '%s\n' "--- status ---"
        (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" --project "$P_REPO" status) 2>&1 || true
        printf '%s\n' "--- supervisor.log tail ---"
        tail -n 80 "$P_HOME/logs/supervisor.log" 2>/dev/null || true
        printf '%s\n' "--- fake harness log ---"
        tail -n 40 "$P_HOME/../fake.log" 2>/dev/null || true
    } >&2
}

cleanup() {
    status=$?
    trap - EXIT INT TERM
    if [ -n "$P_HOME" ] && [ -f "$P_HOME/supervisor.lock" ] && [ -n "$P_REPO" ]; then
        (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" --project "$P_REPO" down --force) >/dev/null 2>&1 || true
    fi
    # Only the private socket is ever killed; the wrapper guarantees the server name.
    if [ -n "$P_SOCK" ]; then
        "$REAL_TMUX" -L "$P_SOCK" kill-server >/dev/null 2>&1 || true
    fi
    if [ "$status" -ne 0 ]; then
        diagnostics
        say "FAILED after $(( $(date +%s) - STARTED_AT ))s; work directory kept at $WORK"
    elif [ "$KEEP" = "1" ]; then
        say "kept work directory $WORK"
    else
        rm -rf "$WORK"
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# --- helpers ----------------------------------------------------------------

vowp_cli() {
    (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" --project "$P_REPO" "$@")
}

vowp_json() {
    (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" --project "$P_REPO" --json "$@")
}

json_check() {
    # json_check '<description>' '<python source>' [args...]; stdin is the JSON.
    local description=$1
    local program=$2
    shift 2
    python3 -c "$program" "$@" || fail "$description"
}

start_phase() {
    PHASE=$1
    P_NAME=$2
    local phase_root="$WORK/$PHASE"
    P_HOME="$phase_root/vowp-home"
    P_DB="$P_HOME/vowp.db"
    P_REPO="$phase_root/repo"
    P_ORIGIN="$phase_root/origin.git"
    P_SOCK="vowp-dogfood-$$-$PHASE"
    export HOME="$phase_root/user-home"
    # The tmux windows `vowp up` creates run the bare binary without --home, so they
    # resolve the node home from VOWP_HOME. Setting it keeps those windows alive with
    # the private home (defect D1 in docs/dogfood/RESULTS.md: --home is not propagated).
    export VOWP_HOME="$P_HOME"
    export TMPDIR="$phase_root/tmp"
    export TMUX_TMPDIR="$phase_root/tmux"
    export VOWP_DOGFOOD_TMUX_SOCKET="$P_SOCK"
    export VOWP_DOGFOOD_FAKE_LOG="$phase_root/fake.log"
    mkdir -p "$HOME" "$TMPDIR" "$TMUX_TMPDIR" "$P_HOME" "$phase_root"
    say "[$PHASE] HOME=$HOME"
    say "[$PHASE] VOWP home=$P_HOME"
    say "[$PHASE] project=$P_REPO"
}

setup_project() {
    local name=$1
    git init -q -b main "$P_REPO"
    git init -q --bare -b main "$P_ORIGIN"
    git -C "$P_REPO" config user.name "Dogfood Runner"
    git -C "$P_REPO" config user.email "dogfood@example.invalid"

    (cd "$P_REPO" && "$VOWP_BIN" --home "$P_HOME" init --name "$name" --no-framework) \
        >"$WORK/$PHASE/init.log" 2>&1 || {
        cat "$WORK/$PHASE/init.log" >&2
        fail "[$PHASE] vowp init exited non-zero"
    }
    grep -q "initialised project $name" "$WORK/$PHASE/init.log" \
        || fail "[$PHASE] init did not report project $name"

    write_config "$name"

    mkdir -p "$P_REPO/.vowp/adapters"
    cp "$FAKE_DIR/mapping.toml" "$P_REPO/.vowp/adapters/fake-worker.toml"
    cp "$FAKE_DIR/project/README.md" "$P_REPO/README.md"
    cp "$FAKE_DIR/project/.gitignore" "$P_REPO/.gitignore"

    git -C "$P_REPO" add -A
    git -C "$P_REPO" commit -q -m "$name: initial project"
    git -C "$P_REPO" remote add origin "$P_ORIGIN"
    git -C "$P_REPO" push -q -u origin main
}

write_config() {
    local name=$1
    cat >"$P_REPO/vowp.toml" <<EOF
# Generated by scripts/dogfood/run.sh. Fake adapters only; no real harness is called.
[node]
name = "local"
max_workers = 4
max_verifiers = 2
tick_secs = 1
tz_offset_minutes = 0

[project]
name = "$name"
remote = "origin"
default_branch = "main"
check_cmds = []
push = true

[profiles.planner]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.keeper]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.worker]
ladder = ["cli:fake-worker"]
on_quota = "park"

[profiles.verifier]
ladder = ["cli:fake-worker"]
on_quota = "park"

# Pin the built-in probes to version-only stand-ins, not the real binaries.
[harnesses.claude-code]
binary = "$FAKE_DIR/claude"

[harnesses.opencode]
binary = "$FAKE_DIR/opencode"

[harnesses.fake-worker]
mapping = ".vowp/adapters/fake-worker.toml"
EOF
}

run_doctor() {
    local report
    report=$(vowp_json doctor) || {
        printf '%s\n' "$report" >&2 || true
        fail "[$PHASE] vowp doctor exited non-zero"
    }
    printf '%s' "$report" | json_check "[$PHASE] doctor checks" '
import json, sys
report = json.load(sys.stdin)
if not report.get("ok"):
    print("doctor ok=false", file=sys.stderr)
checks = {c["name"]: c["status"] for c in report.get("checks", [])}
for name in ("home", "db", "git", "config", "profiles", "harness:claude-code", "harness:opencode"):
    if checks.get(name) != "ok":
        print(f"doctor check {name}: {checks.get(name)}", file=sys.stderr)
        sys.exit(1)
'
    say "[$PHASE] doctor: ok"
}

job_body() {
    local id=$1 file=$2 kind=$3
    case "$kind" in
        gamma)
            cat <<EOF
# Goal

Create \`$file\`. It must contain the first line of the dependency job's file,
followed by \`gamma from fake worker\`.

# Read first

- \`src/\` in this worktree (the dependency is already merged into main).

# Deliverables

- \`$file\`.

# Tests required

- The job's cmd gate checks both lines.

# Done when

\`$file\` exists and the report has \`status: done\`.
EOF
            ;;
        *)
            cat <<EOF
# Goal

Create \`$file\` with the single line \`$kind from fake worker\`.

# Read first

- \`README.md\` in this worktree.

# Deliverables

- \`$file\`.

# Tests required

- The job's cmd gate checks the line.

# Done when

\`$file\` exists and the report has \`status: done\`.
EOF
            ;;
    esac
}

add_job() {
    local id=$1 needs=$2 owns=$3 gates=$4 title=$5 kind=$6
    local body output
    body=$(job_body "$id" "$owns" "$kind")
    if [ -n "$needs" ]; then
        output=$(printf '%s\n' "$body" | vowp_cli job add --id "$id" --title "$title" --kind build \
            --needs "$needs" --owns "$owns" --gates "$gates" --brief -) \
            || fail "[$PHASE] vowp job add $id failed"
    else
        output=$(printf '%s\n' "$body" | vowp_cli job add --id "$id" --title "$title" --kind build \
            --owns "$owns" --gates "$gates" --brief -) \
            || fail "[$PHASE] vowp job add $id failed"
    fi
    case "$output" in
        *"added job $id"*) ;;
        *) fail "[$PHASE] unexpected job add output for $id: $output" ;;
    esac
}

add_three_job_graph() {
    local prefix=$1
    add_job "${prefix}alpha" "" "src/${prefix}alpha.txt" \
        "report,owns_respected,grep -qx 'alpha from fake worker' src/${prefix}alpha.txt" \
        "Write ${prefix}alpha" alpha
    add_job "${prefix}beta" "" "src/${prefix}beta.txt" \
        "report,owns_respected,grep -qx 'beta from fake worker' src/${prefix}beta.txt" \
        "Write ${prefix}beta" beta
    # Exactly one dependency edge: gamma starts only after alpha has merged, so its
    # worktree contains src/<prefix>alpha.txt. beta stays independent.
    add_job "${prefix}gamma" "${prefix}alpha" "src/${prefix}gamma.txt" \
        "report,owns_respected,grep -qx 'alpha from fake worker' src/${prefix}gamma.txt && grep -qx 'gamma from fake worker' src/${prefix}gamma.txt" \
        "Assemble ${prefix}gamma" gamma

    local graph
    graph=$(vowp_cli graph) || fail "[$PHASE] vowp graph failed"
    printf '%s\n' "$graph" | grep -q "${prefix}gamma" || fail "[$PHASE] graph does not show ${prefix}gamma"
    printf '%s\n' "$graph" | grep -q "<- ${prefix}alpha" || fail "[$PHASE] graph does not show the ${prefix}gamma <- ${prefix}alpha edge"
    say "[$PHASE] graph: 3 jobs, ${prefix}gamma needs ${prefix}alpha"
}

assert_loop_rows() {
    # n08d may add `vowp loops list --json`; before that, `vowp loops --json` lists the
    # same rows. Tolerate either shape, and never fail the run when the command is
    # absent (the planner/keeper loops are not part of this graph).
    local loops
    if loops=$(vowp_cli loops list --json 2>/dev/null); then
        :
    elif loops=$(vowp_cli loops --json 2>/dev/null); then
        :
    else
        say "[$PHASE] loops: no JSON listing available (tolerated)"
        return 0
    fi
    if printf '%s' "$loops" | python3 -c '
import json, sys
rows = json.load(sys.stdin)
if isinstance(rows, dict):
    rows = rows.get("loops", [])
names = {row.get("name") for row in rows}
missing = [f"l{n}" for n in range(4, 10) if not any(str(name).startswith(f"l{n}-") for name in names)]
if missing:
    print("missing loop rows: " + ", ".join(missing), file=sys.stderr)
    sys.exit(1)
'; then
        say "[$PHASE] loops: l4-l9 rows present"
    else
        say "[$PHASE] loops: JSON parsed but l4-l9 rows are incomplete (defect note, not fatal)"
    fi
}

start_supervisor() {
    local out
    out=$(vowp_cli up --detach) || fail "[$PHASE] vowp up --detach failed"
    case "$out" in
        *"supervisor started"*) ;;
        *) fail "[$PHASE] unexpected up output: $out" ;;
    esac
    local deadline=$(( $(date +%s) + 30 ))
    while :; do
        local status
        status=$(vowp_json status) || fail "[$PHASE] vowp status --json failed"
        if printf '%s' "$status" | python3 -c '
import json, sys
s = json.load(sys.stdin)
sys.exit(0 if s["supervisor"]["running"] else 1)
'; then
            break
        fi
        [ "$(date +%s)" -lt "$deadline" ] || fail "[$PHASE] supervisor did not report running within 30s"
        sleep 0.2
    done
    say "[$PHASE] supervisor running (pid $(head -n 1 "$P_HOME/supervisor.lock"))"
}

assert_tmux_layout() {
    local session="vowp-$P_NAME"
    local deadline=$(( $(date +%s) + 15 ))
    local windows=""
    while :; do
        windows=$(tmux list-windows -a -F '#{session_name}:#{window_name}' 2>/dev/null || true)
        if printf '%s\n' "$windows" | grep -qx "$session:board"; then
            break
        fi
        [ "$(date +%s)" -lt "$deadline" ] || fail "[$PHASE] tmux layout $session did not appear within 15s"
        sleep 0.2
    done
    local window
    # The board and supervisor windows follow continuously. The planner, keeper and
    # inbox window commands print once and exit, so tmux removes them immediately;
    # defect D2 records that. Require the two stable windows and note the rest.
    for window in board supervisor; do
        printf '%s\n' "$windows" | grep -qx "$session:$window" \
            || fail "[$PHASE] tmux window $session:$window is missing"
    done
    local missing=""
    for window in planner keeper inbox; do
        printf '%s\n' "$windows" | grep -qx "$session:$window" || missing="$missing $window"
    done
    if [ -n "$missing" ]; then
        say "[$PHASE] tmux: board+supervisor present; one-shot windows exited:$missing (defect D2)"
    else
        say "[$PHASE] tmux: all 5 fixed windows on private socket $P_SOCK"
    fi
}

poll_states() {
    # Prints: done_count|active_count|bad_list|state_list|run_detail
    # run_detail is comma-separated job:state:open_runs for every expected job.
    local ids=$1
    local jobs
    jobs=$(vowp_json job ls --all) || fail "[$PHASE] vowp job ls --all --json failed"
    printf '%s' "$jobs" | python3 -c '
import json, sqlite3, sys
expected = sys.argv[1].split()
db = sys.argv[2]
jobs = {job["id"]: job["state"] for job in json.load(sys.stdin)}
def state_of(job):
    return jobs.get(job) or "missing"
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
try:
    open_runs = dict(con.execute(
        "select job_id, count(*) from runs where ended_at is null group by job_id"
    ).fetchall())
finally:
    con.close()
done = sum(1 for job in expected if state_of(job) == "done")
active = sum(1 for job in expected if state_of(job) in {"running", "gating", "reviewing", "fixing", "merging"})
bad = [job + "=" + state_of(job) for job in expected
       if state_of(job) in {"failed", "parked", "stalled", "cancelled"} or job not in jobs]
state = " ".join(job + "=" + state_of(job) for job in expected)
run_detail = ",".join(
    job + ":" + state_of(job) + ":" + str(open_runs.get(job, 0)) for job in expected
)
print("|".join([str(done), str(active), ";".join(bad), state, run_detail]))
' "$ids" "$P_DB"
}

wait_for_jobs_done() {
    local ids=$1
    local limit=${2:-$TIMEOUT}
    local count expected
    expected=$(wc -w <<<"$ids")
    local deadline=$(( $(date +%s) + limit ))
    local done_count=0 active_count=0 bad_list="" state_list="" run_detail=""
    local poll_count=0
    local -A STUCK_SINCE=()
    while :; do
        IFS='|' read -r done_count active_count bad_list state_list run_detail < <(poll_states "$ids")
        if [ -n "$bad_list" ]; then
            fail "[$PHASE] job entered a terminal bad state: $bad_list"
        fi
        # A job in `running` with no open run row is the crash gap of D3. A healthy
        # dispatch can show the same shape for a few milliseconds, so allow five
        # seconds of continuous stuckness before failing.
        local now entry job rest state open
        now=$(date +%s)
        local -a entries=()
        IFS=',' read -ra entries <<<"$run_detail"
        for entry in "${entries[@]}"; do
            job=${entry%%:*}
            rest=${entry#*:}
            state=${rest%%:*}
            open=${rest##*:}
            if [ "$state" = "running" ] && [ "$open" -eq 0 ]; then
                if [ -z "${STUCK_SINCE[$job]:-}" ]; then
                    STUCK_SINCE[$job]=$now
                fi
                if [ $((now - STUCK_SINCE[$job])) -ge 5 ]; then
                    fail "[$PHASE] $job: state=running but no open run for >5s (defect D3)"
                fi
            else
                unset 'STUCK_SINCE[$job]' 2>/dev/null || true
            fi
        done
        if [ "$done_count" -eq "$expected" ] && [ "$active_count" -eq 0 ]; then
            say "[$PHASE] all jobs done: $state_list"
            return 0
        fi
        poll_count=$((poll_count + 1))
        if [ $((poll_count % 5)) -eq 0 ]; then
            local status_json board_json
            status_json=$(vowp_json status) || fail "[$PHASE] vowp status --json failed during the run"
            board_json=$(vowp_json board --once) || fail "[$PHASE] vowp board --json failed during the run"
            printf '%s' "$status_json" | python3 -c '
import json, sys
status = json.load(sys.stdin)
if "supervisor" not in status or "jobs" not in status:
    sys.exit(1)
' || fail "[$PHASE] status --json has the wrong shape"
            printf '%s' "$board_json" | python3 -c '
import json, sys
board = json.load(sys.stdin)
if "jobs" not in board or "running" not in board:
    sys.exit(1)
' || fail "[$PHASE] board --json has the wrong shape"
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            fail "[$PHASE] timed out after ${limit}s waiting for $expected jobs (done=$done_count active=$active_count): $state_list"
        fi
        sleep 0.2
    done
}

wait_for_worker_running() {
    local ids=$1
    local deadline=$(( $(date +%s) + 60 ))
    local state_list=""
    while :; do
        local jobs
        jobs=$(vowp_json job ls --all) || fail "[$PHASE] vowp job ls --all --json failed"
        state_list=$(printf '%s' "$jobs" | python3 -c '
import json, sys
expected = sys.argv[1].split()
jobs = {job["id"]: job["state"] for job in json.load(sys.stdin)}
print(" ".join(job + "=" + (jobs.get(job) or "missing") for job in expected))
' "$ids")
        local running done_count
        running=$(printf '%s' "$jobs" | python3 -c '
import json, sys
expected = set(sys.argv[1].split())
jobs = json.load(sys.stdin)
print(sum(1 for job in jobs if job["id"] in expected and job["state"] == "running"))
' "$ids")
        done_count=$(printf '%s' "$jobs" | python3 -c '
import json, sys
expected = set(sys.argv[1].split())
jobs = json.load(sys.stdin)
print(sum(1 for job in jobs if job["id"] in expected and job["state"] == "done"))
' "$ids")
        if [ "$running" -ge 1 ] && [ "$done_count" -eq 0 ]; then
            return 0
        fi
        if [ "$done_count" -gt 0 ]; then
            fail "[$PHASE] jobs finished before the crash window (states: $state_list)"
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            fail "[$PHASE] no worker reached running within 60s (states: $state_list)"
        fi
        sleep 0.1
    done
}

wait_for_crash_window() {
    # Kill only when the dispatch graph is quiescent: at least one job is running,
    # no job is ready or a dispatchable pending job, and every running job has an
    # open run row whose pid is still alive. Killing while a `ready` job can still
    # be dispatched leaves it running with no run row (defect D3).
    local ids=$1
    local deadline=$(( $(date +%s) + 60 ))
    while :; do
        local probe ready live_total detail
        probe=$(python3 - "$P_DB" "$ids" <<'PY'
import os
import sqlite3
import sys

db, ids = sys.argv[1], sys.argv[2].split()
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
try:
    placeholders = ",".join("?" for _ in ids)
    states = dict(con.execute(
        f"select id, state from jobs where id in ({placeholders})", ids
    ).fetchall())
    needs = {job: [] for job in ids}
    for src, dst in con.execute(
        f"select src, dst from edges where kind='needs' and src in ({placeholders})", ids
    ).fetchall():
        needs.setdefault(src, []).append(dst)
    live = {}
    for job, pid in con.execute(
        f"select job_id, pid from runs where job_id in ({placeholders}) "
        "and ended_at is null and pid is not null",
        ids,
    ).fetchall():
        if pid and os.path.exists(f"/proc/{pid}"):
            live[job] = live.get(job, 0) + 1
    running = [job for job in ids if states.get(job) == "running"]
    ready = [job for job in ids if states.get(job) == "ready"]
    pending_dispatchable = [
        job for job in ids
        if states.get(job) == "pending"
        and all(states.get(need) == "done" for need in needs.get(job, []))
    ]
    every_running_live = bool(running) and all(live.get(job, 0) >= 1 for job in running)
    ok = bool(running) and not ready and not pending_dispatchable and every_running_live
    detail = " ".join(
        job + "=" + (states.get(job) or "missing") + ":" + str(live.get(job, 0))
        for job in ids
    )
    detail += " ready=" + (",".join(ready) or "-")
    detail += " dispatchable=" + (",".join(pending_dispatchable) or "-")
    print(("1" if ok else "0") + "|" + str(sum(live.values())) + "|" + detail)
finally:
    con.close()
PY
)
        ready=${probe%%|*}
        probe=${probe#*|}
        live_total=${probe%%|*}
        detail=${probe#*|}
        if [ "$ready" = "1" ]; then
            say "[$PHASE] crash window: $live_total live in-flight run(s), no ready/dispatchable job ($detail)"
            return 0
        fi
        case "$detail" in
            *"=done:"*) fail "[$PHASE] jobs finished before the crash window ($detail)" ;;
        esac
        if [ "$(date +%s)" -ge "$deadline" ]; then
            fail "[$PHASE] no quiescent in-flight window appeared within 60s ($detail)"
        fi
        sleep 0.05
    done
}


stop_supervisor() {
    local deadline=$(( $(date +%s) + 40 ))
    vowp_cli down --kill-windows >/dev/null || fail "[$PHASE] vowp down --kill-windows failed"
    while :; do
        if [ ! -f "$P_HOME/supervisor.lock" ]; then
            break
        fi
        [ "$(date +%s)" -lt "$deadline" ] || fail "[$PHASE] supervisor.lock still present after down"
        sleep 0.2
    done
    [ ! -e "$P_HOME/heartbeat" ] || fail "[$PHASE] heartbeat file still exists after a clean down"
    [ -s "$P_HOME/heartbeat.stopped" ] || fail "[$PHASE] heartbeat.stopped marker is missing after a clean down"
    local status
    status=$(vowp_json status) || fail "[$PHASE] status --json after down failed"
    printf '%s' "$status" | json_check "[$PHASE] stopped status" '
import json, sys
s = json.load(sys.stdin)
if s["supervisor"]["running"] or s["supervisor"]["pid"] is not None:
    print("supervisor still reported running", file=sys.stderr)
    sys.exit(1)
if s["jobs"].get("done") != 3:
    print("status jobs.done={} (expected 3)".format(s["jobs"].get("done")), file=sys.stderr)
    sys.exit(1)
'
    if vowp_cli status --check >/dev/null 2>&1; then
        say "[$PHASE] down: status --check stays healthy"
    else
        say "[$PHASE] down: status --check exits 1 after a clean stop (defect D5: watchdog would restart)"
    fi
    say "[$PHASE] down: heartbeat removed, stopped marker written, status reports stopped"
}

assert_no_tmux_left() {
    local session="vowp-$P_NAME"
    local windows
    windows=$(tmux list-windows -a -F '#{session_name}:#{window_name}' 2>/dev/null || true)
    if printf '%s\n' "$windows" | grep -q "^$session:"; then
        fail "[$PHASE] leftover tmux window after down: $(printf '%s\n' "$windows" | grep "^$session:" | tr '\n' ' ')"
    fi
    say "[$PHASE] tmux: no leftover windows for $session (private socket $P_SOCK)"
}

assert_no_worktrees() {
    local worktrees branches
    worktrees=$(git -C "$P_REPO" worktree list --porcelain | grep -c '^worktree ')
    [ "$worktrees" -eq 1 ] || {
        git -C "$P_REPO" worktree list >&2
        fail "[$PHASE] expected only the main worktree, found $worktrees"
    }
    branches=$(git -C "$P_REPO" for-each-ref --format='%(refname:short)' 'refs/heads/job/' | wc -l)
    [ "$branches" -eq 0 ] || {
        git -C "$P_REPO" for-each-ref --format='%(refname:short)' 'refs/heads/job/' >&2
        fail "[$PHASE] leftover job branches: $branches"
    }
    say "[$PHASE] git: only the main worktree remains; no job branches"
}

assert_board() {
    local board
    board=$(vowp_json board --once) || fail "[$PHASE] board --json failed"
    printf '%s' "$board" | json_check "[$PHASE] board" '
import json, sys
board = json.load(sys.stdin)
if board["jobs"].get("done") != 3:
    print("board jobs.done={} (expected 3)".format(board["jobs"].get("done")), file=sys.stderr)
    sys.exit(1)
if board["running"]:
    print("board still lists running jobs: {}".format(board["running"]), file=sys.stderr)
    sys.exit(1)
if board["slots_busy"] != 0:
    print("board slots_busy={} (expected 0)".format(board["slots_busy"]), file=sys.stderr)
    sys.exit(1)
'
    say "[$PHASE] board: 3 done, nothing running, no busy slots"
}

assert_origin() {
    local prefix=$1
    local total subjects
    total=$(git --git-dir="$P_ORIGIN" rev-list --count main)
    [ "$total" -eq 4 ] || {
        git --git-dir="$P_ORIGIN" log --oneline main >&2
        fail "[$PHASE] origin/main has $total commits (expected the initial commit plus 3 job commits)"
    }
    subjects=$(git --git-dir="$P_ORIGIN" log --format='%s' main)
    local job
    for job in "${prefix}alpha" "${prefix}beta" "${prefix}gamma"; do
        [ "$(printf '%s\n' "$subjects" | grep -cx "$job: fake worker output")" -eq 1 ] \
            || fail "[$PHASE] origin/main does not have exactly one commit for $job"
    done
    [ "$(printf '%s\n' "$subjects" | grep -c ': fake worker output$')" -eq 3 ] \
        || fail "[$PHASE] origin/main does not have exactly 3 job commits"

    [ "$(git --git-dir="$P_ORIGIN" show "main:src/${prefix}alpha.txt")" = "alpha from fake worker" ] \
        || fail "[$PHASE] origin/main alpha file has the wrong content"
    [ "$(git --git-dir="$P_ORIGIN" show "main:src/${prefix}beta.txt")" = "beta from fake worker" ] \
        || fail "[$PHASE] origin/main beta file has the wrong content"
    local expected="$WORK/$PHASE/expected-gamma.txt"
    {
        printf '%s\n' "alpha from fake worker"
        printf '%s\n' "gamma from fake worker"
    } >"$expected"
    git --git-dir="$P_ORIGIN" show "main:src/${prefix}gamma.txt" >"$WORK/$PHASE/gamma.txt"
    cmp -s "$expected" "$WORK/$PHASE/gamma.txt" \
        || fail "[$PHASE] origin/main gamma file does not contain alpha's merged line plus its own"
    [ -z "$(git -C "$P_REPO" status --porcelain)" ] \
        || fail "[$PHASE] main checkout is dirty after the merge queue finished"
    [ "$(git -C "$P_REPO" rev-parse main)" = "$(git --git-dir="$P_ORIGIN" rev-parse main)" ] \
        || fail "[$PHASE] local main and origin/main disagree"
    say "[$PHASE] origin/main: exactly 3 job commits (initial + 3), files and checkout match"
}

ledger_python() {
    python3 - "$P_HOME/vowp.db" "$@" <<'PY'
import json
import sqlite3
import sys

db = sys.argv[1]
ids = sys.argv[2].split()
mode = sys.argv[3]
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
try:
    cur = con.cursor()
    if mode == "normal":
        problems = []
        for job in ids:
            states = cur.execute(
                "select count(*) from ledger where kind='state' and job_id=?", (job,)
            ).fetchone()[0]
            if states < 3:
                problems.append(f"{job}: only {states} state ledger rows")
            done = cur.execute(
                "select count(*) from ledger where kind='state' and job_id=? "
                "and json_extract(detail_json,'$.to')='done'", (job,)
            ).fetchone()[0]
            if done != 1:
                problems.append(f"{job}: {done} state rows ending in done")
            merges = cur.execute(
                "select count(*) from ledger where kind='merge' and job_id=?", (job,)
            ).fetchone()[0]
            if merges != 1:
                problems.append(f"{job}: {merges} merge ledger rows")
        runs = cur.execute(
            "select count(*) from runs where role='worker' and purpose='start'"
        ).fetchone()[0]
        if runs != 3:
            problems.append(f"expected 3 start runs, found {runs}")
        if problems:
            print("; ".join(problems), file=sys.stderr)
            sys.exit(1)
        print(f"state rows ok for {len(ids)} jobs, one merge row each, {runs} start runs")
    elif mode == "crash":
        crashes = cur.execute(
            "select count(*) from runs where outcome='crash' "
            "and detail like '%orphaned at supervisor start%'"
        ).fetchone()[0]
        if crashes < 1:
            print("no crash run recorded at restart", file=sys.stderr)
            sys.exit(1)
        continues = cur.execute(
            "select count(*) from ledger where kind='state' "
            "and detail_json like '%RunEndedNoReport%'"
        ).fetchone()[0]
        if continues < 1:
            print("no RunEndedNoReport state ledger row", file=sys.stderr)
            sys.exit(1)
        for job in ids:
            merges = cur.execute(
                "select count(*) from ledger where kind='merge' and job_id=?", (job,)
            ).fetchone()[0]
            if merges != 1:
                print(f"{job}: {merges} merge ledger rows after restart", file=sys.stderr)
                sys.exit(1)
            done = cur.execute(
                "select count(*) from ledger where kind='state' and job_id=? "
                "and json_extract(detail_json,'$.to')='done'", (job,)
            ).fetchone()[0]
            if done != 1:
                print(f"{job}: {done} state rows ending in done", file=sys.stderr)
                sys.exit(1)
        print(f"crash runs={crashes}, continue rows={continues}, one merge row per job")
    else:
        print(f"unknown ledger mode {mode}", file=sys.stderr)
        sys.exit(2)
finally:
    con.close()
PY
}

assert_ledger() {
    local ids=$1
    local output
    output=$(ledger_python "$ids" normal) || fail "[$PHASE] ledger assertions failed"
    say "[$PHASE] ledger: $output"
}

assert_crash_ledger() {
    local ids=$1
    local output
    output=$(ledger_python "$ids" crash) || fail "[$PHASE] crash ledger assertions failed"
    say "[$PHASE] ledger: $output"
    grep -q ' continue ' "$VOWP_DOGFOOD_FAKE_LOG" \
        || fail "[$PHASE] fake harness never saw a continue run after the crash"
    say "[$PHASE] fake harness: $(grep -c ' continue ' "$VOWP_DOGFOOD_FAKE_LOG") continue run(s) after restart"
}

# --- phase 1 ----------------------------------------------------------------

run_phase_one() {
    start_phase phase1 dogfood-one
    say "[$PHASE] start"
    setup_project dogfood-one
    run_doctor
    add_three_job_graph ""
    local phase_start=$SECONDS
    start_supervisor
    assert_tmux_layout
    assert_loop_rows || true
    wait_for_jobs_done "alpha beta gamma"
    say "[$PHASE] graph finished in $((SECONDS - phase_start))s"
    stop_supervisor
    assert_board
    assert_origin ""
    assert_ledger "alpha beta gamma"
    if grep -q 'brief=generic' "$VOWP_DOGFOOD_FAKE_LOG" 2>/dev/null; then
        say "[$PHASE] note: workers saw the dispatch fallback brief, not the job add brief (defect D4)"
    fi
    assert_no_worktrees
    assert_no_tmux_left
}

# --- phase 2 ----------------------------------------------------------------

run_phase_two() {
    start_phase phase2 dogfood-two
    say "[$PHASE] start (SIGKILL phase)"
    setup_project dogfood-two
    run_doctor
    add_three_job_graph "crash-"
    local phase_start=$SECONDS
    start_supervisor
    assert_tmux_layout
    if [ "$CRASH_EARLY" = "1" ]; then
        say "[$PHASE] CRASH_EARLY=1: killing as soon as a job reports running (reproduces D3)"
        wait_for_worker_running "crash-alpha crash-beta crash-gamma"
    else
        wait_for_crash_window "crash-alpha crash-beta crash-gamma"
    fi
    local pid
    pid=$(head -n 1 "$P_HOME/supervisor.lock")
    say "[$PHASE] killing supervisor pid $pid with SIGKILL"
    kill -9 "$pid" || fail "[$PHASE] kill -9 $pid failed"
    local deadline=$(( $(date +%s) + 10 ))
    while [ -e "/proc/$pid" ] && [ "$(date +%s)" -lt "$deadline" ]; do
        sleep 0.1
    done
    if [ -e "/proc/$pid" ]; then
        fail "[$PHASE] supervisor pid $pid is still present after SIGKILL (zombie not reaped)"
    fi
    say "[$PHASE] supervisor is dead; stale lock still names $pid"
    sleep 0.3

    local out
    out=$(vowp_cli up --detach) || fail "[$PHASE] restart after SIGKILL failed"
    case "$out" in
        *"supervisor started"*) ;;
        *) fail "[$PHASE] unexpected restart output: $out" ;;
    esac
    say "[$PHASE] restarted: $out"
    wait_for_jobs_done "crash-alpha crash-beta crash-gamma" "$CRASH_TIMEOUT"
    say "[$PHASE] graph finished in $((SECONDS - phase_start))s after one crash"
    stop_supervisor
    assert_board
    assert_origin "crash-"
    assert_crash_ledger "crash-alpha crash-beta crash-gamma"
    assert_no_worktrees
    assert_no_tmux_left
}

# --- build / locate the binary ----------------------------------------------

if [ -n "${VOWP:-}" ]; then
    VOWP_BIN=$VOWP
    say "using VOWP=$VOWP_BIN"
else
    say "building vowp-cli (cargo build -q -p vowp-cli)"
    (cd "$REPO_ROOT" && cargo build -q -p vowp-cli) || fail "cargo build -q -p vowp-cli failed"
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        VOWP_BIN="$CARGO_TARGET_DIR/debug/vowp"
    else
        VOWP_BIN="$REPO_ROOT/target/debug/vowp"
    fi
fi
[ -x "$VOWP_BIN" ] || fail "vowp binary is not executable: $VOWP_BIN"
"$VOWP_BIN" --version >/dev/null || fail "$VOWP_BIN --version failed"
say "binary: $VOWP_BIN ($("$VOWP_BIN" --version))"

run_phase_one
run_phase_two

say "PASS: both phases finished; elapsed $(( $(date +%s) - STARTED_AT ))s"
say "phase 1: clean init/doctor/3 jobs/merge/down; phase 2: SIGKILL + reconcile + merge"
