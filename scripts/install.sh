#!/bin/sh
# install.sh — install the VOWP binary and the L8 watchdog.
#
# This is a per-user install; it never needs root. It writes:
#   $HOME/.local/bin/vowp
#   $HOME/.local/bin/vowp-watchdog.sh       (checked by `vowp doctor`)
#   $HOME/.config/systemd/user/vowp.service (checked by `vowp doctor`)
# and adds one five-minute cron entry for the watchdog.
#
# Spec: docs/design/loops.md §10 (L8 — watchdog); node paths:
# docs/design/architecture.md §3. The service and cron templates must match
# vowp_core::loops::l8::{SYSTEMD_UNIT,CRON_CHECK_SH} once n08d exposes them.

set -eu

PROGRAM=install.sh
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)

BIN_SOURCE=
DRY_RUN=0
WITH_SERVICE=1
WITH_CRON=1
WITH_LINGER=0

CRON_MARKER='# vowp: 5-minute L8 watchdog check (managed by scripts/install.sh)'

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install the vowp binary, the systemd user unit and the cron watchdog for the
current user. No root is required.

Options:
  --bin PATH      vowp binary to install (default: release then debug under
                  $CARGO_TARGET_DIR or target/, then `vowp` on PATH)
  --dry-run       print every action without changing files, services or cron
  --no-service    do not enable or start the systemd user unit
  --no-cron       do not change the crontab
  --linger        run `loginctl enable-linger` for the current user
  -h, --help      show this help
EOF
}

note() {
    printf '%s: %s\n' "$PROGRAM" "$*"
}

warn() {
    printf '%s: warning: %s\n' "$PROGRAM" "$*" >&2
}

die() {
    printf '%s: error: %s\n' "$PROGRAM" "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin)
            [ "$#" -ge 2 ] || die "--bin needs a path"
            BIN_SOURCE=$2
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --no-service)
            WITH_SERVICE=0
            shift
            ;;
        --no-cron)
            WITH_CRON=0
            shift
            ;;
        --linger)
            WITH_LINGER=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            die "unknown option: $1 (try --help)"
            ;;
    esac
done

[ "$#" -eq 0 ] || die "unexpected argument: $1"
[ -n "${HOME:-}" ] || die "HOME is not set"

BIN_DIR=$HOME/.local/bin
UNIT_DIR=$HOME/.config/systemd/user
UNIT_PATH=$UNIT_DIR/vowp.service
WATCHDOG_PATH=$BIN_DIR/vowp-watchdog.sh
VOWP_PATH=$BIN_DIR/vowp

find_binary() {
    if [ -n "$BIN_SOURCE" ]; then
        printf '%s\n' "$BIN_SOURCE"
        return 0
    fi
    if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -x "$CARGO_TARGET_DIR/release/vowp" ]; then
        printf '%s\n' "$CARGO_TARGET_DIR/release/vowp"
        return 0
    fi
    if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -x "$CARGO_TARGET_DIR/debug/vowp" ]; then
        printf '%s\n' "$CARGO_TARGET_DIR/debug/vowp"
        return 0
    fi
    if [ -x "$REPO_ROOT/target/release/vowp" ]; then
        printf '%s\n' "$REPO_ROOT/target/release/vowp"
        return 0
    fi
    if [ -x "$REPO_ROOT/target/debug/vowp" ]; then
        printf '%s\n' "$REPO_ROOT/target/debug/vowp"
        return 0
    fi
    if command -v vowp >/dev/null 2>&1; then
        command -v vowp
        return 0
    fi
    return 1
}

SOURCE_BIN=
if [ -n "$BIN_SOURCE" ]; then
    [ -f "$BIN_SOURCE" ] || die "binary not found: $BIN_SOURCE"
    [ -x "$BIN_SOURCE" ] || die "binary is not executable: $BIN_SOURCE"
    SOURCE_BIN=$BIN_SOURCE
elif SOURCE_BIN=$(find_binary); then
    :
else
    SOURCE_BIN=
fi

if [ -z "$SOURCE_BIN" ] && [ "$DRY_RUN" -eq 0 ]; then
    die "no vowp binary found; build it with 'cargo build --release -p vowp-cli --bin vowp' or pass --bin PATH"
fi
if [ -z "$SOURCE_BIN" ]; then
    warn "no vowp binary found; a real install would stop here (build it or pass --bin)"
fi

[ -f "$SCRIPT_DIR/vowp-watchdog.sh" ] || die "missing $SCRIPT_DIR/vowp-watchdog.sh"
[ -f "$SCRIPT_DIR/vowp.service" ] || die "missing $SCRIPT_DIR/vowp.service"

cron_line() {
    printf '*/5 * * * * "%s" >/dev/null 2>&1\n' "$WATCHDOG_PATH"
}

