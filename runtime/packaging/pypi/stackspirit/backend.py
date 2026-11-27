"""Minimal PEP 517 backend that packs prebuilt binaries into a platform wheel.

Config settings: ``bin-dir`` (directory holding the four binaries), ``platform-tag``,
optional ``target`` (Rust triple, checked against the tag) and optional ``version`` (semver,
default: ``pyproject.toml``). Without ``bin-dir`` a binary-less smoke wheel with the local
version ``+nobinaries`` is built; PyPI refuses local versions, so it can never be published.
"""

import base64
import hashlib
import io
import os
import re
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAME = "stackspirit"
BINARIES = ("stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon")
TARGET_TAGS = {
    "x86_64-unknown-linux-gnu": "manylinux_2_35_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_2_35_aarch64",
    "x86_64-apple-darwin": "macosx_10_12_x86_64",
    "aarch64-apple-darwin": "macosx_11_0_arm64",
}
TAGS = tuple(TARGET_TAGS.values())
MAX_WHEEL_BYTES = 64 * 1024 * 1024
ZIP_DATE = (1980, 1, 1, 0, 0, 0)
URLS = {
    "Homepage": "https://stackspirit.io",
    "Repository": "https://github.com/Stack-Spirit/stackspirit",
    "Issues": "https://github.com/Stack-Spirit/stackspirit/issues",
}
ENTRY_POINTS = (
    "[console_scripts]\n"
    "stackspirit = stackspirit.launcher:main_stackspirit\n"
    "vowp = stackspirit.launcher:main_vowp\n"
    "stackspirit-daemon = stackspirit.launcher:main_stackspirit_daemon\n"
    "vowp-daemon = stackspirit.launcher:main_vowp_daemon\n"
)
_SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.(\d+)|-(dev))?$")


class BuildError(Exception):
    """A refused or invalid build."""


def pep440(version):
    """Map ``X.Y.Z[-alpha.N|-beta.N|-rc.N|-dev]`` to its PEP 440 form."""
    m = _SEMVER.match(version)
    if not m:
        raise BuildError(f"unsupported version: {version!r}")
    major, minor, patch, kind, num, dev = m.groups()
    base = f"{major}.{minor}.{patch}"
    if dev:
        return base + ".dev0"
    if kind:
        return base + {"alpha": "a", "beta": "b", "rc": "rc"}[kind] + num
    return base


def is_final(version):
    """True only for a plain ``X.Y.Z`` version."""
    m = _SEMVER.match(version)
    return bool(m) and m.group(4) is None and m.group(6) is None


def _setting(config, key):
    value = (config or {}).get(key)
    if isinstance(value, (list, tuple)):
        value = value[-1] if value else None
    return value or None


def _project_version():
    text = (HERE / "pyproject.toml").read_text(encoding="utf-8")
    m = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
    if not m:
        raise BuildError("pyproject.toml has no version")
    return m.group(1)


def _resolve(config):
    version = _setting(config, "version") or _project_version()
    bin_dir = _setting(config, "bin-dir")
    tag = _setting(config, "platform-tag")
    target = _setting(config, "target")
    if not is_final(version):
        raise BuildError(f"refusing non-final version {version!r}; only X.Y.Z is publishable")
    if bin_dir is None:
        return version, pep440(version) + "+nobinaries", None, "any"
    if tag is None:
        raise BuildError("platform-tag is required with bin-dir")
    if tag not in TAGS:
        raise BuildError(f"unsupported platform tag {tag!r}; supported: {', '.join(TAGS)}")
    if target is not None and TARGET_TAGS.get(target) != tag:
        raise BuildError(f"platform tag {tag!r} does not match target {target!r}")
    return version, pep440(version), Path(bin_dir), tag


