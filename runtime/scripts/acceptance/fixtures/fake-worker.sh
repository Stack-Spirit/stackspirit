#!/usr/bin/env bash
# scripts/acceptance/fixtures/fake-worker.sh — a deterministic fake harness.
#
# This is a test fixture, not a product adapter. It never calls a model or the
# network. It reads the VOWP worker prompt from stdin, writes the owned file,
# commits it, and writes a report that satisfies the `report` gate.
#
# Modes:
#   default            write src/<job>.txt and a small report
#   --hog --mb N --hold S
#                      first allocate and touch N MiB, hold it for S seconds,
#                      then finish the job. Used by the memory-hog project.
#
# Planner and keeper prompts carry no job report and exit 0. A verifier prompt
# is answered with a passing verifier report, which lets a fixture project use
# the real review gate without a model.
#
# Environment:
#   VOWP_ACCEPTANCE_WORKER_LOG   append one line per invocation (optional)
#
# The script is intentionally POSIX-bash and works in Ubuntu and macOS.

set -Eeuo pipefail

mode=normal
hog_mb=256
hog_hold=20
mode_arg=

log_line() {
    if [ -n "${VOWP_ACCEPTANCE_WORKER_LOG:-}" ]; then
        printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$*" >>"$VOWP_ACCEPTANCE_WORKER_LOG" 2>/dev/null || true
    fi
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            printf '%s\n' "acceptance-fake-worker 1.0.0"
            exit 0
            ;;
        --hog)
            mode=hog
            shift
            ;;
        --mb)
            [ "$#" -ge 2 ] || { printf '%s\n' "fake-worker: --mb needs a number" >&2; exit 2; }
            hog_mb=$2
            shift 2
            ;;
        --hold)
            [ "$#" -ge 2 ] || { printf '%s\n' "fake-worker: --hold needs seconds" >&2; exit 2; }
            hog_hold=$2
            shift 2
            ;;
        --)
            shift
            break
            ;;
        *)
            mode_arg=$1
            shift
            ;;
    esac
done

case "$hog_mb" in
    ''|*[!0-9]*) printf '%s\n' "fake-worker: --mb must be a whole number of MiB" >&2; exit 2 ;;
esac
case "$hog_hold" in
    ''|*[!0-9]*) printf '%s\n' "fake-worker: --hold must be a whole number of seconds" >&2; exit 2 ;;
esac

prompt=$(cat)

# Pull the report path out of the prompt. Keep the path intact even when the
# acceptance state directory contains spaces. These are the exact prompt
# shapes from l2_worker.rs and l3_verify.rs; the generic fallbacks cover a
# sentence reflow without weakening the report contract.
report=$(
    printf '%s\n' "$prompt" |
        sed -n \
            -e 's|^.*write your report to \(.*/\.vowp/jobs/[A-Za-z0-9_.-]*/report\.md\)$|\1|p' \
            -e 's|^.*stopped before writing your report (\(.*/\.vowp/jobs/[A-Za-z0-9_.-]*/report\.md\)).*$|\1|p' \
            -e 's|^.*update \(.*/\.vowp/jobs/[A-Za-z0-9_.-]*/report\.md\)\. Do not widen scope\.$|\1|p' |
        head -n 1
)

# Verifier prompts do not name the verify job's report path. They end with
# `Job: <verify-job-id>` and give the worktree, so the path is deterministic.
if [ -z "$report" ]; then
    case "$prompt" in
        "You are the verifier for job "*)
            verify_id=$(
                printf '%s\n' "$prompt" |
                    sed -n 's/^Job: \(.*\)$/\1/p' |
                    tail -n 1
            )
            if [ -n "$verify_id" ]; then
                report="$PWD/.vowp/jobs/$verify_id/report.md"
            fi
            ;;
    esac
fi

if [ -z "$report" ]; then
    # A planner, keeper or other no-report turn is a deliberate no-op.
    log_line "no-report ${mode_arg:-prompt}"
    printf '%s\n' "fake-worker: no job report in this prompt; nothing to do"
    exit 0
fi

job=$(basename "$(dirname "$report")")
[ -n "$job" ] || { printf '%s\n' "fake-worker: could not derive a job id" >&2; exit 1; }
log_line "start $job mode=$mode mb=$hog_mb hold=$hog_hold"

# Verifier turn: do not edit the reviewed files, only produce a verdict.
case "$prompt" in
    "You are the verifier for job "*)
        branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || printf 'unknown')
        sha=$(git rev-parse --short HEAD 2>/dev/null || printf '')
        mkdir -p "$(dirname "$report")"
        cat >"$report" <<EOF
---
job: "$job"
status: done
branch: "$branch"
commits: ["$sha"]
tests: "the acceptance fake verifier inspected the worktree"
files_changed: []
open_items: []
questions_for_owner: []
verdict: pass
findings: []
---

# $job

The acceptance fake verifier found no problem with the fixture job.
EOF
        log_line "verified $job"
        printf '%s\n' "fake-worker: verified $job"
        exit 0
        ;;
esac

if [ "$mode" = "hog" ]; then
    if ! command -v python3 >/dev/null 2>&1; then
        printf '%s\n' "fake-worker: --hog needs python3" >&2
        exit 1
    fi
    python3 - "$hog_mb" "$hog_hold" <<'PY' &
import sys
import time

megabytes = int(sys.argv[1])
hold_seconds = int(sys.argv[2])
if megabytes < 0:
    raise SystemExit("--mb must be >= 0")
chunk = 1024 * 1024
blocks = []
for _ in range(megabytes):
    block = bytearray(chunk)
    # Touch one byte per page so the kernel accounts real resident pages.
    for offset in range(0, chunk, 4096):
        block[offset] = 1
    blocks.append(block)
time.sleep(hold_seconds)
# Release explicitly before exit so a following job can reuse the memory.
blocks.clear()
PY
    hog_pid=$!
    # The PID lets the self-test read VmHWM from /proc while the hog holds its
    # memory, so a worker that only logged and slept cannot pass.
    log_line "hog $job allocating ${hog_mb}MiB for ${hog_hold}s pid=$hog_pid"
    if ! wait "$hog_pid"; then
        printf '%s\n' "fake-worker: memory hog failed for $job" >&2
        exit 1
    fi
fi

mkdir -p src
printf '%s\n' "fake acceptance worker finished $job" >"src/$job.txt"
git add -- "src/$job.txt"
if ! git diff --cached --quiet; then
    git -c user.name="Acceptance Fake Worker" \
        -c user.email="acceptance@example.invalid" \
        commit -q -m "$job: fake acceptance worker"
fi

sha=$(git rev-parse --short HEAD 2>/dev/null || printf '')
branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || printf 'unknown')
mkdir -p "$(dirname "$report")"
cat >"$report" <<EOF
---
job: "$job"
status: done
branch: "$branch"
commits: ["$sha"]
tests: "the acceptance fake worker wrote src/$job.txt"
files_changed: ["src/$job.txt"]
open_items: []
questions_for_owner: []
---

# $job

The acceptance fake worker wrote \`src/$job.txt\` on branch \`$branch\` and committed it.
EOF
log_line "done $job"
printf '%s\n' "fake-worker: finished $job on $branch ($sha)"
