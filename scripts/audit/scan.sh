#!/bin/sh
# Public-readiness scanner for the VOWP repository.
#
# Scans tracked files in the working tree and every commit reachable from
# `git rev-list --all` for private data and secret-shaped strings. It never
# prints a matched value in full: only the first four characters are shown.
#
# No network access. POSIX shell plus git, grep, awk and sed.
#
# Usage:
#   scripts/audit/scan.sh                  # working tree + all history
#   scripts/audit/scan.sh --worktree       # working tree only (fatal findings)
#   scripts/audit/scan.sh --public-tree    # public paths in the working tree
#   scripts/audit/scan.sh --rev <commit>   # one tree (fatal findings)
#   scripts/audit/scan.sh --history        # all history (informational)
#   scripts/audit/scan.sh --summary        # counts only, no individual lines
#   scripts/audit/scan.sh --strict-history # make history findings fatal too
#
# Exit status:
#   0  no non-allowlisted findings in the fatal scopes
#   1  at least one non-allowlisted finding in a fatal scope
#   2  usage error, not a git repository, or an invalid allowlist entry
#
# The allowlist is scripts/audit/allowlist.txt. Each entry is
#   rule|path|matched-value|reason
# A path of `*` means the value is safe wherever it appears; prefer an exact
# path. Every entry needs a non-empty reason. The scanner validates the file.
#
# The public branch is built from one fresh commit, so findings in old commits
# are reported for the audit record but are not fatal by default. Use
# --strict-history when auditing history that will be published.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2
cd "$root" || { echo "scan: cannot enter repository root" >&2; exit 2; }

allowlist="$script_dir/allowlist.txt"
manifest="$script_dir/public-tree.txt"
max_blob=${AUDIT_MAX_BLOB_BYTES:-1048576}
mode=all
public=0
strict_history=0
summary=0
rev=

usage() {
    cat <<'EOF'
Usage: scripts/audit/scan.sh [options]

Scopes (default: --all):
  --worktree         scan tracked files in the working tree only
  --public-tree      scan the working-tree paths that would ship: every prune
                     path in the public-tree manifest is skipped
  --history          scan every commit reachable from git rev-list --all
  --rev <commit>     scan one commit's tree only (fatal findings)
  --all              working tree + history (the default)

Options:
  --summary          print category counts instead of each finding
  --strict-history   treat history findings as fatal as well
  --allowlist <f>    use another allowlist file
  --manifest <f>     public-tree manifest for --public-tree
                     (default: scripts/audit/public-tree.txt)
  --max-blob-bytes N flag tracked blobs larger than N bytes (default 1048576)
  -h, --help         show this help

Findings are printed as file:commit:line: rule: <first four characters>***.
No matched value is ever printed in full. Exit 1 on a non-allowlisted finding
in a fatal scope; history findings are informational unless --strict-history.
EOF
}

die() {
    echo "scan: $*" >&2
    exit 2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --worktree|--no-history)
            mode=worktree
            shift
            ;;
        --public-tree)
            mode=worktree
            public=1
            shift
            ;;
        --history)
            mode=history
            shift
            ;;
        --rev)
            [ "$#" -ge 2 ] || die "--rev needs a commit"
            rev=$2
            mode=rev
            shift 2
            ;;
        --all)
            mode=all
            shift
            ;;
        --strict-history)
            strict_history=1
            shift
            ;;
        --summary)
            summary=1
            shift
            ;;
        --allowlist)
            [ "$#" -ge 2 ] || die "--allowlist needs a file"
            allowlist=$2
            shift 2
            ;;
        --manifest)
            [ "$#" -ge 2 ] || die "--manifest needs a file"
            manifest=$2
            shift 2
            ;;
        --max-blob-bytes)
            [ "$#" -ge 2 ] || die "--max-blob-bytes needs a number"
            max_blob=$2
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

case "$max_blob" in
    ''|*[!0-9]*) die "--max-blob-bytes must be a non-negative integer" ;;
