#!/bin/sh
# Hermetic self-test for the capacity-spike tools.
#
# It proves the measurement tooling itself works before the numbers are trusted:
# the probe sees cgroup v2 and MemoryHigh, a small job is measured with both
# methods, and the /proc-only fallback still records a peak when the cgroup
# scope is disabled.  It uses a temporary directory, a private TMUX_TMPDIR and a
# temporary VOWP_HOME; it never starts tmux, a real harness or a network client.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
base=${TMPDIR:-/tmp}
[ -d /tmp/opencode ] && base=/tmp/opencode
tmp=$(mktemp -d "$base/vowp-capacity-selftest-XXXXXX")
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT HUP INT TERM

mkdir -p "$tmp/tmux" "$tmp/home" "$tmp/out" "$tmp/tmp"
export TMUX_TMPDIR="$tmp/tmux"
export VOWP_HOME="$tmp/home"
export TMPDIR="$tmp/tmp"
unset TMUX TMUX_PANE || true

echo "== probe =="
python3 "$here/probe.py" --json "$tmp/out/probe.json"

echo "== cgroup + proc measurement =="
CAP_SCOPE_LABEL=selftest-cgroup "$here/run_one.sh" \
    python3 "$here/job_measure.py" \
    --label selftest-cgroup \
    --cargo-mode host \
    --build-cmd "python3 -c 'import time; time.sleep(0.8)'" \
    --harness-mib 32 \
    --harness-cpu-ms 100 \
    --out "$tmp/out/cgroup.json" \
    --log "$tmp/out/cgroup.log"

echo "== /proc-only fallback =="
CAP_SCOPE=0 \
    python3 "$here/job_measure.py" \
    --label selftest-proc \
    --cargo-mode host \
    --build-cmd "python3 -c 'import time; time.sleep(0.5)'" \
    --harness-mib 32 \
    --harness-cpu-ms 100 \
    --out "$tmp/out/proc.json" \
    --log "$tmp/out/proc.log"

python3 - "$tmp/out/cgroup.json" "$tmp/out/proc.json" <<'PY'
import json
import sys

MIB = 1024 * 1024
cgroup = json.load(open(sys.argv[1], encoding="utf-8"))
proc = json.load(open(sys.argv[2], encoding="utf-8"))

def check(condition, message):
    if not condition:
        raise SystemExit(f"selftest: {message}")
    print(f"ok   {message}")

check(cgroup["build_exit_code"] == 0, "measured job exited 0")
check(cgroup["cgroup_setup"]["nested"], "nested cgroups were created inside the scope")
check(cgroup["summary"]["harness_peak_bytes"] >= 32 * MIB, "cgroup harness peak covers the 32 MiB harness")
check(cgroup["summary"]["job_peak_bytes"] >= cgroup["summary"]["harness_peak_bytes"], "job peak is at least the harness peak")
check(cgroup["summary"]["job_cpu_usec"] > 0, "cgroup CPU time was recorded")
check(cgroup["proc"]["samples"] >= 3, "the /proc walk took samples")
check(cgroup["proc"]["job_peak_rss_bytes"] >= 32 * MIB, "/proc peak covers the 32 MiB harness")

check(not proc["cgroup_setup"]["nested"], "CAP_SCOPE=0 disables nested cgroups")
check(not proc["cgroup"]["available"], "the fallback reports cgroup accounting as unavailable")
check(proc["build_exit_code"] == 0, "fallback job exited 0")
check(proc["proc"]["job_peak_rss_bytes"] >= 32 * MIB, "fallback /proc peak covers the 32 MiB harness")
check(proc["proc"]["samples"] >= 3, "fallback took samples")
print("selftest: ok")
PY
