#!/usr/bin/env bash
# Build the static VOWP site into site/dist with the stdlib-only generator.
set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$here/build.py" "$@"
