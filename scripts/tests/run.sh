#!/bin/sh
# Tests for the VOWP ops scripts. Hermetic: a temporary HOME, fake systemctl,
# crontab, loginctl and vowp on PATH, and a temporary VOWP_HOME.
#
# Run: sh scripts/tests/run.sh
# Set VOWP_BIN=/path/to/vowp to also check that `vowp doctor` sees the files.

set -eu

TESTS_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
SCRIPTS_DIR=$(CDPATH='' cd -- "$TESTS_DIR/.." && pwd)
ROOT_DIR=$(CDPATH='' cd -- "$SCRIPTS_DIR/.." && pwd)
FAKES_DIR=$TESTS_DIR/fakes

TMP=$(mktemp -d "${TMPDIR:-/tmp}/vowp-scripts-test.XXXXXX")
trap 'rm -rf "$TMP"' 0 1 2 15

ORIGINAL_PATH=$PATH
FAKEBIN=$TMP/fakebin
HOME_DIR=$TMP/home
VOWP_HOME_DIR=$HOME_DIR/.vowp
SRC_BIN=$TMP/src/vowp

mkdir -p "$FAKEBIN" "$TMP/logs" "$HOME_DIR" "$TMP/src"
for fake in systemctl crontab loginctl vowp; do
    cp "$FAKES_DIR/$fake" "$FAKEBIN/$fake"
    chmod 0755 "$FAKEBIN/$fake"
done
printf '#!/bin/sh\nexit 0\n' > "$SRC_BIN"
chmod 0755 "$SRC_BIN"

export FAKE_SYSTEMCTL_LOG="$TMP/logs/systemctl.log"
export FAKE_CRONTAB_LOG="$TMP/logs/crontab.log"
export FAKE_LOGINCTL_LOG="$TMP/logs/loginctl.log"
export FAKE_VOWP_LOG="$TMP/logs/vowp.log"
export FAKE_CRONTAB_FILE="$TMP/crontab.txt"
: > "$FAKE_SYSTEMCTL_LOG"
: > "$FAKE_CRONTAB_LOG"
: > "$FAKE_LOGINCTL_LOG"
: > "$FAKE_VOWP_LOG"

TEST_PATH=$FAKEBIN:$ORIGINAL_PATH

fail() {
    printf 'not ok - %s\n' "$*" >&2
    exit 1
}

pass() {
    printf 'ok - %s\n' "$*"
}

assert_file() {
    if [ ! -f "$1" ]; then
        fail "missing file: $1"
    fi
}

assert_absent() {
    if [ -e "$1" ]; then
        fail "unexpected path: $1"
    fi
}

assert_contains() {
    if ! grep -F -- "$2" "$1" >/dev/null 2>&1; then
        fail "expected '$2' in $1"
    fi
}

assert_not_contains() {
    if grep -F -- "$2" "$1" >/dev/null 2>&1; then
        fail "did not expect '$2' in $1"
    fi
}

assert_text_contains() {
    if ! printf '%s\n' "$1" | grep -F -- "$2" >/dev/null 2>&1; then
        fail "expected '$2' in command output"
    fi
}

assert_log_contains() {
    if [ ! -f "$1" ] || ! grep -F -- "$2" "$1" >/dev/null 2>&1; then
        fail "expected '$2' in log $1"
    fi
}

assert_log_empty() {
    if [ -s "$1" ]; then
        fail "expected an empty log: $1"
    fi
}

reset_fakes() {
    : > "$FAKE_SYSTEMCTL_LOG"
    : > "$FAKE_CRONTAB_LOG"
    : > "$FAKE_LOGINCTL_LOG"
    : > "$FAKE_VOWP_LOG"
    unset FAKE_SYSTEMCTL_EXIT FAKE_LOGINCTL_EXIT FAKE_VOWP_EXIT
}

run_install() {
    HOME=$HOME_DIR PATH=$TEST_PATH "$SCRIPTS_DIR/install.sh" "$@"
}

run_uninstall() {
    HOME=$HOME_DIR VOWP_HOME=$VOWP_HOME_DIR PATH=$TEST_PATH "$SCRIPTS_DIR/uninstall.sh" "$@"
}

run_watchdog() {
    HOME=$HOME_DIR VOWP_HOME=$VOWP_HOME_DIR PATH=$TEST_PATH "$HOME_DIR/.local/bin/vowp-watchdog.sh" "$@"
}

doctor_check_ok() {
    name=$1
    printf '%s\n' "$DOCTOR_JSON" | awk -v name="$name" '
        index($0, "\"name\": \"" name "\"") > 0 { inside = 1 }
        inside && index($0, "\"status\": \"ok\"") > 0 { found = 1 }
        inside && index($0, "}") > 0 { inside = 0 }
        END { exit found ? 0 : 1 }
    '
}

