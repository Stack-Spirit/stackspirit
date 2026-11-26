#!/bin/sh
# Generate and check the dependency licence table for VOWP.
#
# Runs `cargo metadata --format-version 1 --locked` (no network) and prints
# every third-party package with its licence expression. Anything that is not
# MIT, Apache-2.0, BSD, ISC, Unicode or Zlib is flagged. Also checks the
# workspace crates' `license = "MIT"` and the year/owner line in LICENSE.
#
# Usage:
#   scripts/audit/licenses.sh                 # human-readable table
#   scripts/audit/licenses.sh --markdown      # Markdown table
#   scripts/audit/licenses.sh --third-party   # attribution list for binaries
#
# Exit status: 0 when every licence is allowed, 1 when anything needs review,
# 2 when cargo metadata cannot be read.

set -u

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
root=$(CDPATH= cd -- "$script_dir/../.." && pwd -P) || exit 2

exec python3 "$script_dir/licenses.py" --root "$root" "$@"