if [ "$DRY_RUN" -eq 1 ]; then
    note "dry-run: no files, services or crontabs will be changed"
    if [ -n "$SOURCE_BIN" ]; then
        note "would install binary $SOURCE_BIN -> $VOWP_PATH"
    else
        note "would install binary <not found> -> $VOWP_PATH"
    fi
    note "would install watchdog $SCRIPT_DIR/vowp-watchdog.sh -> $WATCHDOG_PATH"
    note "would install unit $SCRIPT_DIR/vowp.service -> $UNIT_PATH"
    if [ "$WITH_SERVICE" -eq 1 ]; then
        note "would run: systemctl --user daemon-reload"
        note "would run: systemctl --user enable --now vowp.service"
    else
        note "would skip enabling the systemd user unit (--no-service)"
    fi
    if [ "$WITH_LINGER" -eq 1 ]; then
        note "would run: loginctl enable-linger $(id -un)"
    fi
    if [ "$WITH_CRON" -eq 1 ]; then
        note "would install crontab entry: $(cron_line)"
    else
        note "would skip the crontab update (--no-cron)"
    fi
    note "next: vowp doctor"
    exit 0
fi

install_file() {
    src=$1
    dst=$2
    mode=$3
    if command -v install >/dev/null 2>&1; then
        install -m "$mode" "$src" "$dst"
    else
        cp "$src" "$dst"
        chmod "$mode" "$dst"
    fi
}

mkdir -p "$BIN_DIR" "$UNIT_DIR"

if [ "$SOURCE_BIN" != "$VOWP_PATH" ]; then
    install_file "$SOURCE_BIN" "$VOWP_PATH" 0755
    note "installed $VOWP_PATH"
else
    note "binary already at $VOWP_PATH"
fi

install_file "$SCRIPT_DIR/vowp-watchdog.sh" "$WATCHDOG_PATH" 0755
note "installed $WATCHDOG_PATH"

install_file "$SCRIPT_DIR/vowp.service" "$UNIT_PATH" 0644
note "installed $UNIT_PATH"

if [ "$WITH_SERVICE" -eq 1 ]; then
    if command -v systemctl >/dev/null 2>&1; then
        if systemctl --user daemon-reload; then
            :
        else
            warn "systemctl --user daemon-reload failed; run it after your user manager is available"
        fi
        if systemctl --user enable --now vowp.service; then
            note "enabled and started vowp.service"
        else
            warn "could not enable/start vowp.service; run 'systemctl --user enable --now vowp.service' after logging in"
        fi
    else
        warn "systemctl not found; the unit is installed but not enabled"
    fi
else
    note "systemd user unit left disabled (--no-service)"
fi

if [ "$WITH_LINGER" -eq 1 ]; then
    if command -v loginctl >/dev/null 2>&1; then
        if loginctl enable-linger "$(id -un)"; then
            note "enabled linger for $(id -un)"
        else
            warn "loginctl enable-linger failed; the supervisor will only run while you are logged in"
        fi
    else
        warn "loginctl not found; linger was not enabled"
    fi
fi

install_cron() {
    if ! command -v crontab >/dev/null 2>&1; then
        warn "crontab not found; add this entry manually:"
        printf '  %s\n  %s\n' "$CRON_MARKER" "$(cron_line)" >&2
        return 0
    fi

    tmp=$(mktemp "${TMPDIR:-/tmp}/vowp-crontab.XXXXXX") || die "mktemp failed"
    trap 'rm -f "$tmp" "$tmp.filtered" "$tmp.err"' 0 1 2 15

    if ! crontab -l > "$tmp" 2> "$tmp.err"; then
        if grep -qi 'no crontab' "$tmp.err"; then
            : > "$tmp"
        else
            detail=$(head -n 1 "$tmp.err" 2>/dev/null || true)
            [ -n "$detail" ] || detail="unknown error"
            warn "could not read the crontab ($detail); leaving it unchanged"
            warn "add this entry manually:"
            printf '  %s\n  %s\n' "$CRON_MARKER" "$(cron_line)" >&2
            rm -f "$tmp" "$tmp.filtered" "$tmp.err"
            return 0
        fi
    fi
    awk -v marker="$CRON_MARKER" '
        $0 == marker { next }
        $0 ~ /^\*\/5 \* \* \* \* ".*\/vowp-watchdog\.sh" >\/dev\/null 2>&1$/ { next }
        { print }
    ' "$tmp" > "$tmp.filtered"
    {
        printf '%s\n' "$CRON_MARKER"
        cron_line
    } >> "$tmp.filtered"

    if crontab "$tmp.filtered"; then
        note "installed crontab entry for $WATCHDOG_PATH (every 5 minutes)"
    else
        warn "could not update the crontab; add this entry manually:"
        printf '  %s\n  %s\n' "$CRON_MARKER" "$(cron_line)" >&2
    fi

    rm -f "$tmp" "$tmp.filtered" "$tmp.err"
}

if [ "$WITH_CRON" -eq 1 ]; then
    install_cron
else
    note "crontab left unchanged (--no-cron)"
fi

note "next: run 'vowp doctor' and check the systemd and watchdog checks"