esac

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git working tree: $root"
[ -f "$allowlist" ] || die "allowlist not found: $allowlist"
[ -r "$allowlist" ] || die "allowlist is not readable: $allowlist"

if [ "$public" -eq 1 ]; then
    [ -f "$manifest" ] || die "manifest not found: $manifest"
    [ -r "$manifest" ] || die "manifest is not readable: $manifest"
fi

if [ "$mode" = rev ]; then
    rev=$(git rev-parse --verify "$rev^{commit}" 2>/dev/null) || die "not a commit: $rev"
fi

# The seven content patterns, plus the secret_path and large_blob path checks.
# They are also documented in docs/maintainers/public-audit.md.
RE_IP='([0-9]{1,3}\.){3}[0-9]{1,3}'
RE_HOME='(/home/|/Users/)[A-Za-z0-9._-]+'
RE_EMAIL='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'
RE_TOKEN='(sk-[A-Za-z0-9_-]{20,}|sb_secret_[A-Za-z0-9_-]{10,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|xox[abp]-[A-Za-z0-9-]{10,})'
RE_HOST='[A-Za-z0-9][A-Za-z0-9._-]*\.(local|lan|internal|home\.arpa|ts\.net)'
RE_SSH='ssh +[A-Za-z0-9._-]+@[A-Za-z0-9._-]+'
# A JSON/TOML `ssh_target` field carries a concrete user@host even when the
# value is not written as an `ssh` command. Match the field and its value.
RE_SSH_TARGET='ssh_target"?[[:space:]]*[:=][[:space:]]*"[^"]+"'
RE_COMBINED="$RE_IP|$RE_HOME|$RE_EMAIL|$RE_TOKEN|$RE_HOST|$RE_SSH|$RE_SSH_TARGET"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-audit.XXXXXX") || die "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

: > "$tmp/counts"
: > "$tmp/prune"
if [ "$public" -eq 1 ]; then
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
    prune_count=$(wc -l < "$tmp/prune" | tr -d ' ')
    printf 'scan: public-tree: skipping %s pruned path(s) from %s\n' "$prune_count" "$manifest"
fi

# Rule names and regexes for AWK, one per line as name<TAB>regex.
{
    printf 'ipv4\t%s\n' "$RE_IP"
    printf 'home_path\t%s\n' "$RE_HOME"
    printf 'email\t%s\n' "$RE_EMAIL"
    printf 'token\t%s\n' "$RE_TOKEN"
    printf 'private_host\t%s\n' "$RE_HOST"
    printf 'ssh_host\t%s\n' "$RE_SSH"
    printf 'ssh_target\t%s\n' "$RE_SSH_TARGET"
} > "$tmp/rules"

# AWK does the content classification in one process per tree. It reads the
# allowlist itself, masks every value before printing, and appends one
# `scope|rule|decision` line per match to the count file.
cat > "$tmp/scan.awk" <<'AWK'
function trim(s) {
    gsub(/^[ \t]+|[ \t]+$/, "", s)
    return s
}

function mask(v) {
    return substr(v, 1, 4) "***"
}

function allowed(rule, path, value,  key) {
    key = rule SUBSEP path SUBSEP value
    if (key in allow) {
        return 1
    }
    key = rule SUBSEP "*" SUBSEP value
    return key in allow
}

function is_pruned(path,  i, p) {
    if (!public) {
        return 0
    }
    for (i = 1; i <= prune_count; i++) {
        p = prune_path[i]
        if (path == p || index(path, p "/") == 1) {
            return 1
        }
    }
    return 0
}

