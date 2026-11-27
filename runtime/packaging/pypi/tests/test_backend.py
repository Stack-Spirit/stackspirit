import hashlib
import base64
import os
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

PKG = Path(__file__).resolve().parents[1] / "stackspirit"
sys.path.insert(0, str(PKG))
sys.dont_write_bytecode = True
import backend  # noqa: E402

TRIPLES = {v: k for k, v in backend.TARGET_TAGS.items()}


def make_bins(root, script="#!/bin/sh\necho fake\n"):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    for name in backend.BINARIES:
        p = root / name
        p.write_text(script)
        p.chmod(0o755)
    return root


def build(out, bins, tag, **extra):
    cfg = {"bin-dir": str(bins), "platform-tag": tag, "version": "0.6.0", **extra}
    return backend.build_wheel(str(out), cfg)


class BackendTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.tmp_path = Path(self.tmp.name)
        self.bins = make_bins(self.tmp_path / "bins")

    def test_distribution_name_and_version_map(self):
        m = backend.pep440
        self.assertEqual(m("0.6.0"), "0.6.0")
        self.assertEqual(m("1.2.3-alpha.4"), "1.2.3a4")
        self.assertEqual(m("1.2.3-beta.5"), "1.2.3b5")
        self.assertEqual(m("1.2.3-rc.6"), "1.2.3rc6")
        self.assertEqual(m("0.5.1-dev"), "0.5.1.dev0")
        self.assertTrue(backend.is_final("0.6.0"))
        for v in ("0.5.1-dev", "1.0.0-rc.1", "1.0", "x"):
            self.assertFalse(backend.is_final(v))
        with self.assertRaises(backend.BuildError):
            build(self.tmp_path, self.bins, "macosx_11_0_arm64", version="0.6.0-rc.1")
        meta = backend._metadata("0.6.0")
        self.assertIn("Name: stackspirit\n", meta)
        self.assertIn("Metadata-Version: 2.4", meta)
        self.assertIn("License-File: LICENSE\n", meta)
        self.assertNotIn("Private ::", meta)
        self.assertNotIn("stackspirit-engine", meta)

    def test_wheel_tag_matches_the_target(self):
        for tag, triple in TRIPLES.items():
            name = build(self.tmp_path, self.bins, tag, target=triple)
            self.assertEqual(name, f"stackspirit-0.6.0-py3-none-{tag}.whl")
            with zipfile.ZipFile(self.tmp_path / name) as z:
                wheel = z.read("stackspirit-0.6.0.dist-info/WHEEL").decode()
            self.assertIn(f"Tag: py3-none-{tag}\n", wheel)
            self.assertIn("Generator: stackspirit-build-wheel 0.6.0\n", wheel)
            self.assertIn("Root-Is-Purelib: false\n", wheel)
        with self.assertRaises(backend.BuildError):
            build(self.tmp_path, self.bins, "macosx_11_0_arm64", target="x86_64-apple-darwin")
        with self.assertRaises(backend.BuildError):
            build(self.tmp_path, self.bins, "win_amd64")

    def test_wheel_contains_all_four_binaries_and_no_source(self):
        name = build(self.tmp_path, self.bins, "manylinux_2_35_x86_64")
        d = "stackspirit-0.6.0.dist-info"
        expected = [
            "stackspirit/__init__.py", "stackspirit/launcher.py",
            "stackspirit/bin/stackspirit", "stackspirit/bin/vowp",
            "stackspirit/bin/stackspirit-daemon", "stackspirit/bin/vowp-daemon",
            f"{d}/METADATA", f"{d}/WHEEL", f"{d}/entry_points.txt",
            f"{d}/licenses/LICENSE", f"{d}/RECORD",
        ]
        with zipfile.ZipFile(self.tmp_path / name) as z:
            self.assertEqual(sorted(z.namelist()), sorted(expected))
            for info in z.infolist():
                self.assertEqual(info.date_time, (1980, 1, 1, 0, 0, 0))
                mode = stat.S_IMODE(info.external_attr >> 16)
                want = 0o755 if "/bin/" in info.filename else 0o644
                self.assertEqual(mode, want, info.filename)
            ep = z.read(f"{d}/entry_points.txt").decode()
            self.assertEqual(ep, backend.ENTRY_POINTS)

    def test_record_hashes_verify(self):
        name = build(self.tmp_path, self.bins, "macosx_10_12_x86_64")
        with zipfile.ZipFile(self.tmp_path / name) as z:
            record = z.read("stackspirit-0.6.0.dist-info/RECORD").decode().splitlines()
            seen = set()
            for line in record:
                path, h, size = line.rsplit(",", 2)
                seen.add(path)
                if path.endswith("/RECORD"):
                    self.assertEqual((h, size), ("", ""))
                    continue
                data = z.read(path)
                digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
                self.assertEqual(h, "sha256=" + digest)
                self.assertEqual(int(size), len(data))
            self.assertEqual(seen, set(z.namelist()))

    def test_refusals(self):
        (self.bins / "vowp-daemon").chmod(0o644)
        with self.assertRaisesRegex(backend.BuildError, "not executable"):
            build(self.tmp_path, self.bins, "macosx_11_0_arm64")
        (self.bins / "vowp-daemon").unlink()
        with self.assertRaisesRegex(backend.BuildError, "missing binary"):
            build(self.tmp_path, self.bins, "macosx_11_0_arm64")
        with self.assertRaisesRegex(backend.BuildError, "no sdist"):
            backend.build_sdist(str(self.tmp_path))

    def test_oversize_wheel_is_refused(self):
        old = backend.MAX_WHEEL_BYTES
        backend.MAX_WHEEL_BYTES = 100
        self.addCleanup(setattr, backend, "MAX_WHEEL_BYTES", old)
        with self.assertRaisesRegex(backend.BuildError, "64 MiB"):
            build(self.tmp_path, self.bins, "macosx_11_0_arm64")

    def test_build_is_reproducible(self):
        a, b = self.tmp_path / "a", self.tmp_path / "b"
        a.mkdir(), b.mkdir()
        na = build(a, self.bins, "manylinux_2_35_aarch64")
        os.utime(self.bins / "vowp", (1, 1))
        nb = build(b, self.bins, "manylinux_2_35_aarch64")
        self.assertEqual((a / na).read_bytes(), (b / nb).read_bytes())

    def test_unsupported_platform_has_no_wheel(self):
        self.assertEqual(set(backend.TAGS), {
            "manylinux_2_35_x86_64", "manylinux_2_35_aarch64",
            "macosx_10_12_x86_64", "macosx_11_0_arm64"})
        for bad in ("musllinux_1_2_x86_64", "win_amd64", "manylinux_2_35_i686"):
            with self.assertRaises(backend.BuildError):
                build(self.tmp_path, self.bins, bad)

    def test_smoke_wheel_has_local_version_and_no_binaries(self):
        name = backend.build_wheel(str(self.tmp_path), {})
        self.assertIn("+nobinaries", name)
        with zipfile.ZipFile(self.tmp_path / name) as z:
            self.assertFalse([n for n in z.namelist() if "/bin/" in n])

    def test_console_script_execs_the_binary(self):
        script = '#!/bin/sh\nprintf "%s|%s\\n" "$0" "$*"\n'
        bins = make_bins(self.tmp_path / "echo", script)
        code = ("import sys; sys.path.insert(0, sys.argv[1]);"
                "from stackspirit import launcher; sys.argv = ['x', 'a', 'b'];"
                "launcher.main_vowp()")
        env = {**os.environ, "STACKSPIRIT_BIN_DIR": str(bins)}
        r = subprocess.run([sys.executable, "-I", "-c", code, str(PKG)], env=env,
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(r.stdout.strip().endswith("|a b"), r.stdout)
        self.assertIn("vowp", r.stdout.split("|")[0])  # argv[0] is the command name


if __name__ == "__main__":
    unittest.main()
