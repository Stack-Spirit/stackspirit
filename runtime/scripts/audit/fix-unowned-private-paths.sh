#!/bin/sh
# Replace the real home directory in files owned by other jobs.
#
# p01 owns only scripts/audit/**, docs/maintainers/public-audit.md,
# SECURITY.md and .gitignore. The working-tree scan found real home paths in
# six files outside those paths. This script is an OWNER ACTION: p01 does not
# run it with --execute. It is dry-run by default and contains no private
# string itself; the replacements are written as regular expressions.
#
# Usage:
#   scripts/audit/fix-unowned-private-paths.sh                 # dry run
#   scripts/audit/fix-unowned-private-paths.sh --execute       # edit files
#   scripts/audit/fix-unowned-private-paths.sh --root /tmp/x   # test a copy
#
# Only the six files below are considered. They are test fixtures and design
# documents; the replacement is the conventional /home/user placeholder.
# Review `git diff` and commit the result yourself.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2
execute=0
show_diff=0

usage() {
    cat <<'EOF'
Usage: scripts/audit/fix-unowned-private-paths.sh [--execute] [--show-diff] [--root DIR]

  --execute    write the replacements (default: dry run)
  --show-diff  print a unified diff for each changed file
  --root DIR   operate on a copy of the repository (default: this checkout)
  -h, --help   show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --execute)
            execute=1
            shift
            ;;
        --show-diff)
            show_diff=1
            shift
            ;;
        --root)
            [ "$#" -ge 2 ] || { echo "fix-unowned: --root needs a directory" >&2; exit 2; }
            root=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "fix-unowned: unknown option: $1" >&2
            exit 2
            ;;
    esac
done

[ -d "$root" ] || { echo "fix-unowned: not a directory: $root" >&2; exit 2; }

# These are the only files the working-tree scan flagged. Keep the list
# explicit: a new finding should be reviewed, not pattern-matched blindly.
files="
crates/vowp-protocol/tests/domain.rs
crates/vowp-protocol/tests/fixtures/event_log_lines.jsonl
crates/vowp-protocol/tests/fixtures/job_n04_graph.json
crates/vowp-protocol/tests/report.rs
docs/design/adapters.md
docs/design/types.md
"

# /home/<user>/vowp-night -> /home/user/vowp, then the test worktree path
# becomes the conventional /home/user/project. /home/<user>/.vowp -> the
# conventional placeholder as well.
replace_file() {
    file=$1
    tmp=$(mktemp "${TMPDIR:-/tmp}/vowp-fix.XXXXXX") || return 2
    sed -E \
        -e 's#/home/[^/[:space:]]+/vowp-night#/home/user/vowp#g' \
        -e 's#/home/user/vowp/wt/[^"[:space:]]*#/home/user/project#g' \
        -e 's#/home/[^/[:space:]]+/\.vowp#/home/user/.vowp#g' \
        "$file" > "$tmp" || {
        rm -f "$tmp"
        return 2
    }
    if cmp -s "$file" "$tmp"; then
        rm -f "$tmp"
        return 1
    fi
    if [ "$show_diff" -eq 1 ]; then
        diff -u "$file" "$tmp" || true
    fi
    if [ "$execute" -eq 1 ]; then
        cat "$tmp" > "$file" || {
            rm -f "$tmp"
            return 2
        }
    fi
    rm -f "$tmp"
    return 0
}

changed_files=0
changed_occurrences=0

for relative in $files; do
    file="$root/$relative"
    if [ ! -f "$file" ]; then
        echo "fix-unowned: missing file: $relative" >&2
        exit 2
    fi
    # Count matching lines before editing, so the summary is useful in both
    # dry-run and execute modes.
    count=$(grep -cE '/home/[^/[:space:]]+/(vowp-night|\.vowp)' "$file" 2>/dev/null || true)
    replace_file "$file"
    status=$?
    if [ "$status" -eq 2 ]; then
        echo "fix-unowned: could not rewrite $relative" >&2
        exit 2
    fi
    if [ "$status" -eq 0 ]; then
        changed_files=$((changed_files + 1))
        changed_occurrences=$((changed_occurrences + count))
        if [ "$execute" -eq 1 ]; then
            echo "fixed: $relative ($count line(s))"
        else
            echo "would fix: $relative ($count line(s))"
        fi
    else
        echo "clean: $relative"
    fi
done

if [ "$execute" -eq 1 ]; then
    echo "fix-unowned: changed $changed_occurrences line(s) in $changed_files file(s)."
    echo "fix-unowned: review with 'git diff' and commit; p01 did not commit these paths."
else
    echo "fix-unowned: dry run: $changed_occurrences line(s) in $changed_files file(s) would change."
    echo "fix-unowned: re-run with --execute to apply, then review and commit."
fi
