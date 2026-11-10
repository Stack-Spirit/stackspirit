#!/usr/bin/env python3
"""Rewrite known build-process prose in the staged public tree.

The public branch is built from the private tree with a temporary index. A few
product files contain comments about the private build process that cannot be
pruned because the file itself ships. This script replaces the known paragraphs
with product-facing text in the index only; the private working tree is not
changed.

Every rule is idempotent: one exact old paragraph is rewritten, an already
scrubbed file is a no-op, and a file that no longer contains the paragraph at
all is a no-op (the private tree may have been cleaned up first). A duplicate,
a mix of old and new text, or a left-over line from either paragraph is still a
failure, so a partial edit cannot silently leave build-process prose in the
public tree.

Usage:
  scripts/audit/public-tree-scrub.sh --index
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

OLD_VERSION_TEST = """//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.
"""

NEW_VERSION_TEST = """//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It
//! asserts the same version that the root package publishes.
"""


@dataclass(frozen=True)
class Rule:
    """One exact paragraph replacement in a staged file."""

    path: str
    old: str
    new: str

    @property
    def markers(self) -> tuple[str, ...]:
        """Non-empty lines that belong to the old or new paragraph."""
        lines = {
            line
            for block in (self.old, self.new)
            for line in block.splitlines()
            if line.strip()
        }
        return tuple(sorted(lines))


RULES = (Rule("scripts/release/version-test.rs", OLD_VERSION_TEST, NEW_VERSION_TEST),)


class ScrubError(Exception):
    """A staged paragraph is duplicated, mixed, or only partly present."""


def git(*args: str, input_bytes: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(["git", *args], input=input_bytes, check=True, stdout=subprocess.PIPE)


def scrub_text(current: str, rule: Rule) -> tuple[str | None, str]:
    """Return the updated text (or None when unchanged) and a status message.

    An exact old paragraph is rewritten. Zero copies is already clean, unless a
    known line from the old or new paragraph is left behind: that is a partial
    edit and raises ScrubError instead of passing as a no-op.
    """
    old_count = current.count(rule.old)
    new_count = current.count(rule.new)

    if old_count > 1:
        raise ScrubError(
            f"found {old_count} copies of the old paragraph in {rule.path}; expected at most one"
        )
    if new_count > 1:
        raise ScrubError(
            f"found {new_count} copies of the new paragraph in {rule.path}; expected at most one"
        )
    if old_count == 1 and new_count == 1:
        raise ScrubError(f"both old and new text in {rule.path}")

    updated = current.replace(rule.old, rule.new, 1) if old_count == 1 else current
    residual = updated.replace(rule.new, "", 1) if old_count or new_count else updated
    stale = [line for line in rule.markers if line in residual]
    if stale:
        shown = ", ".join(repr(line) for line in stale)
        raise ScrubError(f"partial paragraph in {rule.path}: {shown}")

    if old_count == 1:
        return updated, f"rewrote {rule.path}"
    if new_count == 1:
        return None, f"already scrubbed: {rule.path}"
    return None, f"already clean: {rule.path}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", action="store_true", help="scrub the staged index (default)")
    args = parser.parse_args()
    if not args.index:
        parser.error("only --index is supported")

    for rule in RULES:
        path = Path(rule.path)
        try:
            current = git("show", f":{path}").stdout.decode("utf-8")
        except subprocess.CalledProcessError:
            print(f"public-tree-scrub: missing staged path: {path}", file=sys.stderr)
            return 1
        except UnicodeDecodeError:
            print(f"public-tree-scrub: staged path is not UTF-8: {path}", file=sys.stderr)
            return 1

        try:
            updated, message = scrub_text(current, rule)
        except ScrubError as exc:
            print(f"public-tree-scrub: {exc}", file=sys.stderr)
            return 1

        if updated is not None:
            blob = (
                git("hash-object", "-w", "--stdin", input_bytes=updated.encode("utf-8"))
                .stdout.decode()
                .strip()
            )
            mode = git("ls-files", "-s", "--", str(path)).stdout.decode().split()[0]
            git("update-index", "--cacheinfo", f"{mode},{blob},{path}")

            check = git("show", f":{path}").stdout.decode("utf-8")
            if check != updated:
                print(f"public-tree-scrub: index verification failed for {path}", file=sys.stderr)
                return 1

        print(f"public-tree-scrub: {message}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
