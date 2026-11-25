#!/bin/sh
# VOWP L8 watchdog: restart the supervisor when its heartbeat is stale.
#
# L1 step 13 rewrites `$VOWP_HOME/heartbeat` (unix seconds, ASCII) every tick.
# This script runs from cron every five minutes, and restarts `vowp.service`
# (or `stackspirit.service` on a fresh StackSpirit host) through the user systemd
# manager, or starts `vowp up --detach` (`stackspirit up --detach` when only the
# new name is installed) when systemd is unavailable. It only restarts the
# supervisor; job state is reconciled by the next L1 tick.
#
# Spec: docs/design/loops.md §10 (L8 — watchdog); heartbeat layout:
# docs/design/architecture.md §3. Keep this script in sync with
# vowp_core::loops::l8::CRON_CHECK_SH.

set -eu

# The installer copies this script as vowp-watchdog.sh (existing hosts) or
# stackspirit-watchdog.sh (fresh StackSpirit hosts); report under that name.
PROGRAM=${0##*/}
case "$PROGRAM" in
    vowp-watchdog.sh|stackspirit-watchdog.sh) ;;
    *) PROGRAM=vowp-watchdog.sh ;;
esac
MAX_AGE_SECS=180
DRY_RUN=0

usage() {
    cat <<'EOF'
Usage: vowp-watchdog.sh [--dry-run]

Checks the supervisor heartbeat in $VOWP_HOME/heartbeat ($STACKSPIRIT_HOME when
VOWP_HOME is unset; default ~/.vowp)
and restarts the supervisor when it is missing, unparsable, or older than
180 seconds. --dry-run prints what would happen without changing anything.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf '%s: unknown option: %s (try --help)\n' "$PROGRAM" "$1" >&2
            exit 2
            ;;
    esac
done

if [ -z "${HOME:-}" ]; then
    printf '%s: HOME is not set\n' "$PROGRAM" >&2
    exit 2
fi

# cron starts with a minimal PATH; keep the caller's entries so tests and
# wrappers can inject fakes, and append the default user binary directory.
PATH="${PATH:-/usr/local/bin:/usr/bin:/bin}:$HOME/.local/bin"
export PATH

# systemctl --user needs the user runtime directory when cron does not set it.
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    runtime_dir="/run/user/$(id -u)"
    if [ -d "$runtime_dir" ]; then
        XDG_RUNTIME_DIR=$runtime_dir
        export XDG_RUNTIME_DIR
    fi
fi

# Both home variable names point at the same directory for the supervisor.
VOWP_HOME=${VOWP_HOME:-${STACKSPIRIT_HOME:-"$HOME/.vowp"}}
STACKSPIRIT_HOME=$VOWP_HOME
export VOWP_HOME STACKSPIRIT_HOME

# The supervisor unit is chosen by existence, like the installer: an existing
# vowp.service wins, then stackspirit.service, else the legacy name.
UNIT=vowp.service
if [ ! -e "$HOME/.config/systemd/user/vowp.service" ] && [ -e "$HOME/.config/systemd/user/stackspirit.service" ]; then
    UNIT=stackspirit.service
fi
HEARTBEAT=$VOWP_HOME/heartbeat
WATCHDOG_LOG=$VOWP_HOME/logs/watchdog.log

now=$(date +%s)
age=
if [ -f "$HEARTBEAT" ]; then
    value=$(head -n 1 "$HEARTBEAT" 2>/dev/null | tr -d '[:space:]' || true)
    case "$value" in
        ''|*[!0-9]*)
            value=
            ;;
    esac
    # Unix seconds are 10 digits today; a longer token is not a heartbeat.
    if [ "${#value}" -gt 12 ]; then
        value=
    fi
    # Strip leading zeros so POSIX arithmetic never sees 08 as an octal literal.
    while [ -n "$value" ] && [ "${value#0}" != "$value" ]; do
        value=${value#0}
    done
    if [ -n "$value" ]; then
        age=$((now - value))
        if [ "$age" -lt 0 ]; then
            age=0
        fi
    fi
fi

stale=0
reason=
if [ ! -f "$HEARTBEAT" ]; then
    stale=1
    reason="heartbeat is missing"
elif [ -z "$age" ]; then
    stale=1
    reason="heartbeat is not a unix timestamp"
elif [ "$age" -gt "$MAX_AGE_SECS" ]; then
    stale=1
    reason="heartbeat is ${age}s old (limit ${MAX_AGE_SECS}s)"
fi

if [ "$stale" -eq 0 ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
        printf '%s: dry-run: heartbeat is fresh (%ss old); nothing to do\n' "$PROGRAM" "$age"
    fi
    exit 0
fi

if [ "$DRY_RUN" -eq 1 ]; then
    printf '%s: dry-run: %s; would restart the supervisor\n' "$PROGRAM" "$reason"
    if command -v systemctl >/dev/null 2>&1; then
        printf '%s: would run: systemctl --user restart %s\n' "$PROGRAM" "$UNIT"
    else
        printf '%s: would run: vowp up --detach\n' "$PROGRAM"
    fi
    exit 0
fi

# Append one line to the watchdog log; a read-only home must not stop the restart.
log_event() {
    if ! mkdir -p "$(dirname "$WATCHDOG_LOG")" 2>/dev/null; then
        printf '%s: warning: cannot create %s\n' "$PROGRAM" "$(dirname "$WATCHDOG_LOG")" >&2
    fi
    if ! printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$1" >> "$WATCHDOG_LOG" 2>/dev/null; then
        printf '%s: warning: cannot write %s\n' "$PROGRAM" "$WATCHDOG_LOG" >&2
    fi
}

# The CLI for the fallback: the legacy name when present, else the new one.
CLI=vowp
if ! command -v vowp >/dev/null 2>&1 && command -v stackspirit >/dev/null 2>&1; then
    CLI=stackspirit
fi

restarted=0
if command -v systemctl >/dev/null 2>&1; then
    if systemctl --user restart "$UNIT"; then
        restarted=1
        log_event "$reason; restarted $UNIT with systemctl --user"
    else
        log_event "$reason; systemctl --user restart $UNIT failed; trying $CLI up --detach"
    fi
fi

if [ "$restarted" -eq 0 ]; then
    if command -v "$CLI" >/dev/null 2>&1; then
        if "$CLI" up --detach; then
            restarted=1
            log_event "$reason; started the supervisor with '$CLI up --detach'"
        else
            log_event "$reason; '$CLI up --detach' failed"
        fi
    else
        log_event "$reason; no systemd restart and no vowp or stackspirit binary on PATH"
    fi
fi

if [ "$restarted" -eq 0 ]; then
    printf '%s: could not restart the supervisor: %s\n' "$PROGRAM" "$reason" >&2
    exit 1
fi

exit 0
