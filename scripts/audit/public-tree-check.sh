#!/bin/sh
# Check a git tree, the index or the working tree against the public-tree
# manifest in scripts/audit/public-tree.txt.
#
# Usage:
#   scripts/audit/public-tree-check.sh --worktree
#   scripts/audit/public-tree-check.sh --index
#   scripts/audit/public-tree-check.sh --rev <commit>
#   scripts/audit/public-tree-check.sh --index --manifest <file>
#
# --worktree and --index read tracked paths with `git ls-files`; the index mode
# honours GIT_INDEX_FILE, which lets squash-for-public.sh check the staged tree
# before it creates the public commit. --rev reads one commit's tree.
#
# Exit status:
#   0  every required path exists, no pruned path is present and every
#      top-level entry is allowed
#   1  a path violation
#   2  usage error, not a git repository, or an invalid manifest
#
# No network access. POSIX shell plus git and awk.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2
cd "$root" || { echo "public-tree: cannot enter repository root" >&2; exit 2; }

manifest=$script_dir/public-tree.txt
mode=
rev=

usage() {
    cat <<'EOF'
Usage: scripts/audit/public-tree-check.sh --worktree|--index|--rev <commit> [options]

Scopes:
  --worktree        check the tracked paths in the working tree
  --index           check the staged index (honours GIT_INDEX_FILE)
  --rev <commit>    check one commit's tree

Options:
  --manifest <file> use another manifest (default: scripts/audit/public-tree.txt)
  -h, --help        show this help

The manifest has `allow`, `prune` and `require` lines. A trailing slash means a
directory and everything below it. Exit 1 on a path violation, 2 on a usage or
manifest error.
EOF
}

die() {
    echo "public-tree: $*" >&2
    exit 2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --worktree)
            mode=worktree
            shift
            ;;
        --index)
            mode=index
            shift
            ;;
        --rev)
            [ "$#" -ge 2 ] || die "--rev needs a commit"
            rev=$2
            mode=rev
            shift 2
            ;;
        --manifest)
            [ "$#" -ge 2 ] || die "--manifest needs a file"
            manifest=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1 (try --help)"
            ;;
    esac
done

[ -n "$mode" ] || die "choose --worktree, --index or --rev <commit> (try --help)"
[ -f "$manifest" ] || die "manifest not found: $manifest"
[ -r "$manifest" ] || die "manifest is not readable: $manifest"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git working tree: $root"

if [ "$mode" = rev ]; then
    rev=$(git rev-parse --verify "$rev^{commit}" 2>/dev/null) || die "not a commit: $rev"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-public-tree.XXXXXX") || die "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

: > "$tmp/allow"
: > "$tmp/prune"
: > "$tmp/require"
lineno=0
while IFS= read -r line || [ -n "$line" ]; do
    lineno=$((lineno + 1))
    case "$line" in
        ''|'#'*)
            continue
            ;;
    esac
    set -- $line
    [ "$#" -eq 2 ] || die "manifest line $lineno must be: allow|prune|require <path>"
    kind=$1
    path=$2
    case "$kind" in
        allow|prune|require)
            ;;
        *)
            die "manifest line $lineno has unknown kind: $kind"
            ;;
    esac
    case "$path" in
        /*)
            die "manifest line $lineno must use a repository-relative path: $path"
            ;;
        *' '*)
            die "manifest line $lineno must not contain spaces: $path"
            ;;
    esac
    path=${path%/}
    [ -n "$path" ] || die "manifest line $lineno has an empty path"
    printf '%s\n' "$path" >> "$tmp/$kind"
done < "$manifest"

[ -s "$tmp/allow" ] || die "manifest has no allow entries"
[ -s "$tmp/prune" ] || die "manifest has no prune entries"
[ -s "$tmp/require" ] || die "manifest has no require entries"

case "$mode" in
    worktree|index)
        git ls-files > "$tmp/paths" 2>/dev/null || die "cannot list tracked paths"
        label=$mode
        ;;
    rev)
        git ls-tree -r --name-only "$rev" > "$tmp/paths" 2>/dev/null || die "cannot list tree $rev"
        label=$rev
        ;;
esac

bad=0

while IFS= read -r path; do
    [ -n "$path" ] || continue
    case "$path" in
        */*)
            top=${path%%/*}
            ;;
        *)
            top=$path
            ;;
    esac
    if ! grep -Fxq -- "$top" "$tmp/allow"; then
        printf 'public-tree: top-level path not allowed: %s\n' "$path" >&2
        bad=1
    fi
    while IFS= read -r pruned; do
        case "$path" in
            "$pruned"|"$pruned"/*)
                printf 'public-tree: pruned path present: %s\n' "$path" >&2
                bad=1
                ;;
        esac
    done < "$tmp/prune"
done < "$tmp/paths"

while IFS= read -r required; do
    found=0
    while IFS= read -r path; do
        case "$path" in
            "$required"|"$required"/*)
                found=1
                break
                ;;
        esac
    done < "$tmp/paths"
    if [ "$found" -ne 1 ]; then
        printf 'public-tree: required path missing: %s\n' "$required" >&2
        bad=1
    fi
done < "$tmp/require"

if [ "$bad" -ne 0 ]; then
    echo "public-tree: FAIL: $label does not match the public-tree manifest." >&2
    exit 1
fi

count=$(wc -l < "$tmp/paths" | tr -d ' ')
echo "public-tree: OK: $label has $count file(s), no pruned path and every required path."
exit 0
