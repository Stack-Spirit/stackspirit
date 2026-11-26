#!/usr/bin/env bash
# Run the remote-exec streaming spike.
#
#   ./run.sh              hermetic tests only (no sockets, no sshd, no tmux)
#   ./run.sh --live       hermetic tests, then a private loopback sshd run
#   ./run.sh --live --out evidence/live-check.json
#
# The live mode binds a private sshd to 127.0.0.1 on a free port, with temporary
# keys under the system temporary directory. It never touches the real VOWP home,
# the user's tmux server, the network or a harness.

set -euo pipefail

HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
MODE=${1:-}
if [ "$MODE" = "--live" ]; then
    shift
elif [ -z "$MODE" ] || [ "$MODE" = "--hermetic" ]; then
    shift || true
else
    printf 'usage: %s [--hermetic|--live] [live options]\n' "$0" >&2
    exit 2
fi

export PYTHONDONTWRITEBYTECODE=1
# A caller inside tmux exports these; this spike never uses tmux, but clear them
# so a future helper cannot accidentally attach to the caller's server.
unset TMUX TMUX_PANE 2>/dev/null || true

# The repo-wide safety_grep test reads every file under scripts/ as UTF-8, so a
# stray .pyc would fail it. Clean bytecode on every exit, including failures.
cleanup_pycache() {
    find "$HERE" -name '__pycache__' -type d -prune -exec rm -rf {} + 2>/dev/null || true
}
trap cleanup_pycache EXIT

python3 -B -m unittest discover -s "$HERE/tests" -v

if [ "$MODE" = "--live" ]; then
    python3 -B "$HERE/live_check.py" "$@"
fi
