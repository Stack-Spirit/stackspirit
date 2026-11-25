"""Console-script entry points that exec the vendored StackSpirit binaries."""

import os
import sys
from pathlib import Path

NAMES = ("stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon")


def bin_dir() -> Path:
    """The directory holding the binaries: STACKSPIRIT_BIN_DIR, else the package's bin/."""
    override = os.environ.get("STACKSPIRIT_BIN_DIR")
    if override:
        return Path(override)
    return Path(__file__).resolve().parent / "bin"


def run(name: str) -> None:
    """Replace this process with the binary `name`, passing the arguments through."""
    if name not in NAMES:
        raise SystemExit(f"stackspirit: unknown binary: {name}")
    binary = bin_dir() / name
    if not binary.is_file():
        sys.stderr.write(f"stackspirit: the {name} binary is not installed at {binary}\n")
        raise SystemExit(1)
    # argv[0] is the command name so `vowp` keeps its identity.
    os.execv(str(binary), [name, *sys.argv[1:]])


def main_stackspirit() -> None:
    run("stackspirit")


def main_vowp() -> None:
    run("vowp")


def main_stackspirit_daemon() -> None:
    run("stackspirit-daemon")


def main_vowp_daemon() -> None:
    run("vowp-daemon")
