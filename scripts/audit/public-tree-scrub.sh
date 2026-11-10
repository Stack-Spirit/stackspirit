#!/bin/sh
# Apply the public-tree content scrub to a staged tree.
#
# A few product files contain comments about the private build process that
# cannot be pruned because the file itself ships. This script replaces the
# known paragraphs with product-facing text in the staged index only; the
# private working tree is not changed.
#
# Usage:
#   scripts/audit/public-tree-scrub.sh --index
#
# --index honours GIT_INDEX_FILE, so squash-for-public.sh can scrub the
# temporary public index before it writes the public commit. The scrub is
# idempotent: an already-scrubbed file, or a file whose old paragraph was
# removed from the private tree, succeeds without changing anything. A missing
# file, a duplicated paragraph, a mix of old and new text, or a partial
# paragraph is a failure (exit 1), so a future edit cannot silently leave
# build-process prose in the public tree.
#
# Exit status:
#   0  the staged tree is scrubbed (or already clean)
#   1  the staged file is missing, duplicated, mixed or partial
#   2  usage error or not a git working tree

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2
cd "$root" || { echo "public-tree-scrub: cannot enter repository root" >&2; exit 2; }

case "${1:-}" in
    --index)
        ;;
    -h|--help)
        cat <<'EOF'
Usage: scripts/audit/public-tree-scrub.sh --index

Rewrites the known build-process prose in the staged public tree. The private
working tree is not changed. An already-clean file is a no-op; exit 1 when the
staged file is missing, duplicated, mixed or partial; exit 2 on a usage error.
EOF
        exit 0
        ;;
    *)
        echo "public-tree-scrub: usage: scripts/audit/public-tree-scrub.sh --index" >&2
        exit 2
        ;;
esac

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
    echo "public-tree-scrub: not a git working tree: $root" >&2
    exit 2
}

exec python3 "$script_dir/public-tree-scrub.py" --index
