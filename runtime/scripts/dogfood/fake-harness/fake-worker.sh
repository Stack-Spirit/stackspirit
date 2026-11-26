#!/bin/sh
# scripts/dogfood/fake-harness/fake-worker.sh — the fake worker/verifier harness.
#
# VOWP starts this script in the job worktree with the L2 (or L3) prompt on stdin.
# It never calls a model or the network. It edits exactly one file, commits it on the
# job branch, and writes a report that satisfies the `report` gate. The gate runner,
# git worktrees, merge queue, ledger, tmux windows and supervisor are the real ones;
# only the "model" is fake.
#
# Job shapes used by scripts/dogfood/run.sh:
#   alpha  -> src/alpha.txt  containing "alpha from fake worker"
#   beta   -> src/beta.txt   containing "beta from fake worker"
#   gamma  -> src/gamma.txt  containing src/alpha.txt then "gamma from fake worker"
# A `<prefix>-alpha` / `<prefix>-beta` / `<prefix>-gamma` id (for example
# `crash-alpha`) writes `src/<id>.txt` and reads `src/<prefix>alpha.txt`, so the
# crash phase can run the same graph in a second project.
#
# Prompt shapes come from crates/vowp-core/src/loops/l2_worker.rs and l3_verify.rs.
# Planner/keeper prompts have no job report and are a deliberate no-op.
#
# Environment:
#   VOWP_DOGFOOD_FAKE_LOG    append one line per invocation when set
#   VOWP_DOGFOOD_FAKE_SLEEP  seconds to sleep before doing work (default 0)
#
# Exit codes: 0 on success, non-zero when the job is unknown or its dependency file
# is missing. A non-zero exit is a real failed run for the supervisor to handle.

set -eu

# The supervisor starts harnesses with a small explicit environment, so the run
# script cannot rely on its own variables reaching this process. Prefer the
# documented override; otherwise derive <phase>/fake.log from the worktree path
# (<phase>/repo-wt/<job>).
log_file=${VOWP_DOGFOOD_FAKE_LOG:-}
if [ -z "$log_file" ]; then
    parent=$(dirname "$PWD")
    case "$(basename "$parent")" in
        *-wt) log_file=$(dirname "$parent")/fake.log ;;
    esac
fi

log() {
    if [ -n "$log_file" ]; then
        printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$*" >>"$log_file"
    fi
}

if [ "${1:-}" = "--version" ]; then
    printf '%s\n' "fake-dogfood-harness 1.0.0"
    exit 0
fi

prompt=$(cat)


# --- verifier turns ---------------------------------------------------------
# The L3 prompt ends with "Job: <verify-job-id>" and does not name the report file;
# the report path is always .vowp/jobs/<id>/report.md relative to the shared worktree.
case "$prompt" in
    "You are the verifier for job "*)
        job=$(
            printf '%s\n' "$prompt" |
                sed -n 's/^Job: \(.*\)$/\1/p' |
                tail -n 1
        )
        if [ -z "$job" ]; then
            log "verifier: no job id in prompt"
            printf '%s\n' "fake-worker: verifier prompt has no job id" >&2
            exit 1
        fi
        branch=$(git rev-parse --abbrev-ref HEAD)
        report=".vowp/jobs/$job/report.md"
        mkdir -p "$(dirname "$report")"
        cat >"$report" <<EOF
---
job: "$job"
status: done
branch: "$branch"
commits: []
tests: "the verifier inspected the gate evidence and the diff"
files_changed: []
open_items: []
questions_for_owner: []
verdict: pass
findings: []
---

# $job

