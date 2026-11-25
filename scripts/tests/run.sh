#!/bin/sh
# Tests for the VOWP ops scripts. Hermetic: a temporary HOME, fake systemctl,
# crontab, loginctl, vowp, claude and opencode on PATH, a fake date for the
# watchdog boundary, and a temporary VOWP_HOME.
#
# Run: sh scripts/tests/run.sh
# Set VOWP_BIN=/path/to/vowp to run the doctor check; otherwise a built
# target/debug/vowp is used when present.

set -eu

TESTS_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
SCRIPTS_DIR=$(CDPATH='' cd -- "$TESTS_DIR/.." && pwd)
ROOT_DIR=$(CDPATH='' cd -- "$SCRIPTS_DIR/.." && pwd)
FAKES_DIR=$TESTS_DIR/fakes

TMP=$(mktemp -d "${TMPDIR:-/tmp}/vowp-scripts-test.XXXXXX")
trap 'rm -rf "$TMP"' 0 1 2 15

ORIGINAL_PATH=$PATH
FAKEBIN=$TMP/fakebin
FAKEDATE=$TMP/fakedate
HOME_DIR=$TMP/home
VOWP_HOME_DIR=$HOME_DIR/.vowp
SRC_BIN=$TMP/src/vowp

mkdir -p "$FAKEBIN" "$FAKEDATE" "$TMP/logs" "$HOME_DIR" "$TMP/src"
for fake in systemctl crontab loginctl vowp claude opencode; do
    cp "$FAKES_DIR/$fake" "$FAKEBIN/$fake"
    chmod 0755 "$FAKEBIN/$fake"
done
# The fake curl is written here (not kept under fakes/) so it stays with this runner.
cat > "$FAKEBIN/curl" <<'FAKE_CURL_EOF'
#!/bin/sh
# Fake `curl` for scripts/tests/run.sh. Never touches the network: a download
# of https://host/path/<name> is served from $FAKE_CURL_DIR/<name>, and a name
# that is not there fails like a 404. Every URL is logged in $FAKE_CURL_LOG.

set -eu

out=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            out=$2
            shift 2
            ;;
        --retry|--connect-timeout|--proto|--proto-redir|--max-time|-w)
            shift 2
            ;;
        -*)
            shift
            ;;
        *)
            url=$1
            shift
            ;;
    esac
done

