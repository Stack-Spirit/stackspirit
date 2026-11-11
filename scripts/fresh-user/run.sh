#!/usr/bin/env bash
# scripts/fresh-user/run.sh - follow the VOWP docs as a new user in a clean container.
#
# The script:
#   1. extracts the documented commands from docs/guides/install.md,
#      docs/guides/quickstart.md and examples/hello-vowp/README.md;
#   2. builds a clean Rust image with that checkout (no target/, no .git) and
#      no VOWP binary;
#   3. runs every selected command in order, one timed step at a time, and stops
#      at the first failure with the source doc line.
#
# The container has no network except what cargo and git need, no secrets, and
# no real harness. Two tiny fake executables answer only `--version` so the
# documented `vowp doctor` can run; they never fake a model turn. The hello-vowp
# example supplies its own fake worker for the full graph.
#
# Usage:
#   bash scripts/fresh-user/run.sh
#   bash scripts/fresh-user/run.sh --help
#
# Variables:
#   FRESH_USER_IMAGE=tag   image tag to use (default: vowp-fresh-user:local)
#   KEEP_IMAGE=1           keep the built image for inspection
#   KEEP_WORK=1            keep the temporary build context and log
#
# Exit status is 0 only when every selected documented command and every
# documented-output check passed.

set -euo pipefail

usage() {
    awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$0"
}

case "${1:-}" in
    -h|--help)
        usage
        exit 0
        ;;
    "")
        ;;
    *)
        echo "fresh-user: unknown argument: $1 (try --help)" >&2
        exit 2
        ;;
esac

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH='' cd -- "$here/../.." && pwd)

if ! command -v docker >/dev/null 2>&1; then
    echo "fresh-user: docker is required" >&2
    exit 2
fi
if ! docker version >/dev/null 2>&1; then
    echo "fresh-user: the docker daemon is not reachable" >&2
    exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "fresh-user: python3 is required to read the docs" >&2
    exit 2
fi

image=${FRESH_USER_IMAGE:-vowp-fresh-user:local}
suffix=$$
container="vowp-fresh-user-$suffix"
work=$(mktemp -d "${TMPDIR:-/tmp}/vowp-fresh-user.XXXXXX")
context="$work/context"
log="$work/run.log"
status=0

cleanup() {
    current=$?
    trap - EXIT INT TERM
    if docker container inspect "$container" >/dev/null 2>&1; then
        docker rm -f "$container" >/dev/null 2>&1 || true
    fi
    if [ "${KEEP_IMAGE:-0}" != "1" ]; then
        docker image rm -f "$image" >/dev/null 2>&1 || true
    fi
    if [ "${KEEP_WORK:-0}" = "1" ]; then
        echo "fresh-user: kept work directory $work" >&2
    else
        rm -rf "$work"
    fi
    exit "$current"
}
trap cleanup EXIT INT TERM

mkdir -p "$context/repo" "$context/fake-bin"

# The build context is the checkout itself, minus build output and git state.
# A worktree's .git is a file pointing outside the tree, so it must not be
# copied. target/ is never part of a fresh checkout.
echo "fresh-user: preparing build context from $root"
tar -C "$root" \
    --exclude=.git \
    --exclude=target \
    --exclude='*.pyc' \
    --exclude=__pycache__ \
    -cf - . | tar -C "$context/repo" -xf -

cp "$here/Dockerfile" "$context/Dockerfile"
cp "$here/extract.py" "$context/extract.py"
cp "$here/fake-bin/claude" "$context/fake-bin/claude"
cp "$here/fake-bin/opencode" "$context/fake-bin/opencode"
chmod 0755 "$context/fake-bin/claude" "$context/fake-bin/opencode"

echo "fresh-user: extracting the documented commands"
python3 "$here/extract.py" \
    --root "$root" \
    --out "$context/fresh-user-plan.sh" \
    --manifest "$work/plan.json"
chmod 0755 "$context/fresh-user-plan.sh"

steps=$(python3 -c 'import json,sys; p=json.load(open(sys.argv[1])); print(len(p["steps"]))' "$work/plan.json")
skips=$(python3 -c 'import json,sys; p=json.load(open(sys.argv[1])); print(len(p["skipped"]))' "$work/plan.json")
echo "fresh-user: $steps documented command(s), $skips documented-planned skip(s)"

echo "fresh-user: building $image"
build_start=$(date +%s)
docker build --tag "$image" "$context" >"$work/build.log" 2>&1 || {
    status=$?
    echo "fresh-user: docker build failed; last 80 lines:" >&2
    tail -n 80 "$work/build.log" >&2
    exit "$status"
}
build_seconds=$(( $(date +%s) - build_start ))
echo "fresh-user: image built in ${build_seconds}s"

echo "fresh-user: running the documented commands in the container"
set +e
docker run --rm --name "$container" "$image" /work/fresh-user-plan.sh 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
set -e

if [ "$status" -eq 0 ]; then
    echo "fresh-user: PASS (documented commands, output checks and hello-vowp example)"
else
    echo "fresh-user: FAIL (exit $status); the failing doc line is in the output above" >&2
fi
exit "$status"