def _metadata(pyver):
    readme = (HERE / "README.md").read_text(encoding="utf-8")
    lines = [
        "Metadata-Version: 2.4",
        f"Name: {NAME}",
        f"Version: {pyver}",
        "Summary: StackSpirit: local-first supervisor for AI coding agents",
        "Author: Syver-August Meyer",
        "License-Expression: MIT",
        "License-File: licenses/LICENSE",
        "Requires-Python: >=3.9",
        "Classifier: Development Status :: 4 - Beta",
        "Classifier: Environment :: Console",
        "Classifier: Programming Language :: Python :: 3",
        "Classifier: Programming Language :: Rust",
    ]
    lines += [f"Project-URL: {k}, {v}" for k, v in URLS.items()]
    lines += ["Description-Content-Type: text/markdown", "", readme]
    return "\n".join(lines)


def _wheel_file(version, tag):
    return (
        "Wheel-Version: 1.0\n"
        f"Generator: stackspirit-build-wheel {version}\n"
        "Root-Is-Purelib: false\n"
        f"Tag: py3-none-{tag}\n"
    )


def _dist_info(pyver):
    return f"{NAME}-{pyver}.dist-info"


def _license():
    return (HERE / "LICENSE").read_bytes()


def get_requires_for_build_wheel(config_settings=None):
    """No build requirements."""
    return []


def prepare_metadata_for_build_wheel(metadata_directory, config_settings=None):
    """Write the ``.dist-info`` directory and return its name."""
    version, pyver, _bin, tag = _resolve(config_settings)
    di = Path(metadata_directory) / _dist_info(pyver)
    (di / "licenses").mkdir(parents=True, exist_ok=True)
    (di / "METADATA").write_text(_metadata(pyver), encoding="utf-8")
    (di / "WHEEL").write_text(_wheel_file(version, tag), encoding="utf-8")
    (di / "entry_points.txt").write_text(ENTRY_POINTS, encoding="utf-8")
    (di / "licenses" / "LICENSE").write_bytes(_license())
    return di.name


def build_sdist(sdist_directory, config_settings=None):
    """Refused: there is no source distribution."""
    raise BuildError("stackspirit has no sdist; build wheels only (--wheel)")


def _hash(data):
    digest = hashlib.sha256(data).digest()
    return "sha256=" + base64.urlsafe_b64encode(digest).rstrip(b"=").decode()


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    """Build ``stackspirit-<pep440>-py3-none-<tag>.whl`` and return its file name."""
    version, pyver, bin_dir, tag = _resolve(config_settings)
    init = (HERE / NAME / "__init__.py").read_text(encoding="utf-8")
    files = [  # (arcname, bytes, mode)
        (f"{NAME}/__init__.py", (init.rstrip("\n") + f'\n\n__version__ = "{pyver}"\n').encode(), 0o644),
        (f"{NAME}/launcher.py", (HERE / NAME / "launcher.py").read_bytes(), 0o644),
    ]
    if bin_dir is not None:
        for name in BINARIES:
            path = bin_dir / name
            if not path.is_file():
                raise BuildError(f"missing binary: {path}")
            if not os.access(path, os.X_OK):
                raise BuildError(f"binary is not executable: {path}")
            files.append((f"{NAME}/bin/{name}", path.read_bytes(), 0o755))
    di = _dist_info(pyver)
    files += [
        (f"{di}/METADATA", _metadata(pyver).encode(), 0o644),
        (f"{di}/WHEEL", _wheel_file(version, tag).encode(), 0o644),
        (f"{di}/entry_points.txt", ENTRY_POINTS.encode(), 0o644),
        (f"{di}/licenses/LICENSE", _license(), 0o644),
    ]
    record = "".join(f"{n},{_hash(d)},{len(d)}\n" for n, d, _ in files) + f"{di}/RECORD,,\n"
    files.append((f"{di}/RECORD", record.encode(), 0o644))

    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for arcname, data, mode in files:
            info = zipfile.ZipInfo(arcname, ZIP_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = (0o100000 | mode) << 16
            zf.writestr(info, data, compresslevel=9)
    blob = buf.getvalue()
    if len(blob) > MAX_WHEEL_BYTES:
        raise BuildError(f"wheel is {len(blob)} bytes, over the 64 MiB limit")
    filename = f"{NAME}-{pyver}-py3-none-{tag}.whl"
    Path(wheel_directory, filename).write_bytes(blob)
    return filename


if __name__ == "__main__":
    sys.exit("backend.py is a PEP 517 backend; use `python3 -m build --wheel`")