The fake verifier inspected the gate evidence for \`$branch\` and found no problem.
EOF
        log "verify $job"
        printf '%s\n' "fake-worker: verified $job on $branch"
        exit 0
        ;;
esac

# --- worker turns -----------------------------------------------------------
# The start prompt names the absolute report path; continue and fix use the other
# two documented phrasings. Match each shape up to its end marker (the paths under
# test may contain spaces).
report=$(
    printf '%s\n' "$prompt" |
        sed -n \
            -e 's|^.*write your report to \(.*/\.vowp/jobs/[A-Za-z0-9_-]*/report\.md\)$|\1|p' \
            -e 's|^.*writing your report (\(.*/\.vowp/jobs/[A-Za-z0-9_-]*/report\.md\)).*$|\1|p' \
            -e 's|^.*update \(.*/\.vowp/jobs/[A-Za-z0-9_-]*/report\.md\)\. Do not widen scope\.$|\1|p' |
        head -n 1
)
if [ -z "$report" ]; then
    # Planner/keeper runs carry no job report. Doing nothing keeps those loops alive
    # while still exercising the real supervisor if n08d has merged.
    log "no-job-report"
    printf '%s\n' "fake-worker: no job report in this prompt; nothing to do"
    exit 0
fi

job=$(basename "$(dirname "$report")")
case "$prompt" in
    "You stopped before writing your report"*)
        turn=continue
        ;;
    "Job "*" failed its checks "*)
        turn=fix
        ;;
    *)
        turn=start
        ;;
esac
log "$turn $job"

# Record whether the brief the worker was pointed at is the one written by
# `vowp job add` or the dispatch fallback. This is evidence for defect D4.
brief=$(
    printf '%s\n' "$prompt" |
        sed -n 's|^.*your brief at \(.*/\.vowp/jobs/[A-Za-z0-9_-]*/brief\.md\) first.*$|\1|p' |
        head -n 1
)
if [ -n "$brief" ]; then
    if [ ! -f "$brief" ]; then
        log "brief=missing $job"
    elif grep -q "Do job $job" "$brief"; then
        log "brief=generic $job"
    else
        log "brief=real $job"
    fi
fi

# The crash phase kills the supervisor while a worker is running. Its ids start with
# `crash-`; sleeping here widens that window without slowing phase 1. An explicit
# VOWP_DOGFOOD_FAKE_SLEEP overrides both.
sleep_secs=${VOWP_DOGFOOD_FAKE_SLEEP:-}
if [ -z "$sleep_secs" ]; then
    case "$job" in
        crash-*) sleep_secs=2 ;;
        *) sleep_secs=0 ;;
    esac
fi
case "$sleep_secs" in
    ''|*[!0-9.]*) sleep_secs=0 ;;
esac
if [ "$sleep_secs" != "0" ]; then
    sleep "$sleep_secs"
fi

case "$job" in
    *alpha)
        kind=alpha
        prefix=${job%alpha}
        ;;
    *beta)
        kind=beta
        prefix=${job%beta}
        ;;
    *gamma)
        kind=gamma
        prefix=${job%gamma}
        ;;
    *)
        log "unknown $job"
        printf '%s\n' "fake-worker: unknown job $job" >&2
        exit 1
        ;;
esac

file="src/$job.txt"
mkdir -p src
case "$kind" in
    alpha)
        printf '%s\n' "alpha from fake worker" >"$file"
        ;;
    beta)
        printf '%s\n' "beta from fake worker" >"$file"
        ;;
    gamma)
        dependency="src/${prefix}alpha.txt"
        if [ ! -f "$dependency" ]; then
            log "missing-dependency $job $dependency"
            printf '%s\n' "fake-worker: $job needs $dependency, which is not on the base branch" >&2
            exit 1
        fi
        {
            cat "$dependency"
            printf '%s\n' "gamma from fake worker"
        } >"$file"
        ;;
esac

git add -- "$file"
if ! git diff --cached --quiet; then
    git \
        -c user.name="Fake Dogfood Worker" \
        -c user.email="dogfood@example.invalid" \
        commit -q -m "$job: fake worker output"
fi
sha=$(git rev-parse --short HEAD)
branch=$(git rev-parse --abbrev-ref HEAD)

mkdir -p "$(dirname "$report")"
cat >"$report" <<EOF
---
job: "$job"
status: done
branch: "$branch"
commits: ["$sha"]
tests: "the job's cmd gate re-checks $file"
files_changed: ["$file"]
open_items: []
questions_for_owner: []
---

# $job

The fake worker wrote \`$file\` on branch \`$branch\` and committed it.
EOF

printf '%s\n' "fake-worker: finished $job on $branch ($sha)"