printf '%s\n' "$url" >> "${FAKE_CURL_LOG:?FAKE_CURL_LOG is not set}"
file=${FAKE_CURL_DIR:?FAKE_CURL_DIR is not set}/${url##*/}
if [ ! -f "$file" ]; then
    printf 'curl: (22) The requested URL returned error: 404\n' >&2
    exit 22
fi
cp "$file" "${out:?fake curl needs -o}"
FAKE_CURL_EOF
chmod 0755 "$FAKEBIN/curl"
cp "$FAKES_DIR/date" "$FAKEDATE/date"
chmod 0755 "$FAKEDATE/date"
cat > "$SRC_BIN" <<'EOF'
#!/bin/sh
printf 'installed-vowp %s\n' "$*" >> "${FAKE_INSTALLED_VOWP_LOG:-/dev/null}"
exit 0
EOF
chmod 0755 "$SRC_BIN"

export FAKE_SYSTEMCTL_LOG="$TMP/logs/systemctl.log"
export FAKE_CRONTAB_LOG="$TMP/logs/crontab.log"
export FAKE_LOGINCTL_LOG="$TMP/logs/loginctl.log"
export FAKE_VOWP_LOG="$TMP/logs/vowp.log"
export FAKE_INSTALLED_VOWP_LOG="$TMP/logs/installed-vowp.log"
export FAKE_CRONTAB_FILE="$TMP/crontab.txt"
export FAKE_CURL_LOG="$TMP/logs/curl.log"
export FAKE_CURL_DIR="$TMP/curl-files"
mkdir -p "$FAKE_CURL_DIR"
: > "$FAKE_CURL_LOG"
: > "$FAKE_SYSTEMCTL_LOG"
: > "$FAKE_CRONTAB_LOG"
: > "$FAKE_LOGINCTL_LOG"
: > "$FAKE_VOWP_LOG"
: > "$FAKE_INSTALLED_VOWP_LOG"

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

# A dry run may read the crontab (`crontab -l`) to find an existing watchdog
# entry, but must never write it.
assert_cron_not_written() {
    if [ -f "$FAKE_CRONTAB_LOG" ] && grep -v -x 'crontab -l' "$FAKE_CRONTAB_LOG" | grep . >/dev/null 2>&1; then
        fail "crontab was written: $(cat "$FAKE_CRONTAB_LOG")"
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
    : > "$FAKE_INSTALLED_VOWP_LOG"
    unset FAKE_SYSTEMCTL_EXIT FAKE_LOGINCTL_EXIT FAKE_VOWP_EXIT
    unset FAKE_CRONTAB_FAIL
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

run_watchdog_at() {
    fake_now=$1
    shift
    HOME=$HOME_DIR VOWP_HOME=$VOWP_HOME_DIR FAKE_NOW=$fake_now PATH="$FAKEDATE:$TEST_PATH" \
        "$HOME_DIR/.local/bin/vowp-watchdog.sh" "$@"
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
assert_contains "$SCRIPTS_DIR/vowp.service" 'Restart=on-failure'
assert_not_contains "$SCRIPTS_DIR/vowp.service" 'Restart=always'
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
assert_cron_not_written
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
run_install --bin "$SRC_BIN" --no-service --linger >/dev/null
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_log_contains "$FAKE_LOGINCTL_LOG" 'enable-linger'
pass "install --no-service --linger still enables linger"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
count=$(grep -c -F 'vowp-watchdog.sh' "$FAKE_CRONTAB_FILE")
if [ "$count" -ne 1 ]; then
    fail "install added $count watchdog cron entries, expected 1"
fi
pass "install is idempotent for the crontab entry"

reset_fakes
printf '0 3 * * * /usr/bin/backup\n' > "$FAKE_CRONTAB_FILE"
FAKE_CRONTAB_FAIL=permission
export FAKE_CRONTAB_FAIL
install_cron_fail=$(run_install --bin "$SRC_BIN" 2>&1)
unset FAKE_CRONTAB_FAIL
assert_text_contains "$install_cron_fail" 'could not read the crontab'
assert_contains "$FAKE_CRONTAB_FILE" '/usr/bin/backup'
assert_not_contains "$FAKE_CRONTAB_FILE" 'vowp'
pass "install leaves the crontab alone when it cannot read it"

# --- vowp doctor sees the installed files ----------------------------------

mkdir -p "$VOWP_HOME_DIR" "$TMP/xdg/config" "$TMP/xdg/data" "$TMP/xdg/cache" "$TMP/xdg/runtime"
DOCTOR_BIN=${VOWP_BIN:-}
if [ -z "$DOCTOR_BIN" ] && [ -n "${CARGO_TARGET_DIR:-}" ] && [ -x "$CARGO_TARGET_DIR/debug/vowp" ]; then
    DOCTOR_BIN=$CARGO_TARGET_DIR/debug/vowp
fi
if [ -z "$DOCTOR_BIN" ] && [ -x "$ROOT_DIR/target/debug/vowp" ]; then
    DOCTOR_BIN=$ROOT_DIR/target/debug/vowp
fi
DOCTOR_RAN=0
if [ -n "$DOCTOR_BIN" ]; then
    if [ ! -x "$DOCTOR_BIN" ]; then
        fail "VOWP_BIN is not executable: $DOCTOR_BIN"
    fi
    # Doctor may probe harnesses; PATH contains only the fakes so a real
    # claude/opencode (or git/tmux/df) can never be executed by this test.
    if ! DOCTOR_JSON=$(
        cd "$TMP" &&
            HOME=$HOME_DIR \
            PATH=$FAKEBIN \
            XDG_CONFIG_HOME=$TMP/xdg/config \
            XDG_DATA_HOME=$TMP/xdg/data \
            XDG_CACHE_HOME=$TMP/xdg/cache \
            XDG_RUNTIME_DIR=$TMP/xdg/runtime \
            "$DOCTOR_BIN" --home "$VOWP_HOME_DIR" doctor --json 2>/dev/null
    ); then
        :
    fi
    if ! doctor_check_ok systemd; then
        fail "vowp doctor did not report the systemd unit as ok: $DOCTOR_JSON"
    fi
    if ! doctor_check_ok watchdog; then
        fail "vowp doctor did not report the watchdog as ok: $DOCTOR_JSON"
    fi
    if ! doctor_check_ok harness:claude-code; then
        fail "vowp doctor did not probe the fake claude harness: $DOCTOR_JSON"
    fi
    if ! doctor_check_ok harness:opencode; then
        fail "vowp doctor did not probe the fake opencode harness: $DOCTOR_JSON"
    fi
    DOCTOR_RAN=1
    pass "vowp doctor reports systemd and watchdog installed and probes only fake harnesses"
else
    printf 'skip - doctor acceptance not verified: build target/debug/vowp or set VOWP_BIN\n'
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

fake_now=1000000000
reset_fakes
rm -f "$VOWP_HOME_DIR/logs/watchdog.log"
printf '%s\n' "$((fake_now - 180))" > "$VOWP_HOME_DIR/heartbeat"
run_watchdog_at "$fake_now"
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_absent "$VOWP_HOME_DIR/logs/watchdog.log"
pass "watchdog treats a heartbeat exactly 180 s old as fresh"

reset_fakes
rm -f "$VOWP_HOME_DIR/logs/watchdog.log"
printf '%s\n' "$((fake_now - 181))" > "$VOWP_HOME_DIR/heartbeat"
run_watchdog_at "$fake_now"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" '181s old'
pass "watchdog restarts a heartbeat 181 s old"

reset_fakes
rm -f "$VOWP_HOME_DIR/logs/watchdog.log"
printf '08\n' > "$VOWP_HOME_DIR/heartbeat"
run_watchdog_at "$fake_now"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" 'restarted vowp.service'
pass "watchdog normalizes a leading-zero heartbeat instead of crashing"

reset_fakes
rm -f "$VOWP_HOME_DIR/logs/watchdog.log"
printf 'yesterday\n' > "$VOWP_HOME_DIR/heartbeat"
run_watchdog_at "$fake_now"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$VOWP_HOME_DIR/logs/watchdog.log" 'heartbeat is not a unix timestamp'
pass "watchdog restarts on non-numeric heartbeat text"

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
0 4 * * * echo "keep /home/user/.local/bin/vowp-watchdog.sh"
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
assert_contains "$FAKE_CRONTAB_FILE" 'keep /home/user/.local/bin/vowp-watchdog.sh'
assert_not_contains "$FAKE_CRONTAB_FILE" '# vowp: 5-minute L8 watchdog check'
assert_not_contains "$FAKE_CRONTAB_FILE" "*/5 * * * * \"$HOME_DIR/.local/bin/vowp-watchdog.sh\""
pass "uninstall removes only the managed cron entry and keeps node state"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
mv "$FAKEBIN/vowp" "$FAKEBIN/vowp.hidden"
FAKE_SYSTEMCTL_EXIT=1
export FAKE_SYSTEMCTL_EXIT
run_uninstall >/dev/null
unset FAKE_SYSTEMCTL_EXIT
mv "$FAKEBIN/vowp.hidden" "$FAKEBIN/vowp"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'disable --now vowp.service'
assert_log_contains "$FAKE_INSTALLED_VOWP_LOG" "--home $VOWP_HOME_DIR down"
assert_not_contains "$FAKE_VOWP_LOG" 'down'
pass "uninstall uses the installed binary for 'vowp down' when systemctl fails"

reset_fakes
printf '0 3 * * * /usr/bin/backup\n' > "$FAKE_CRONTAB_FILE"
FAKE_CRONTAB_FAIL=permission
export FAKE_CRONTAB_FAIL
uninstall_cron_fail=$(run_uninstall 2>&1)
unset FAKE_CRONTAB_FAIL
assert_text_contains "$uninstall_cron_fail" 'could not read the crontab'
assert_contains "$FAKE_CRONTAB_FILE" '/usr/bin/backup'
pass "uninstall leaves the crontab alone when it cannot read it"

reset_fakes
run_install --bin "$SRC_BIN" >/dev/null
run_uninstall --keep-binary >/dev/null
assert_absent "$HOME_DIR/.local/bin/vowp-watchdog.sh"
assert_file "$HOME_DIR/.local/bin/vowp"
assert_file "$VOWP_HOME_DIR/keep.me"
pass "uninstall --keep-binary leaves the vowp binary in place"

# --- StackSpirit product mode ----------------------------------------------
#
# Fresh and legacy hosts use their own temporary HOME so the checks above are
# not disturbed. Every binary is a tiny fake script; nothing real is run.

make_fake_cli() {
    # make_fake_cli PATH VERSION: prints "<its own name> VERSION" for --version.
    cat > "$1" <<EOF2
#!/bin/sh
printf '%s %s\\n' "\$(basename "\$0")" "$2"
EOF2
    chmod 0755 "$1"
}

SS_SRC=$TMP/ss-src
mkdir -p "$SS_SRC"
make_fake_cli "$SS_SRC/stackspirit" 1.2.3
make_fake_cli "$SS_SRC/stackspirit-daemon" 1.2.3

run_install_in() {
    install_home=$1
    shift
    HOME=$install_home PATH=$TEST_PATH "$SCRIPTS_DIR/install.sh" "$@"
}

run_uninstall_in() {
    uninstall_home=$1
    shift
    HOME=$uninstall_home PATH=$TEST_PATH "$SCRIPTS_DIR/uninstall.sh" "$@"
}

count_lines() {
    # count_lines FILE PATTERN: number of fixed-string matches, 0 when none.
    grep -c -F -- "$2" "$1" 2>/dev/null || true
}

assert_contains "$SCRIPTS_DIR/stackspirit.service" 'ExecStart=%h/.local/bin/stackspirit up --foreground'
assert_contains "$SCRIPTS_DIR/stackspirit.service" 'Environment=STACKSPIRIT_HOME=%h/.vowp'
assert_contains "$SCRIPTS_DIR/stackspirit.service" 'Environment=VOWP_HOME=%h/.vowp'
assert_contains "$SCRIPTS_DIR/vowp.service" 'Environment=STACKSPIRIT_HOME=%h/.vowp'
assert_contains "$SCRIPTS_DIR/vowp.service" 'Environment=VOWP_HOME=%h/.vowp'
pass "both unit templates carry both home variables"

# Fresh host, dry run: the plan names every StackSpirit and legacy piece.
FRESH=$TMP/fresh
mkdir -p "$FRESH"
reset_fakes
rm -f "$FAKE_CRONTAB_FILE"
ss_dry=$(run_install_in "$FRESH" --product stackspirit --dry-run --bin "$SS_SRC/stackspirit")
assert_text_contains "$ss_dry" 'stackspirit-daemon'
assert_text_contains "$ss_dry" "$FRESH/.local/bin/vowp -> stackspirit"
assert_text_contains "$ss_dry" "$FRESH/.local/bin/vowp-daemon -> stackspirit-daemon"
assert_text_contains "$ss_dry" "$FRESH/.config/systemd/user/stackspirit.service"
assert_text_contains "$ss_dry" 'enable --now stackspirit.service'
assert_text_contains "$ss_dry" "$FRESH/.local/bin/stackspirit-watchdog.sh"
assert_text_contains "$ss_dry" 'STACKSPIRIT_HOME and VOWP_HOME'
assert_absent "$FRESH/.local/bin"
assert_absent "$FRESH/.config"
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
assert_cron_not_written
pass "stackspirit dry run on a fresh HOME names both CLIs, both daemons, the unit and both env names"

# Fresh host, real install.
reset_fakes
rm -f "$FAKE_CRONTAB_FILE"
run_install_in "$FRESH" --product stackspirit --bin "$SS_SRC/stackspirit" >/dev/null
for name in stackspirit stackspirit-daemon; do
    assert_file "$FRESH/.local/bin/$name"
done
for pair in vowp:stackspirit vowp-daemon:stackspirit-daemon vowp-watchdog.sh:stackspirit-watchdog.sh; do
    link=${pair%%:*}
    want=${pair#*:}
    if [ ! -L "$FRESH/.local/bin/$link" ] || [ "$(readlink "$FRESH/.local/bin/$link")" != "$want" ]; then
        fail "$link is not a link to $want"
    fi
done
assert_file "$FRESH/.local/bin/stackspirit-watchdog.sh"
assert_file "$FRESH/.config/systemd/user/stackspirit.service"
assert_absent "$FRESH/.config/systemd/user/vowp.service"
assert_contains "$FRESH/.config/systemd/user/stackspirit.service" 'ExecStart=%h/.local/bin/stackspirit up --foreground'
assert_contains "$FRESH/.config/systemd/user/stackspirit.service" 'VOWP_HOME'
assert_contains "$FRESH/.config/systemd/user/stackspirit.service" 'STACKSPIRIT_HOME'
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now stackspirit.service'
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'vowp.service'
assert_contains "$FAKE_CRONTAB_FILE" '# stackspirit: 5-minute L8 watchdog check'
assert_contains "$FAKE_CRONTAB_FILE" "$FRESH/.local/bin/stackspirit-watchdog.sh"
if [ "$(count_lines "$FAKE_CRONTAB_FILE" 'watchdog.sh')" -ne 1 ]; then
    fail "a fresh install must add exactly one watchdog cron line"
fi
pass "stackspirit fresh install creates both CLI names, both daemon names, stackspirit.service and one cron line"

# Re-running on that host stays one of everything (no duplicate unit or cron).
reset_fakes
run_install_in "$FRESH" --product stackspirit --bin "$SS_SRC/stackspirit" >/dev/null
assert_absent "$FRESH/.config/systemd/user/vowp.service"
assert_absent "$FRESH/.local/bin/vowp-watchdog.sh.new"
if [ "$(count_lines "$FAKE_CRONTAB_FILE" 'watchdog.sh')" -ne 1 ]; then
    fail "re-running the installer duplicated the cron line"
fi
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now stackspirit.service'
pass "stackspirit install is idempotent"

# The watchdog on a stackspirit-only host restarts stackspirit.service.
mkdir -p "$FRESH/.vowp"
reset_fakes
HOME=$FRESH PATH=$TEST_PATH "$FRESH/.local/bin/stackspirit-watchdog.sh" >/dev/null
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'restart stackspirit.service'
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'restart vowp.service'
assert_contains "$FRESH/.vowp/logs/watchdog.log" 'restarted stackspirit.service'
pass "watchdog restarts stackspirit.service on a fresh StackSpirit host"

# Legacy host: vowp.service, a real vowp-watchdog.sh and its cron line exist.
LEGACY=$TMP/legacy
mkdir -p "$LEGACY/.local/bin" "$LEGACY/.config/systemd/user"
cp "$SCRIPTS_DIR/vowp.service" "$LEGACY/.config/systemd/user/vowp.service"
printf '#!/bin/sh\n# VOWP L8 watchdog (old copy)\n' > "$LEGACY/.local/bin/vowp-watchdog.sh"
chmod 0755 "$LEGACY/.local/bin/vowp-watchdog.sh"
make_fake_cli "$LEGACY/.local/bin/vowp" 0.0.1
cat > "$FAKE_CRONTAB_FILE" <<EOF
0 3 * * * /usr/bin/backup
# vowp: 5-minute L8 watchdog check (managed by scripts/install.sh)
*/5 * * * * "$LEGACY/.local/bin/vowp-watchdog.sh" >/dev/null 2>&1
EOF
reset_fakes_keep_cron() {
    : > "$FAKE_SYSTEMCTL_LOG"
    : > "$FAKE_CRONTAB_LOG"
}
reset_fakes_keep_cron
legacy_dry=$(run_install_in "$LEGACY" --product stackspirit --dry-run --bin "$SS_SRC/stackspirit")
assert_text_contains "$legacy_dry" "$LEGACY/.config/systemd/user/vowp.service (in place; no second unit)"
assert_text_contains "$legacy_dry" 'enable --now vowp.service'
assert_text_contains "$legacy_dry" "$LEGACY/.local/bin/vowp-watchdog.sh"
if printf '%s\n' "$legacy_dry" | grep -F 'stackspirit.service' >/dev/null; then
    fail "legacy dry run mentions stackspirit.service"
fi
if printf '%s\n' "$legacy_dry" | grep -F 'stackspirit-watchdog.sh' >/dev/null; then
    fail "legacy dry run mentions a second watchdog"
fi
pass "stackspirit dry run on a legacy host updates vowp.service and the old watchdog in place"

reset_fakes_keep_cron
run_install_in "$LEGACY" --product stackspirit --bin "$SS_SRC/stackspirit" >/dev/null
assert_file "$LEGACY/.config/systemd/user/vowp.service"
assert_absent "$LEGACY/.config/systemd/user/stackspirit.service"
assert_absent "$LEGACY/.local/bin/stackspirit-watchdog.sh"
assert_file "$LEGACY/.local/bin/vowp-watchdog.sh"
assert_contains "$LEGACY/.local/bin/vowp-watchdog.sh" 'MAX_AGE_SECS=180'
assert_file "$LEGACY/.local/bin/stackspirit"
assert_file "$LEGACY/.local/bin/stackspirit-daemon"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now vowp.service'
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'stackspirit.service'
if [ "$(count_lines "$FAKE_CRONTAB_FILE" 'watchdog.sh"')" -ne 1 ]; then
    fail "the legacy host must keep exactly one watchdog cron line"
fi
assert_contains "$FAKE_CRONTAB_FILE" "$LEGACY/.local/bin/vowp-watchdog.sh"
assert_not_contains "$FAKE_CRONTAB_FILE" 'stackspirit-watchdog.sh'
assert_contains "$FAKE_CRONTAB_FILE" '/usr/bin/backup'
pass "stackspirit install on a legacy host keeps vowp.service, one cron line and one watchdog"

# Both unit files: refuse, then rename only on request.
BOTH=$TMP/both
mkdir -p "$BOTH/.config/systemd/user"
cp "$SCRIPTS_DIR/vowp.service" "$BOTH/.config/systemd/user/vowp.service"
cp "$SCRIPTS_DIR/stackspirit.service" "$BOTH/.config/systemd/user/stackspirit.service"
reset_fakes
if both_out=$(run_install_in "$BOTH" --product stackspirit --bin "$SS_SRC/stackspirit" 2>&1); then
    fail "install succeeded on a host with both unit files"
fi
assert_text_contains "$both_out" '--rename-unit'
assert_absent "$BOTH/.local/bin/stackspirit"
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now'
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'disable'
assert_file "$BOTH/.config/systemd/user/vowp.service"
reset_fakes
rm -f "$FAKE_CRONTAB_FILE"
run_install_in "$BOTH" --product stackspirit --rename-unit --bin "$SS_SRC/stackspirit" >/dev/null
assert_absent "$BOTH/.config/systemd/user/vowp.service"
assert_file "$BOTH/.config/systemd/user/stackspirit.service"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'disable --now vowp.service'
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'enable --now stackspirit.service'
pass "a host with both units is refused until --rename-unit is explicit"

# The public default is unchanged: it installs vowp only.
PUBLIC=$TMP/public
mkdir -p "$PUBLIC"
reset_fakes
rm -f "$FAKE_CRONTAB_FILE"
run_install_in "$PUBLIC" --bin "$SRC_BIN" >/dev/null
assert_file "$PUBLIC/.local/bin/vowp"
assert_file "$PUBLIC/.config/systemd/user/vowp.service"
assert_absent "$PUBLIC/.local/bin/stackspirit"
assert_absent "$PUBLIC/.local/bin/stackspirit-daemon"
assert_absent "$PUBLIC/.config/systemd/user/stackspirit.service"
assert_absent "$PUBLIC/.local/bin/stackspirit-watchdog.sh"
pass "the default (public) install still installs only vowp"

# Uninstall a fresh StackSpirit install: generated files go, state stays.
mkdir -p "$FRESH/.vowp"
printf 'state\n' > "$FRESH/.vowp/vowp.db"
reset_fakes
uninstall_ss_dry=$(run_uninstall_in "$FRESH" --dry-run)
assert_text_contains "$uninstall_ss_dry" 'disable --now stackspirit.service'
assert_text_contains "$uninstall_ss_dry" "would remove $FRESH/.local/bin/stackspirit-daemon"
assert_file "$FRESH/.config/systemd/user/stackspirit.service"
assert_log_empty "$FAKE_SYSTEMCTL_LOG"
run_uninstall_in "$FRESH" >/dev/null
for gone in stackspirit stackspirit-daemon vowp vowp-daemon stackspirit-watchdog.sh vowp-watchdog.sh; do
    if [ -e "$FRESH/.local/bin/$gone" ] || [ -L "$FRESH/.local/bin/$gone" ]; then
        fail "uninstall left $gone behind"
    fi
done
assert_absent "$FRESH/.config/systemd/user/stackspirit.service"
assert_file "$FRESH/.vowp/vowp.db"
assert_log_contains "$FAKE_SYSTEMCTL_LOG" 'disable --now stackspirit.service'
assert_not_contains "$FAKE_CRONTAB_FILE" 'watchdog'
pass "uninstall removes a StackSpirit install (both names), keeps node state, and its dry run matches"

# Uninstall refuses a foreign unit and a foreign link, whatever the name.
FOREIGN=$TMP/foreign
mkdir -p "$FOREIGN/.config/systemd/user" "$FOREIGN/.local/bin" "$FOREIGN/elsewhere"
cat > "$FOREIGN/.config/systemd/user/stackspirit.service" <<'EOF'
[Unit]
Description=Somebody else's service

[Service]
ExecStart=/opt/other/bin/run --serve
EOF
printf 'other\n' > "$FOREIGN/elsewhere/tool"
ln -s ../elsewhere/tool "$FOREIGN/.local/bin/vowp-daemon"
reset_fakes
foreign_out=$(run_uninstall_in "$FOREIGN" 2>&1)
assert_file "$FOREIGN/.config/systemd/user/stackspirit.service"
assert_text_contains "$foreign_out" 'was not generated by scripts/install.sh'
assert_not_contains "$FAKE_SYSTEMCTL_LOG" 'disable'
if [ ! -L "$FOREIGN/.local/bin/vowp-daemon" ]; then
    fail "uninstall removed a link to a foreign target"
fi
pass "uninstall refuses to delete a foreign unit or a link to a foreign target"

# --- StackSpirit release archive and download ------------------------------

ARCH=$TMP/archive
mkdir -p "$ARCH/bin"
for name in stackspirit vowp stackspirit-daemon vowp-daemon; do
    make_fake_cli "$ARCH/bin/$name" 1.2.3
done
ss_archive() {
    # ss_archive DIST [extra package-archive args]: build the private archive.
    dist=$1
    shift
    bash "$SCRIPTS_DIR/release/package-archive.sh" --product stackspirit --version 1.2.3 \
        --target x86_64-unknown-linux-gnu --dist "$dist" \
        --stackspirit "$ARCH/bin/stackspirit" --vowp "$ARCH/bin/vowp" \
        --stackspirit-daemon "$ARCH/bin/stackspirit-daemon" --vowp-daemon "$ARCH/bin/vowp-daemon" "$@"
}
ss_listing=$(ss_archive "$ARCH/dist")
for member in stackspirit vowp stackspirit-daemon vowp-daemon LICENSE README.md THIRD-PARTY-LICENSES.md; do
    assert_text_contains "$ss_listing" "stackspirit-1.2.3-x86_64-unknown-linux-gnu/$member"
done
assert_file "$ARCH/dist/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
assert_file "$ARCH/dist/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz.sha256"
pass "package-archive builds stackspirit-<version>-<target>.tar.gz with both CLIs and both daemons"

# A missing or empty daemon or CLI is rejected and no archive is left.
: > "$ARCH/empty"
for missing in vowp-daemon stackspirit-daemon vowp stackspirit; do
    rm -rf "$ARCH/bad"
    case $missing in
        stackspirit) args="--stackspirit $ARCH/nonexistent" ;;
        vowp) args="--vowp $ARCH/nonexistent" ;;
        stackspirit-daemon) args="--stackspirit-daemon $ARCH/nonexistent" ;;
        vowp-daemon) args="--vowp-daemon $ARCH/empty" ;;
    esac
    # shellcheck disable=SC2086
    if ss_archive "$ARCH/bad" $args >/dev/null 2>&1; then
        fail "package-archive accepted an archive without $missing"
    fi
    assert_absent "$ARCH/bad/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
done
pass "package-archive rejects a stackspirit archive missing either CLI or either daemon"

# The public archive is still vowp-<version>-<target>.tar.gz with the vowp binary.
pub_listing=$(bash "$SCRIPTS_DIR/release/package-archive.sh" --version 1.2.3 \
    --target x86_64-unknown-linux-gnu --binary "$ARCH/bin/vowp" --dist "$ARCH/pubdist")
assert_text_contains "$pub_listing" 'vowp-1.2.3-x86_64-unknown-linux-gnu/vowp'
assert_file "$ARCH/pubdist/vowp-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
if printf '%s\n' "$pub_listing" | grep -F 'stackspirit' >/dev/null; then
    fail "the public archive contains a StackSpirit file"
fi
pass "the public archive name and contents are unchanged"

# Download install through the fake curl: verified, extracted, both versions run.
cp "$ARCH/dist/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz" "$ARCH/dist/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz.sha256" "$FAKE_CURL_DIR/"
DL=$TMP/download-host
mkdir -p "$DL"
reset_fakes
rm -f "$FAKE_CRONTAB_FILE"
: > "$FAKE_CURL_LOG"
FAKE_UNAME_OK=1
dl_target=$(uname -s):$(uname -m)
case $dl_target in
    Linux:x86_64|Linux:amd64) ;;
    *) FAKE_UNAME_OK=0 ;;