BEGIN {
    while ((getline line < allowfile) > 0) {
        if (line ~ /^[ \t]*#/ || line ~ /^[ \t]*$/) {
            continue
        }
        n = split(line, field, "|")
        if (n < 4) {
            printf "scan: invalid allowlist entry (need rule|path|value|reason): %s\n", line > "/dev/stderr"
            bad = 1
            continue
        }
        rule = trim(field[1])
        path = trim(field[2])
        value = trim(field[3])
        reason = trim(field[4])
        if (rule == "" || path == "" || value == "" || reason == "") {
            printf "scan: invalid allowlist entry (empty field): %s\n", line > "/dev/stderr"
            bad = 1
            continue
        }
        allow[rule SUBSEP path SUBSEP value] = 1
    }
    close(allowfile)

    while ((getline line < prunefile) > 0) {
        if (line == "") {
            continue
        }
        prune_count++
        prune_path[prune_count] = line
    }
    close(prunefile)

    while ((getline line < rulesfile) > 0) {
        tab = index(line, "\t")
        if (tab == 0) {
            continue
        }
        rule_count++
        rule_name[rule_count] = substr(line, 1, tab - 1)
        rule_re[rule_count] = substr(line, tab + 1)
    }
    close(rulesfile)

    if (bad || rule_count == 0) {
        exit 2
    }
}

{
    if (scope == "worktree") {
        i = index($0, ":")
        if (i == 0) {
            next
        }
        path = substr($0, 1, i - 1)
        rest = substr($0, i + 1)
        j = index(rest, ":")
        if (j == 0) {
            next
        }
        lineno = substr(rest, 1, j - 1)
        text = substr(rest, j + 1)
    } else {
        i = index($0, ":")
        if (i == 0) {
            next
        }
        rest = substr($0, i + 1)
        j = index(rest, ":")
        if (j == 0) {
            next
        }
        path = substr(rest, 1, j - 1)
        rest = substr(rest, j + 1)
        k = index(rest, ":")
        if (k == 0) {
            next
        }
        lineno = substr(rest, 1, k - 1)
        text = substr(rest, k + 1)
    }

    if (scope == "worktree" && is_pruned(path)) {
        next
    }

    for (r = 1; r <= rule_count; r++) {
        rule = rule_name[r]
        regex = rule_re[r]
        remaining = text
        while (match(remaining, regex)) {
            value = substr(remaining, RSTART, RLENGTH)
            remaining = substr(remaining, RSTART + RLENGTH)
            if (value == "") {
                continue
            }
            # `args.local_port`, `listener.local_addr()` and `self.local.path()`
            # are Rust field access, not hostnames. A real hostname is followed
            # by a delimiter (quote, colon, slash, whitespace or end of line).
            if (rule == "private_host" && remaining ~ /^[A-Za-z0-9_.]/) {
                continue
            }
            if (rule == "email" && value ~ /@users\.noreply\.github\.com$/) {
                print scope "|" rule "|allowed" >> countfile
                continue
            }
            if (allowed(rule, path, value)) {
                print scope "|" rule "|allowed" >> countfile
            } else {
                print scope "|" rule "|blocked" >> countfile
                if (!summary) {
                    printf "%s:%s:%s: %s: %s\n", path, commit, lineno, rule, mask(value)
                }
            }
        }
    }
}
AWK

record() {
    printf '%s|%s|%s\n' "$1" "$2" "$3" >> "$tmp/counts"
}

# Content scan for one tree. `scope` is worktree, history or rev; `commit` is
# the label printed in file:commit:line.
scan_content() {
    scope=$1
    tree=$2
    label=$3
    if [ "$scope" = worktree ]; then
        git grep -nIE "$RE_COMBINED" -- . 2>/dev/null || true
    else
        git grep -nIE "$RE_COMBINED" "$tree" -- . 2>/dev/null || true
    fi | awk \
        -v scope="$scope" \
        -v commit="$label" \
        -v allowfile="$allowlist" \
        -v rulesfile="$tmp/rules" \
        -v prunefile="$tmp/prune" \
        -v public="$public" \
        -v countfile="$tmp/counts" \
        -v summary="$summary" \
        -f "$tmp/scan.awk"
    status=$?
    if [ "$status" -eq 2 ]; then
        exit 2
    fi
}

