#!/usr/bin/env bash
# Build the candidate StackSpirit public branch: one parentless commit of the
# current HEAD tree, checked by the public-readiness scanner before it can be
# pushed.
#
# The owner's publication model (2026-11-09) is: the private repository and its
# history stay private; the public repository starts with a single commit. This
# script makes that commit locally. It never pushes, tags, publishes, changes
# repository visibility, runs git filter-branch, rewrites main, or deletes a
# branch or tag. The only ref it creates or replaces is the local
# `public-main` branch.
#
# Usage:
#   bash scripts/prepare-public-history.sh
#   bash scripts/prepare-public-history.sh --dry-run
#   bash scripts/prepare-public-history.sh --force
#   bash scripts/prepare-public-history.sh --message "StackSpirit 0.6.0"
#   bash scripts/prepare-public-history.sh --help
#
# The script refuses a dirty working tree (untracked files included), a missing
# HEAD and a missing scanner. It builds the commit with `git commit-tree` from
# the current HEAD tree in a temporary index-free step, so the working tree and
# the private branch are not changed. Author and committer are the public
# identity already used by the runtime squash script:
#   Syver-August <92798051+syver777@users.noreply.github.com>
#
# After creating (or, with --dry-run, only computing) the commit, the script
# runs `bash scripts/check-public-readiness.sh --rev <commit>` and exits
# non-zero if that scan does not pass. Every finding class is fatal there
# because this tree is what would be published. A failed scan leaves the branch
# in place for inspection.
#
# Exit status:
#   0  the candidate commit exists (or was computed) and scans clean
#   1  a prerequisite is missing, the scan failed, or the branch exists
#   2  usage error
#
# The script pushed nothing. It prints the owner's inspect and push commands.

set -euo pipefail

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
scanner=$root/scripts/check-public-readiness.sh
branch=public-main
author_name='Syver-August'
author_email='92798051+syver777@users.noreply.github.com'
message='StackSpirit 0.6.0'
force=0
dry_run=0

usage() {
  sed -n '2,/^set -euo pipefail$/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --force)
      force=1
      ;;
    --dry-run)
      dry_run=1
      ;;
    --message)
      [ "$#" -ge 2 ] || {
        echo "prepare-public-history: --message requires text" >&2
        exit 2
      }
      message=$2
      shift
      ;;
    --message=*)
      message=${1#*=}
      ;;
    *)
      echo "prepare-public-history: unknown argument: $1 (try --help)" >&2
      exit 2
      ;;
  esac
  shift
done

[ -n "$message" ] || {
  echo "prepare-public-history: commit message must not be empty" >&2
  exit 2
}

fail() {
  echo "prepare-public-history: $*" >&2
  exit 1
}

git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
  || fail "not a git working tree: $root"
[ -f "$scanner" ] || fail "public-readiness scanner not found: $scanner"

if [ -n "$(git -C "$root" status --porcelain)" ]; then
  echo "prepare-public-history: working tree is not clean:" >&2
  git -C "$root" status --short >&2
  fail "commit or stash every change first; the public commit must be reproducible"
fi

source_head=$(git -C "$root" rev-parse --verify --quiet 'HEAD^{commit}') \
  || fail "HEAD is missing or does not point at a commit"

current_branch=$(git -C "$root" symbolic-ref --quiet --short HEAD 2>/dev/null || true)
if git -C "$root" show-ref --verify --quiet "refs/heads/$branch"; then
  if [ "$force" -ne 1 ]; then
    fail "branch $branch already exists; use --force to replace it"
  fi
  [ "$current_branch" != "$branch" ] \
    || fail "cannot replace $branch while it is checked out"
  if git -C "$root" worktree list --porcelain \
      | grep -qx "branch refs/heads/$branch"; then
    fail "cannot replace $branch while it is checked out in another worktree"
  fi
fi

tree=$(git -C "$root" rev-parse --verify "$source_head^{tree}")
file_count=$(git -C "$root" ls-tree -r "$tree" | wc -l | tr -d ' ')

commit=$(
  GIT_AUTHOR_NAME="$author_name" \
  GIT_AUTHOR_EMAIL="$author_email" \
  GIT_COMMITTER_NAME="$author_name" \
  GIT_COMMITTER_EMAIL="$author_email" \
  git -C "$root" commit-tree "$tree" -m "$message"
) || fail "git commit-tree failed"

parents=$(git -C "$root" rev-list --parents -n 1 "$commit" | wc -w | tr -d ' ')
[ "$parents" -eq 1 ] || fail "$commit is not a parentless commit"

echo "prepare-public-history: repository <git top level>"
echo "prepare-public-history: source HEAD $source_head"
echo "prepare-public-history: tree $tree ($file_count files)"
echo "prepare-public-history: commit $commit (parentless)"
echo "prepare-public-history: author $author_name <$author_email>"
echo "prepare-public-history: committer $author_name <$author_email>"
echo "prepare-public-history: message: $message"

if [ "$dry_run" -eq 1 ]; then
  echo "prepare-public-history: dry run: branch $branch was not created"
  target=$commit
else
  git -C "$root" update-ref "refs/heads/$branch" "$commit"
  echo "prepare-public-history: branch $branch -> $commit"
  target=$branch
fi

echo "prepare-public-history: scanning $target with scripts/check-public-readiness.sh --rev"
scan_status=0
bash "$scanner" --rev "$target" || scan_status=$?
if [ "$scan_status" -ne 0 ]; then
  fail "public-readiness scan failed (exit $scan_status); the candidate was left in place for inspection"
fi

echo "prepare-public-history: public-readiness PASS"
echo "prepare-public-history: the working tree, main and every other ref are unchanged; nothing was pushed"
echo "prepare-public-history: inspect (owner):"
if [ "$dry_run" -eq 1 ]; then
  echo "  git show --stat $commit"
  echo "  git diff --stat HEAD $commit"
  echo "  # create the branch, then inspect it:"
  echo "  git branch $branch $commit"
else
  echo "  git log -1 --format=fuller $branch"
  echo "  git show --stat $branch"
  echo "  git diff --stat HEAD $branch"
fi
echo "prepare-public-history: push (owner action, after review; this script never pushes):"
echo "  git push <remote> $branch:main"