esac
if [ "$FAKE_UNAME_OK" -eq 1 ]; then
    dl_out=$(run_install_in "$DL" --product stackspirit --version 1.2.3 --base-url https://releases.example.invalid/dl 2>&1)
    assert_log_contains "$FAKE_CURL_LOG" 'https://releases.example.invalid/dl/v1.2.3/stackspirit-1.2.3-x86_64-unknown-linux-gnu.tar.gz'
    assert_text_contains "$dl_out" 'installed: stackspirit 1.2.3'
    assert_file "$DL/.local/bin/stackspirit-daemon"
    assert_file "$DL/.config/systemd/user/stackspirit.service"
    dl_version=$("$DL/.local/bin/vowp" --version)
    if [ "$dl_version" != 'vowp 1.2.3' ]; then
        fail "vowp --version after a download install printed '$dl_version'"
    fi
    pass "stackspirit download install verifies the archive and both stackspirit and vowp answer --version"

    # An archive that lacks a daemon is rejected by the installer too.
    mkdir -p "$ARCH/incomplete/stackspirit-1.2.4-x86_64-unknown-linux-gnu"
    for name in stackspirit vowp stackspirit-daemon; do
        cp "$ARCH/bin/$name" "$ARCH/incomplete/stackspirit-1.2.4-x86_64-unknown-linux-gnu/$name"
    done
    tar -C "$ARCH/incomplete" -czf "$FAKE_CURL_DIR/stackspirit-1.2.4-x86_64-unknown-linux-gnu.tar.gz" stackspirit-1.2.4-x86_64-unknown-linux-gnu
    (cd "$FAKE_CURL_DIR" && if command -v sha256sum >/dev/null 2>&1; then sha256sum stackspirit-1.2.4-x86_64-unknown-linux-gnu.tar.gz; else shasum -a 256 stackspirit-1.2.4-x86_64-unknown-linux-gnu.tar.gz; fi > stackspirit-1.2.4-x86_64-unknown-linux-gnu.tar.gz.sha256)
    DL2=$TMP/download-host2
    mkdir -p "$DL2"
    if dl_bad=$(run_install_in "$DL2" --product stackspirit --version 1.2.4 --base-url https://releases.example.invalid/dl 2>&1); then
        fail "installer accepted an archive without vowp-daemon"
    fi
    assert_text_contains "$dl_bad" 'does not contain'
    assert_absent "$DL2/.local/bin/stackspirit"
    pass "stackspirit download install rejects an archive without a daemon"
else
    printf 'skip - download install test needs an x86_64 Linux host (uname: %s)\n' "$dl_target"
fi

if run_install_in "$DL" --product stackspirit --version 1.2.3 >/dev/null 2>&1; then
    fail "stackspirit download without --base-url should be refused"
fi
pass "stackspirit never falls back to the public release URL"

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
        "$FAKES_DIR/vowp" \
        "$FAKES_DIR/claude" \
        "$FAKES_DIR/opencode" \
        "$FAKES_DIR/date" \
        "$FAKES_DIR/curl"
    pass "shellcheck is clean"
else
    printf 'skip - shellcheck is not installed\n'
fi

if [ "$DOCTOR_RAN" -eq 1 ]; then
    printf 'all scripts tests passed\n'
else
    printf 'scripts tests passed; doctor acceptance was skipped\n'
fi
