#!/bin/sh
# Run the capacity-spike job N times in one mode and summarise the results.
#
#   ./run_three.sh [--mode host|shim] [--runs 3] [--out DIR] [--build-cmd CMD]
#
# `host` runs cargo directly on the host (the stock-VPS case, where both
# measurement methods can see every process).  `shim` runs the normal
# night-shift cargo shim, which puts the Rust build in a Docker container; the
# cgroup method follows the container cgroup, while the /proc walk cannot.
#
# The script creates a private TMUX_TMPDIR (and a temporary VOWP_HOME), unsets
# TMUX/TMUX_PANE, and removes only its own temporary directory on exit.  It
# never starts tmux and never touches ~/.vowp.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
mode=host
runs=3
out=
build_cmd=
harness_mib=192
harness_cpu_ms=400
sample_ms=100

usage() {
    sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --mode) mode=$2; shift 2 ;;
        --runs) runs=$2; shift 2 ;;
        --out) out=$2; shift 2 ;;
        --build-cmd) build_cmd=$2; shift 2 ;;
        --harness-mib) harness_mib=$2; shift 2 ;;
        --harness-cpu-ms) harness_cpu_ms=$2; shift 2 ;;
        --sample-ms) sample_ms=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "run_three: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$mode" in
    host|shim) ;;
    *) echo "run_three: --mode must be host or shim" >&2; exit 2 ;;
esac
case "$runs" in
    ''|*[!0-9]*) echo "run_three: --runs must be a number" >&2; exit 2 ;;
esac
[ "$runs" -ge 1 ] || { echo "run_three: --runs must be at least 1" >&2; exit 2; }

base=${TMPDIR:-/tmp}
[ -d /tmp/opencode ] && base=/tmp/opencode
tmp=$(mktemp -d "$base/vowp-capacity-$mode-XXXXXX")
cleanup() {
    rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$tmp/tmux" "$tmp/home" "$tmp/tmp"
export TMUX_TMPDIR="$tmp/tmux"
export VOWP_HOME="$tmp/home"
export TMPDIR="$tmp/tmp"
unset TMUX TMUX_PANE || true

if [ -z "$out" ]; then
    stamp=$(date -u '+%Y%m%dT%H%M%SZ')
    out="$here/results/$stamp-$mode"
fi
mkdir -p "$out"

if [ -z "$build_cmd" ]; then
    build_cmd="cargo test --workspace"
fi

echo "capacity runs: mode=$mode runs=$runs out=$out" >&2
echo "  private TMUX_TMPDIR=$TMUX_TMPDIR" >&2
echo "  temporary VOWP_HOME=$VOWP_HOME" >&2
echo "  build: $build_cmd" >&2

i=1
rc=0
while [ "$i" -le "$runs" ]; do
    label="$mode-$i"
    echo "== $label ==" >&2
    CAP_SCOPE_LABEL="$label" "$here/run_one.sh" \
        python3 "$here/job_measure.py" \
        --label "$label" \
        --cargo-mode "$mode" \
        --build-cmd "$build_cmd" \
        --harness-mib "$harness_mib" \
        --harness-cpu-ms "$harness_cpu_ms" \
        --sample-ms "$sample_ms" \
        --out "$out/$label.json" \
        --log "$out/$label.log" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "run_three: $label failed with exit $rc; see $out/$label.log" >&2
        break
    fi
    i=$((i + 1))
done

if command -v python3 >/dev/null 2>&1; then
    python3 "$here/summarize.py" --out "$out" --mode "$mode" || true
fi

exit "$rc"
