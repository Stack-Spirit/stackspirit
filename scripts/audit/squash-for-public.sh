#!/bin/sh
# Build the public branch: one orphan commit of the current tree, with the
# internal build and operations paths pruned.
#
# This is the publication path chosen by the owner on 2026-11-09: the current
# private repository and its history stay private; the public repository starts
# with a single commit. The script refuses to create the branch if the working
# tree has findings, if the build is not green, or if the staged public tree
# contains a pruned path, is missing a required product path, or has a
# top-level path outside the publication allow-list.
#
# The staged tree is built in a temporary index. The private checkout, branch
# and files are left untouched. It never pushes, tags, publishes or changes
# repository visibility. Run it as an owner/orchestrator action, then follow
# scripts/audit/go-public.md.
#
# Usage:
#   scripts/audit/squash-for-public.sh
#   scripts/audit/squash-for-public.sh --force   # replace an existing public-main
#
# The prune/allow/require list is scripts/audit/public-tree.txt and the checker
# is scripts/audit/public-tree-check.sh.
#
# The resulting branch is `public-main`, with author and committer
#   Syver-August <92798051+syver777@users.noreply.github.com>
# and the single commit message `VOWP 0.1.0`.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2
cd "$root" || { echo "squash: cannot enter repository root" >&2; exit 2; }

branch=public-main
author_name='Syver-August'
author_email='92798051+syver777@users.noreply.github.com'
commit_message='VOWP 0.1.0'
manifest=$script_dir/public-tree.txt
force=0

usage() {
    cat <<'EOF'
Usage: scripts/audit/squash-for-public.sh [--force]

Creates the orphan branch `public-main` with one commit of the current tree,
after:
  1. scripts/audit/scan.sh --worktree passes;
  2. cargo fmt --all -- --check passes;
  3. cargo clippy --workspace --all-targets -- -D warnings passes;
  4. cargo test --workspace passes;
  5. the staged public tree removes every path in scripts/audit/public-tree.txt,
     contains every required product path and keeps only allowed top-level
     entries (scripts/audit/public-tree-check.sh --index);
  6. scripts/audit/public-tree-check.sh --rev public-main passes;
  7. scripts/audit/scan.sh --rev public-main passes.

The staged tree is built in a temporary index, so the private checkout is not
changed. The script never pushes. It prints the exact commands for the owner.
  --force   delete and recreate an existing public-main branch
  -h, --help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --force)
            force=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "squash: unknown option: $1 (try --help)" >&2
            exit 2
            ;;
    esac
done

fail() {
    echo "squash: $*" >&2
    exit 1
}

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail "not a git working tree: $root"

if [ -n "$(git status --porcelain)" ]; then
    echo "squash: working tree is not clean:" >&2
    git status --short >&2
    fail "commit or stash every change first; the public commit must be reproducible"
fi

[ -f "$manifest" ] || fail "public-tree manifest not found: $manifest"

original=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || git rev-parse HEAD)

if git show-ref --verify --quiet "refs/heads/$branch"; then
    if [ "$force" -ne 1 ]; then
        fail "branch $branch already exists; use --force to replace it"
    fi
    [ "$original" != "$branch" ] || fail "cannot replace $branch while it is checked out"
    git branch -D "$branch" >/dev/null 2>&1 || fail "cannot delete existing $branch (is it checked out in another worktree?)"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/vowp-public.XXXXXX") || fail "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

echo "squash: scanning the working tree"
scripts/audit/scan.sh --worktree || fail "the working tree still has a non-allowlisted finding"

echo "squash: checking formatting"
cargo fmt --all -- --check || fail "cargo fmt --all -- --check failed"

echo "squash: running clippy"
cargo clippy --workspace --all-targets -- -D warnings || fail "cargo clippy failed"

echo "squash: running the test suite"
cargo test --workspace || fail "cargo test --workspace failed"

echo "squash: staging the public tree in a temporary index"
GIT_INDEX_FILE=$tmp/index
export GIT_INDEX_FILE
git read-tree --empty || fail "git read-tree --empty failed"
git add -A || fail "git add -A failed"

echo "squash: removing internal paths from the public index"
while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
        ''|'#'*)
            continue
            ;;
    esac
    set -- $line
    [ "$#" -eq 2 ] || continue
    [ "$1" = prune ] || continue
    path=${2%/}
    [ -n "$path" ] || continue
    git rm -r --cached --ignore-unmatch --quiet -- "$path" || fail "git rm --cached $path failed"
done < "$manifest"

echo "squash: checking the staged public tree"
scripts/audit/public-tree-check.sh --index --manifest "$manifest" || fail "the staged public tree does not match the manifest"

tree=$(git write-tree) || fail "git write-tree failed"

GIT_AUTHOR_NAME="$author_name"
GIT_AUTHOR_EMAIL="$author_email"
GIT_COMMITTER_NAME="$author_name"
GIT_COMMITTER_EMAIL="$author_email"
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

commit=$(git commit-tree "$tree" -m "$commit_message") || fail "git commit-tree failed"
unset GIT_INDEX_FILE
git update-ref "refs/heads/$branch" "$commit" || fail "git update-ref refs/heads/$branch failed"

commits=$(git rev-list --count "$branch")
[ "$commits" -eq 1 ] || fail "$branch has $commits commits, expected exactly 1"
identity=$(git log -1 --format='%an <%ae>' "$branch")
committer=$(git log -1 --format='%cn <%ce>' "$branch")
[ "$identity" = "$author_name <$author_email>" ] || fail "unexpected author identity: $identity"
[ "$committer" = "$author_name <$author_email>" ] || fail "unexpected committer identity: $committer"

echo "squash: checking the new tree"
scripts/audit/public-tree-check.sh --rev "$branch" --manifest "$manifest" || fail "$branch does not match the manifest; it was left in place for inspection"
scripts/audit/scan.sh --rev "$branch" || fail "$branch still has a non-allowlisted finding; it was left in place for inspection"

echo
echo "squash: $branch is ready: 1 commit, author $identity"
echo "squash: the working tree and branch were not changed; still on $original"
echo "squash: next commands (the orchestrator runs them; this script never pushes):"
echo
echo "  # 1. Rename the old private repository first, so the new name is free:"
echo "  gh repo rename vowp-private --repo syver777/vowp --yes"
echo
echo "  # 2. Create the empty public repository:"
echo "  gh repo create syver777/vowp --public --description 'Local-first supervisor for AI coding agents'"
echo
echo "  # 3. Push this one commit as main:"
echo "  git remote add public git@github.com:syver777/vowp.git   # or: git remote set-url public ..."
echo "  git push public $branch:main"
echo
echo "  # 4. Tag the release and push the tag (this starts the draft-release workflow):"
echo "  git tag -a v0.1.0 -m 'VOWP 0.1.0' $branch"
echo "  git push public v0.1.0"
echo
echo "  # 5. Follow the rest of scripts/audit/go-public.md (release, crates, npm)."
echo