# --- Templates -------------------------------------------------------------

assert_contains "$SCRIPTS_DIR/vowp.service" 'ExecStart=%h/.local/bin/vowp up --foreground'
assert_contains "$SCRIPTS_DIR/vowp.service" 'Restart=always'
assert_contains "$SCRIPTS_DIR/vowp.service" 'RestartSec=5'
assert_contains "$SCRIPTS_DIR/vowp.service" 'WantedBy=default.target'
assert_contains "$SCRIPTS_DIR/vowp-watchdog.cron" '*/5 * * * *'
assert_contains "$SCRIPTS_DIR/vowp-watchdog.cron" 'vowp-watchdog.sh'
assert_contains "$SCRIPTS_DIR/vowp-watchdog.sh" 'MAX_AGE_SECS=180'
assert_contains "$SCRIPTS_DIR/vowp-watchdog.sh" 'heartbeat'
pass "templates carry the L8 unit, five-minute cron and 180 s heartbeat limit"

# --- install --dry-run -----------------------------------------------------

reset_fakes
install_dry_run=$(run_install --dry-run --bin "$SRC_BIN")
assert_text_contains "$install_dry_run" 'dry-run'
assert_text_contains "$install_dry_run" "$HOME_DIR/.config/systemd/user/vowp.service"
assert_text_contains "$install_dry_run" "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_text_contains "$install_dry_run" 'systemctl --user enable --now vowp.service'
assert_absent "$HOME_DIR/.config/systemd/user/vowp.service"
assert_absent "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_log_empty "$FAKE_CRONTAB_LOG"
pass "install --dry-run prints the plan and changes nothing"

# --- install ---------------------------------------------------------------

reset_fakes
run_install --bin "$SRC_BIN" --linger >/dev/null
assert_file "$HOME_DIR/.local/bin/vowp"
assert_file "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_file "$HOME_DIR/.config/systemd/user/vowp.service"
assert_contains "$HOME_DIR/.config/systemd/user/vowp.service" 'ExecStart=%h/.local/bin/vowp up --foreground'
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'daemon-reload'
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now vowp.service'
assert_log_contains "$FAKE_LOGINCTL_LOG" 'enable-linger'
assert_contains "$FAKE_CRONTAB_FILE" '# vowp: 5-minute L8 watchdog check'
assert_contains "$FAKE_CRONTAB_FILE" '*/5 * * * *'
assert_contains "$FAKE_CRONTAB_FILE" "$HOME_DIR/.local/bin/vowp-watchdog.sh"
pass "install copies the binary, unit and watchdog, enables the unit and adds cron"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
count=$(grep -c -F 'vowp-watchdog.sh' "$FAKE_CRONTAB_FILE")
if [ "$count" -ne 1 ]; then
    fail "install added $count watchdog cron entries, expected 1"
fi
pass "install is idempotent for the crontab entry"

# --- vowp doctor sees the installed files ----------------------------------

mkdir -p "$VOWP_HOME_DIR"
DOCTOR_BIN=${VOWP_BIN:-}
if [ -z "$DOCTOR_BIN" ] && [ -n "${CARGO_TARGET_DIR:-}" ] && [ -x "$CARGO_TARGET_DIR/debug/vowp" ]; then
    DOCTOR_BIN=$CARGO_TARGET_DIR/debug/vowp
fi
if [ -z "$DOCTOR_BIN" ] && [ -x "$ROOT_DIR/target/debug/vowp" ]; then
    DOCTOR_BIN=$ROOT_DIR/target/debug/vowp
fi
if [ -n "$DOCTOR_BIN" ]; then
    if [ ! -x "$DOCTOR_BIN" ]; then
        fail "VOWP_BIN is not executable: $DOCTOR_BIN"
    fi
    if ! DOCTOR_JSON=$(cd "$TMP" && HOME=$HOME_DIR "$DOCTOR_BIN" --home "$VOWP_HOME_DIR" doctor --json 2>/dev/null); then
        :
    fi
    if ! doctor_check_ok systemd; then
        fail "vowp doctor did not report the systemd unit as ok: $DOCTOR_JSON"
    fi
    if ! doctor_check_ok watchdog; then
        fail "vowp doctor did not report the watchdog as ok: $DOCTOR_JSON"
    fi
    pass "vowp doctor reports systemd and watchdog installed"
else
    printf 'skip - set VOWP_BIN (or build target/debug/vowp) to run the doctor check\n'
fi

