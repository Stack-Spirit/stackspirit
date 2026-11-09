#!/bin/sh
# Build the public branch: one orphan commit of the current tree.
#
# This is the publication path chosen by the owner on 2026-11-09: the current
# private repository and its history stay private; the public repository starts
# with a single commit. The script refuses to create the branch if the working
# tree has findings, if the build is not green, or if `git ls-files` contains a
# top-level path outside the publication allow-list.
#
# It never pushes, tags, publishes or changes repository visibility. Run it as
# an owner/orchestrator action, then follow scripts/audit/go-public.md.
#
# Usage:
#   scripts/audit/squash-for-public.sh
#   scripts/audit/squash-for-public.sh --force   # replace an existing public-main
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
  5. git ls-files contains only the allowed top-level paths;
  6. scripts/audit/scan.sh --rev public-main passes.

The script never pushes. It prints the exact commands for the owner.
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

if git show-ref --verify --quiet "refs/heads/$branch"; then
    if [ "$force" -ne 1 ]; then
        fail "branch $branch already exists; use --force to replace it"
    fi
    git branch -D "$branch" >/dev/null 2>&1 || fail "cannot delete existing $branch (is it checked out in another worktree?)"
fi

original=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || git rev-parse HEAD)

echo "squash: scanning the working tree"
scripts/audit/scan.sh --worktree || fail "the working tree still has a non-allowlisted finding"

echo "squash: checking formatting"
cargo fmt --all -- --check || fail "cargo fmt --all -- --check failed"

echo "squash: running clippy"
cargo clippy --workspace --all-targets -- -D warnings || fail "cargo clippy failed"

echo "squash: running the test suite"
cargo test --workspace || fail "cargo test --workspace failed"

echo "squash: checking top-level paths"
files_tmp=$(mktemp "${TMPDIR:-/tmp}/vowp-public-files.XXXXXX") || fail "cannot create a temporary file"
trap 'rm -f "$files_tmp"' EXIT HUP INT TERM
git ls-files > "$files_tmp" || fail "cannot list tracked files"
bad_paths=0
while IFS= read -r path; do
    [ -n "$path" ] || continue
    case "$path" in
        */*)
            top=${path%%/*}
            case "$top" in
                crates|docs|examples|framework|packaging|scripts|site|.github)
                    ;;
                *)
                    echo "squash: not allowed at the top level: $path" >&2
                    bad_paths=$((bad_paths + 1))
                    ;;
            esac
            ;;
        *)
            case "$path" in
                .editorconfig|.gitignore|AGENTS.md|CHANGELOG.md|CODE_OF_CONDUCT.md|CONTRIBUTING.md|Cargo.lock|Cargo.toml|LICENSE|README.md|SECURITY.md|SUPPORT.md|THIRD-PARTY-LICENSES.md|rust-toolchain.toml|rustfmt.toml)
                    ;;
                *)
                    echo "squash: not allowed at the top level: $path" >&2
                    bad_paths=$((bad_paths + 1))
                    ;;
            esac
            ;;
    esac
done < "$files_tmp"
[ "$bad_paths" -eq 0 ] || fail "$bad_paths tracked path(s) are outside the publication allow-list"

echo "squash: creating the orphan branch $branch"
git checkout --orphan "$branch" || fail "git checkout --orphan $branch failed"
git add -A || fail "git add -A failed"

GIT_AUTHOR_NAME="$author_name"
GIT_AUTHOR_EMAIL="$author_email"
GIT_COMMITTER_NAME="$author_name"
GIT_COMMITTER_EMAIL="$author_email"
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

git commit -m "$commit_message" || fail "git commit failed"

commits=$(git rev-list --count "$branch")
[ "$commits" -eq 1 ] || fail "$branch has $commits commits, expected exactly 1"
identity=$(git log -1 --format='%an <%ae>' "$branch")
committer=$(git log -1 --format='%cn <%ce>' "$branch")
[ "$identity" = "$author_name <$author_email>" ] || fail "unexpected author identity: $identity"
[ "$committer" = "$author_name <$author_email>" ] || fail "unexpected committer identity: $committer"

echo "squash: checking the new tree"
scripts/audit/scan.sh --rev "$branch" || fail "$branch still has a non-allowlisted finding; it was left in place for inspection"

echo
echo "squash: $branch is ready: 1 commit, author $identity"
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
echo "squash: original checkout was $original; return with: git checkout $original"
