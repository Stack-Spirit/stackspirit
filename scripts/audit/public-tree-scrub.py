#!/usr/bin/env python3
"""Rewrite known build-process prose in the staged public tree.

The public branch is built from the private tree with a temporary index. A few
product files contain comments about the private build process that cannot be
pruned because the file itself ships. This script replaces the known paragraphs
with product-facing text in the index only; the private working tree is not
changed.

The replacement is exact. It fails when neither the old paragraph nor the new
paragraph is present, or when the old paragraph appears more than once, so a
future edit cannot silently leave build-process prose in the public tree. If
the new paragraph is already present, the script is a no-op.

Usage:
  scripts/audit/public-tree-scrub.sh --index
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

PATH = "scripts/release/version-test.rs"

OLD = """//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It is
//! the same check as `crates/vowp-cli/tests/version.rs`, which the packaging job does not own
//! and therefore cannot update with the release version.
"""

NEW = """//! The publishable `vowp-cli` package manifest lives at the workspace root (see the comment
//! in the root `Cargo.toml`), so this test is wired up as the `version` target there. It
//! asserts the same version that the root package publishes.
"""


def git(*args: str, input_bytes: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(["git", *args], input=input_bytes, check=True, stdout=subprocess.PIPE)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", action="store_true", help="scrub the staged index (default)")
    args = parser.parse_args()
    if not args.index:
        parser.error("only --index is supported")

    path = Path(PATH)
    try:
        current = git("show", f":{path}").stdout.decode("utf-8")
    except subprocess.CalledProcessError:
        print(f"public-tree-scrub: missing staged path: {path}", file=sys.stderr)
        return 1

    if NEW in current:
        if OLD in current:
            print(f"public-tree-scrub: both old and new text in {path}", file=sys.stderr)
            return 1
        print(f"public-tree-scrub: already scrubbed: {path}")
        return 0

    count = current.count(OLD)
    if count != 1:
        print(
            f"public-tree-scrub: expected exactly one copy of the old paragraph in {path}, found {count}",
            file=sys.stderr,
        )
        return 1

    updated = current.replace(OLD, NEW, 1)
    blob = git("hash-object", "-w", "--stdin", input_bytes=updated.encode("utf-8")).stdout.decode().strip()
    mode = git("ls-files", "-s", "--", str(path)).stdout.decode().split()[0]
    git("update-index", "--cacheinfo", f"{mode},{blob},{path}")

    check = git("show", f":{path}").stdout.decode("utf-8")
    if NEW not in check or OLD in check:
        print(f"public-tree-scrub: index verification failed for {path}", file=sys.stderr)
        return 1

    print(f"public-tree-scrub: rewrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