# --- watchdog --------------------------------------------------------------

mkdir -p "$VOWP_HOME_DIR/logs"
now=$(date +%s)

reset_fakes
printf '%s\n' "$now" > "$VOWP_HOME_DIR/heartbeat"
run_watchdog
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_absent "$VOWP_HOME_DIR/logs/watchdog.log"
pass "watchdog leaves a fresh supervisor alone"

reset_fakes
printf '%s\n' "$((now - 600))" > "$VOWP_HOME_DIR/heartbeat"
run_watchdog
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" 'restarted vowp.service'
pass "watchdog restarts a stale supervisor through systemd and logs it"

reset_fakes
rm -f "$VOWP_HOME_DIR/heartbeat"
run_watchdog
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" 'heartbeat is missing'
pass "watchdog restarts when the heartbeat file is missing"

reset_fakes
printf '%s\n' "$((now - 600))" > "$VOWP_HOME_DIR/heartbeat"
FAKE_SYSTEMCTL_EXIT=1
export FAKE_SYSTEMCTL_EXIT
run_watchdog
unset FAKE_SYSTEMCTL_EXIT
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_log_contains "$FAKE_VOWP_LOG" 'up --detach'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" "started the supervisor with 'vowp up --detach'"
pass "watchdog falls back to 'vowp up --detach' when systemd fails"

reset_fakes
rm -f "$VOWP_HOME_DIR/heartbeat" "$VOWP_HOME_DIR/logs/watchdog.log"
watchdog_dry_run=$(run_watchdog --dry-run)
assert_text_contains "$watchdog_dry_run" 'dry-run'
assert_text_contains "$watchdog_dry_run" 'would restart'
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_absent "$VOWP_HOME_DIR/logs/watchdog.log"
pass "watchdog --dry-run reports the restart without doing it"

# --- uninstall -------------------------------------------------------------

reset_fakes
cat > "$FAKE_CRONTAB_FILE" <<EOF
# user cron
0 3 * * * /usr/bin/backup
# vowp: 5-minute L8 watchdog check (managed by scripts/install.sh)
*/5 * * * * "$HOME_DIR/.local/bin/vowp-watchdog.sh" >/dev/null 2>&1
EOF
mkdir -p "$VOWP_HOME_DIR"
printf 'state\n' > "$VOWP_HOME_DIR/keep.me"
uninstall_dry_run=$(run_uninstall --dry-run)
assert_text_contains "$uninstall_dry_run" 'dry-run'
assert_file "$HOME_DIR/.config/systemd/user/vowp.service"
assert_file "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_contains "$FAKE_CRONTAB_FILE" 'vowp-watchdog.sh'
pass "uninstall --dry-run prints the plan and changes nothing"

reset_fakes
run_uninstall >/dev/null
assert_absent "$HOME_DIR/.config/systemd/user/vowp.service"
assert_absent "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_absent "$HOME_DIR/.local/bin/vowp"
assert_file "$VOWP_HOME_DIR/keep.me"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'disable --now vowp.service'
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'daemon-reload'
assert_contains "$FAKE_CRONTAB_FILE" '/usr/bin/backup'
assert_not_contains "$FAKE_CRONTAB_FILE" 'vowp-watchdog.sh'
pass "uninstall removes the unit, watchdog, binary and cron entry but keeps node state"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
FAKE_SYSTEMCTL_EXIT=1
export FAKE_SYSTEMCTL_EXIT
run_uninstall >/dev/null
unset FAKE_SYSTEMCTL_EXIT
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'disable --now vowp.service'
assert_log_contains "$FAKE_VOWP_LOG" 'down'
pass "uninstall falls back to 'vowp down' when systemctl fails"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
run_uninstall --keep-binary >/dev/null
assert_absent "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_file "$HOME_DIR/.local/bin/vowp"
assert_file "$VOWP_HOME_DIR/keep.me"
pass "uninstall --keep-binary leaves the vowp binary in place"

# --- shellcheck (optional) -------------------------------------------------

if command -v shellcheck >/dev/null 2>&1; then
    shellcheck -s sh \
        "$SCRIPTS_DIR/install.sh" \
        "$SCRIPTS_DIR/uninstall.sh" \
        "$SCRIPTS_DIR/vowp-watchdog.sh" \
        "$TESTS_DIR/run.sh" \
        "$FAKES_DIR/systemctl" \
        "$FAKES_DIR/crontab" \
        "$FAKES_DIR/loginctl" \
        "$FAKES_DIR/vowp"
    pass "shellcheck is clean"
else
    printf 'skip - shellcheck is not installed\n'
fi

printf 'all scripts tests passed\n'
