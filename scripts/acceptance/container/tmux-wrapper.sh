#!/bin/sh
# scripts/acceptance/container/tmux-wrapper.sh — installed as /usr/local/bin/tmux.
#
# Every tmux call inside an acceptance container must use the private server
# named by VOWP_ACCEPTANCE_TMUX_SOCKET and the private TMUX_TMPDIR created by
# the rig entrypoint. This wrapper refuses the default server and refuses to
# run without a real private directory, because tmux silently falls back to the
# default server when TMUX_TMPDIR is missing.
#
# It is deliberately self-contained: no host binary is reachable from inside
# the container, so the worst mistake cannot touch the caller's tmux server.

set -eu

case "${VOWP_ACCEPTANCE_TMUX_SOCKET:-}" in
    ""|default)
        printf '%s\n' "acceptance tmux: refusing an empty or default socket" >&2
        exit 1
        ;;
esac
case "${TMUX_TMPDIR:-}" in
    ""|/tmp|/tmp/|/var/tmp|/var/tmp/|/tmp/tmux-*|/var/tmp/tmux-*)
        printf '%s\n' "acceptance tmux: refusing an unset or shared TMUX_TMPDIR: ${TMUX_TMPDIR:-<unset>}" >&2
        exit 1
        ;;
esac
if [ ! -d "$TMUX_TMPDIR" ]; then
    printf '%s\n' "acceptance tmux: TMUX_TMPDIR does not exist: $TMUX_TMPDIR" >&2
    exit 1
fi

previous=""
has_socket=0
for arg in "$@"; do
    if [ "$previous" = "-L" ] && [ "$arg" = "default" ]; then
        printf '%s\n' "acceptance tmux: refusing the default tmux socket" >&2
        exit 1
    fi
    case "$arg" in
        -Ldefault|-L=default)
            printf '%s\n' "acceptance tmux: refusing the default tmux socket" >&2
            exit 1
            ;;
        -L|-L?*) has_socket=1 ;;
    esac
    previous=$arg
done

unset TMUX TMUX_PANE || true
if [ "$has_socket" -eq 1 ]; then
    exec /usr/bin/tmux "$@"
fi
exec /usr/bin/tmux -L "$VOWP_ACCEPTANCE_TMUX_SOCKET" "$@"
