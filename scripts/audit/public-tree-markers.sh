#!/bin/sh
# Scan a public candidate tree for private build metadata that the secret
# scanner does not cover: night-shift names, AI commit trailers, v0.2 job ids,
# references to pruned private paths and build-diary wording.
#
# This is the audit companion to scripts/audit/scan.sh. It is deliberately
# separate: scan.sh covers secrets, home paths, hostnames and large blobs;
# this script covers the build-process material that must not ship.
#
# Usage:
#   scripts/audit/public-tree-markers.sh --worktree
#   scripts/audit/public-tree-markers.sh --index
#   scripts/audit/public-tree-markers.sh --rev <commit>
#
# `--worktree` scans only the paths that would ship: every `prune` entry in
# scripts/audit/public-tree.txt is skipped. `--index` honours GIT_INDEX_FILE,
# so the squash script can check the staged public tree. `--rev` scans one
# commit's whole tree.
#
# Exit status:
#   0  no private build marker found
#   1  at least one private build marker found
#   2  usage error, not a git repository, or a missing manifest
#
# No network access. POSIX shell plus git, grep and sed.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2

manifest=
mode=
rev=
summary=0

usage() {
    cat <<'EOF'
Usage: scripts/audit/public-tree-markers.sh --worktree|--index|--rev <commit> [options]

Scopes:
  --worktree        scan the working-tree paths that would ship
  --index           scan the staged index (honours GIT_INDEX_FILE)
  --rev <commit>    scan one commit's whole tree

Options:
  --summary         print rule counts instead of each finding
  --manifest <f>    public-tree manifest for --worktree and --index
                    (default: <root>/scripts/audit/public-tree.txt)
  --root <dir>      repository root to scan (default: this script's repository)
  -h, --help        show this help

Findings are printed as path:line: rule: line text. Exit 1 when any marker is
found, 2 on a usage or manifest error.
EOF
}

die() {
    echo "public-tree-markers: $*" >&2
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
        --summary)
            summary=1
            shift
            ;;
        --manifest)
            [ "$#" -ge 2 ] || die "--manifest needs a file"
            manifest=$2
            shift 2
            ;;
        --root)
            [ "$#" -ge 2 ] || die "--root needs a directory"
            root=$2
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

root=$(CDPATH= cd -- "$root" && pwd -P) || die "cannot enter root: $root"
cd "$root" || die "cannot enter root: $root"
[ -n "$manifest" ] || manifest="$root/scripts/audit/public-tree.txt"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git working tree: $root"

if [ "$mode" = rev ]; then
    rev=$(git rev-parse --verify "$rev^{commit}" 2>/dev/null) || die "not a commit: $rev"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-markers.XXXXXX") || die "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

: > "$tmp/prune"
case "$mode" in
    worktree|index)
    [ -f "$manifest" ] || die "manifest not found: $manifest"
    [ -r "$manifest" ] || die "manifest is not readable: $manifest"
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            ''|'#'*)
                continue
                ;;
        esac
        set -- $line
        [ "$#" -ge 2 ] && [ "$1" = prune ] || continue
        path=${2%/}
        [ -n "$path" ] && printf '%s\n' "$path" >> "$tmp/prune"
    done < "$manifest"
        ;;
esac

is_pruned() {
    case "$mode" in
        worktree|index)
            ;;
        *)
            return 1
            ;;
    esac
    path=$1
    while IFS= read -r pruned; do
        [ -n "$pruned" ] || continue
        case "$path" in
            "$pruned"|"$pruned"/*)
                return 0
                ;;
        esac
    done < "$tmp/prune"
    return 1
}

# name<TAB>extended-regular-expression. Keep each pattern narrow enough that a
# legitimate product mention (for example `AI-Assisted: <model> via VOWP`)
# does not match.
cat > "$tmp/rules" <<'RULES'
night_shift	vowp-night|night-shift|night shift
ai_trailer	AI-Assisted:.*(via vowp-night|(deepseek|claude|gpt|opencode|anthropic)/)
job_id	\bv2-[a-z][a-z0-9]*-[a-z][a-z0-9-]*\b
private_path	docs/maintainers/|scripts/spikes/|scripts/acceptance/|scripts/dogfood/|scripts/fresh-user/|scripts/audit/
build_diary	\b(J1|J2) (scrub|release)\b|v2-b2-spec-amend|K[1-4]'s job|T[0-9] (reports|acceptance)|spike \(merged|merged 2026-11|This is the specification the v0\.2 jobs implement|C0, C1 and the feature job ids|^B1 spec \(this directory\)|C0 skeleton|C0 freezes|C1-owned|is B1 → C0
RULES

grep_tree() {
    regex=$1
    case "$mode" in
        worktree)
            git grep -nIE "$regex" -- . 2>/dev/null || true
            ;;
        index)
            git grep --cached -nIE "$regex" -- . 2>/dev/null || true
            ;;
        rev)
            git grep -nIE "$regex" "$rev" -- . 2>/dev/null || true
            ;;
    esac
}

: > "$tmp/findings"
while IFS='	' read -r rule regex; do
    [ -n "$rule" ] || continue
    grep_tree "$regex" > "$tmp/raw"
    while IFS= read -r hit; do
        [ -n "$hit" ] || continue
        path=${hit%%:*}
        if is_pruned "$path"; then
            continue
        fi
        printf '%s: %s\n' "$rule" "$hit" >> "$tmp/findings"
    done < "$tmp/raw"
done < "$tmp/rules"

total=$(wc -l < "$tmp/findings" | tr -d ' ')

if [ "$summary" -eq 1 ]; then
    printf 'public-tree-markers: %s finding(s)\n' "$total"
    if [ "$total" -gt 0 ]; then
        awk -F': ' '{ count[$1]++ } END { for (rule in count) printf "  %-14s %d\n", rule, count[rule] }' \
            "$tmp/findings" | sort
    fi
else
    cat "$tmp/findings"
fi

if [ "$total" -gt 0 ]; then
    echo "public-tree-markers: FAIL: $total private build marker(s)." >&2
    exit 1
fi

echo "public-tree-markers: OK: no private build marker in the scanned tree."
exit 0
