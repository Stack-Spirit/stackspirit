"""Offline tests for the launcher: fake binaries in a temporary directory."""

import os
import subprocess
import sys
import tempfile
try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
NAMES = ("stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon")


def run_entry(function: str, bin_dir: Path, *args: str) -> subprocess.CompletedProcess:
    env = {"PATH": os.environ.get("PATH", ""), "STACKSPIRIT_BIN_DIR": str(bin_dir)}
    code = f"import sys; sys.path.insert(0, {str(SRC)!r}); from stackspirit import launcher; launcher.{function}()"
    return subprocess.run(
        [sys.executable, "-I", "-c", code, *args],
        capture_output=True, text=True, env=env, timeout=30,
    )


class LauncherTest(unittest.TestCase):
    @unittest.skipIf(tomllib is None, "tomllib needs Python 3.11")
    def test_distribution_name_is_stackspirit(self):
        meta = tomllib.loads((ROOT / "pyproject.toml").read_text())
        self.assertEqual(meta["project"]["name"], "stackspirit")
        self.assertIn("Private :: Do Not Upload", meta["project"]["classifiers"])
        self.assertEqual(sorted(meta["project"]["scripts"]), sorted(NAMES))

    def test_each_entry_point_runs_its_own_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name in NAMES:
                path = Path(tmp) / name
                path.write_text(f"#!/bin/sh\nprintf '%s:%s\\n' \"$0\" \"$*\"\nexit 3\n")
                path.chmod(0o755)
            for name in NAMES:
                result = run_entry("main_" + name.replace("-", "_"), Path(tmp), "a", "b")
                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertTrue(result.stdout.endswith(f"{name}:a b\n") or f"/{name}:a b" in result.stdout, result.stdout)

    def test_missing_binary_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = run_entry("main_vowp", Path(tmp))
            self.assertEqual(result.returncode, 1)
            self.assertIn("vowp binary is not installed", result.stderr)


if __name__ == "__main__":
    unittest.main()