# True when --public-tree is active and `path` is under a manifest prune entry.
is_pruned_path() {
    [ "$public" -eq 1 ] || return 1
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

# True for a tracked path whose name is a secret-bearing file.
is_secret_path() {
    path=$1
    base=${path##*/}
    case "$base" in
        .env|.env.*)
            [ "$base" = ".env.example" ] && return 1
            return 0
            ;;
        *.pem)
            return 0
            ;;
        id_rsa*|id_dsa*|id_ecdsa*|id_ed25519*|authorized_keys)
            return 0
            ;;
    esac
    case "$path" in
        .ssh/*|*/.ssh/*)
            return 0
            ;;
    esac
    return 1
}

# Allowlist check for path and blob findings, which are few enough for one
# awk process each.
is_allowed_path() {
    rule=$1
    path=$2
    value=$3
    awk -F'|' -v r="$rule" -v p="$path" -v v="$value" '
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        {
            gsub(/^[ \t]+|[ \t]+$/, "", $1)
            gsub(/^[ \t]+|[ \t]+$/, "", $2)
            gsub(/^[ \t]+|[ \t]+$/, "", $3)
            gsub(/^[ \t]+|[ \t]+$/, "", $4)
            if ($1 == r && ($2 == "*" || $2 == p) && $3 == v && $4 != "") {
                found = 1
            }
        }
        END { exit found ? 0 : 1 }
    ' "$allowlist"
}

report_path_finding() {
    scope=$1
    label=$2
    path=$3
    rule=$4
    if is_allowed_path "$rule" "$path" "$path"; then
        record "$scope" "$rule" allowed
    else
        record "$scope" "$rule" blocked
        if [ "$summary" -eq 0 ]; then
            printf '%s:%s:0: %s: %s\n' "$path" "$label" "$rule" "$path"
        fi
    fi
}

scan_paths() {
    scope=$1
    tree=$2
    label=$3
    if [ "$scope" = worktree ]; then
        git ls-files > "$tmp/paths" 2>/dev/null || true
    else
        git ls-tree -r --name-only "$tree" > "$tmp/paths" 2>/dev/null || true
    fi
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        if is_pruned_path "$path"; then
            continue
        fi
        if is_secret_path "$path"; then
            report_path_finding "$scope" "$label" "$path" secret_path
        fi
    done < "$tmp/paths"
}

report_blob_finding() {
    scope=$1
    label=$2
    path=$3
    size=$4
    if is_allowed_path large_blob "$path" "$path"; then
        record "$scope" large_blob allowed
    else
        record "$scope" large_blob blocked
        if [ "$summary" -eq 0 ]; then
            printf '%s:%s:0: large_blob: %s bytes\n' "$path" "$label" "$size"
        fi
    fi
}

scan_worktree_blobs() {
    git ls-files -s > "$tmp/ls-files" 2>/dev/null || true
    while read -r file_mode file_sha file_stage file_path; do
        [ -n "$file_path" ] || continue
        if is_pruned_path "$file_path"; then
            continue
        fi
        file_size=$(git cat-file -s "$file_sha" 2>/dev/null || printf '0')
        case "$file_size" in
            ''|*[!0-9]*) file_size=0 ;;
        esac
        if [ "$file_size" -gt "$max_blob" ]; then
            report_blob_finding worktree worktree "$file_path" "$file_size"
        fi
    done < "$tmp/ls-files"
}

scan_rev_blobs() {
    tree=$1
    label=$2
    git ls-tree -r -l "$tree" > "$tmp/ls-tree" 2>/dev/null || true
    while read -r file_mode file_type file_sha file_size file_path; do
        [ -n "$file_path" ] || continue
        case "$file_size" in
            ''|*[!0-9]*) continue ;;
        esac
        if [ "$file_size" -gt "$max_blob" ]; then
            report_blob_finding rev "$label" "$file_path" "$file_size"
        fi
    done < "$tmp/ls-tree"
}

scan_history_blobs() {
    git rev-list --objects --all 2>/dev/null |
        git cat-file --batch-check='%(objectname) %(objecttype) %(objectsize) %(rest)' 2>/dev/null |
        awk -v max="$max_blob" '$2 == "blob" && $3 > max { print $4 "|" $3 }' |
        sort -u > "$tmp/large-blobs"
    while IFS='|' read -r path size; do
        [ -n "$path" ] || path="<unknown>"
        report_blob_finding history history "$path" "$size"
    done < "$tmp/large-blobs"
}

# --- worktree ---------------------------------------------------------------

if [ "$mode" = worktree ] || [ "$mode" = all ]; then
    scan_content worktree "" worktree
    scan_paths worktree "" worktree
    scan_worktree_blobs
fi

# --- history ----------------------------------------------------------------

if [ "$mode" = history ] || [ "$mode" = all ]; then
    git rev-list --all > "$tmp/commits" 2>/dev/null || true
    while IFS= read -r commit; do
        [ -n "$commit" ] || continue
        short=$(git rev-parse --short "$commit" 2>/dev/null || printf '%s' "$commit")
        scan_content history "$commit" "$short"
        scan_paths history "$commit" "$short"
    done < "$tmp/commits"
    scan_history_blobs
fi

# --- one revision -----------------------------------------------------------

if [ "$mode" = rev ]; then
    short=$(git rev-parse --short "$rev" 2>/dev/null || printf '%s' "$rev")
    scan_content rev "$rev" "$short"
    scan_paths rev "$rev" "$short"
    scan_rev_blobs "$rev" "$short"
fi

# --- report -----------------------------------------------------------------

scope_total() {
    awk -F'|' -v s="$1" '$1 == s { n++ } END { printf "%d", n + 0 }' "$tmp/counts"
}

scope_decision() {
    awk -F'|' -v s="$1" -v d="$2" '$1 == s && $3 == d { n++ } END { printf "%d", n + 0 }' "$tmp/counts"
}

print_scope_counts() {
    scope=$1
    label=$2
    total=$(scope_total "$scope")
    allowed=$(scope_decision "$scope" allowed)
    blocked=$(scope_decision "$scope" blocked)
    printf 'scan: %s: %s finding(s), %s allowlisted, %s not allowlisted\n' \
        "$label" "$total" "$allowed" "$blocked"
}

print_category_counts() {
    scope=$1
    label=$2
    printf 'scan: %s by category:\n' "$label"
    awk -F'|' -v s="$scope" '
        $1 == s { key = $2 "|" $3; count[key]++ }
        END {
            for (key in count) {
                split(key, part, "|")
                printf "  %-16s %-8s %d\n", part[1], part[2], count[key]
            }
        }
    ' "$tmp/counts" | sort
}

echo "scan: repository $root"
echo "scan: commit $(git rev-parse --short HEAD 2>/dev/null || printf 'none') (mode: $mode)"

worktree_blocked=0
history_blocked=0
rev_blocked=0

if [ "$mode" = worktree ] || [ "$mode" = all ]; then
    print_scope_counts worktree "working tree"
    [ "$summary" -eq 1 ] && print_category_counts worktree "working tree"
    worktree_blocked=$(scope_decision worktree blocked)
fi

if [ "$mode" = history ] || [ "$mode" = all ]; then
    print_scope_counts history "history"
    [ "$summary" -eq 1 ] && print_category_counts history "history"
    history_blocked=$(scope_decision history blocked)
fi

if [ "$mode" = rev ]; then
    print_scope_counts rev "tree $rev"
    [ "$summary" -eq 1 ] && print_category_counts rev "tree $rev"
    rev_blocked=$(scope_decision rev blocked)
fi

failed=0

if [ "$worktree_blocked" -gt 0 ] || [ "$rev_blocked" -gt 0 ]; then
    failed=1
fi

if [ "$history_blocked" -gt 0 ]; then
    if [ "$strict_history" -eq 1 ]; then
        failed=1
    else
        echo "scan: history findings are informational: the public branch starts from one fresh commit."
        echo "scan: use --strict-history to make them fatal."
    fi
fi

if [ "$failed" -eq 1 ]; then
    echo "scan: FAIL: add a reasoned entry to scripts/audit/allowlist.txt or fix the finding." >&2
    exit 1
fi

echo "scan: OK: no non-allowlisted finding in the fatal scope(s)."
exit 0
