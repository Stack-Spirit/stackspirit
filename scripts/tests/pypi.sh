#!/usr/bin/env bash
# Hermetic proof of the stackspirit PyPI wheels (design §4.6): fake binaries, temp venv, no network.
# Runs under a temporary HOME. Needs python3 with `build` and pip importable. `uv` is optional locally; set REQUIRE_UV=1 (CI) to fail without it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export PYTHONDONTWRITEBYTECODE=1

# Locate the python tooling (`build`, `pip` and their dependencies) while the caller's environment is
# still intact, then run everything under a temporary HOME. Only the importable code directories are
# added to PYTHONPATH (read-only use); caches, config and tool state all go under the temp home.
tool_path="$(python3 -P - <<'PYX' || true
import importlib.util, os
dirs = []
for mod in ("build", "pip", "pyproject_hooks", "packaging"):
    spec = importlib.util.find_spec(mod)
    if spec and spec.origin:
        d = os.path.dirname(os.path.dirname(spec.origin))
        if d not in dirs:
            dirs.append(d)
print(os.pathsep.join(dirs))
PYX
)"
export HOME="$work/home"
mkdir -p "$HOME/.cache" "$HOME/.config" "$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache" XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share"
export PIP_CACHE_DIR="$HOME/.cache/pip" UV_CACHE_DIR="$HOME/.cache/uv"
export PYTHONUSERBASE="$HOME/.local"
[ -z "$tool_path" ] || export PYTHONPATH="$tool_path"
python3 -c 'import build, pip' 2>/dev/null \
  || { echo "FAIL: python3 needs the 'build' and 'pip' modules importable from the caller's environment" >&2; exit 1; }
unset PIP_INDEX_URL PIP_EXTRA_INDEX_URL UV_INDEX_URL VIRTUAL_ENV

fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok: $*"; }

triples=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin)
tags=(manylinux_2_35_x86_64 manylinux_2_35_aarch64 macosx_10_12_x86_64 macosx_11_0_arm64)
names=(stackspirit vowp stackspirit-daemon vowp-daemon)
version=0.6.0

# Fake binaries: print argv[0], the arguments and the target they were built for.
for t in "${triples[@]}"; do
  d="$work/target/$t/release"; mkdir -p "$d"
  for n in "${names[@]}"; do
    printf '#!/bin/sh\nprintf "fake:%%s:%%s:%s\\n" "$(basename "$0")" "$*"\n' "$t" >"$d/$n"
    chmod 755 "$d/$n"
  done
done

build() { bash "$root/scripts/release/build-pypi-wheels.sh" --version "$version" --target-dir "$work/target" --out "$1" >/dev/null; }

build "$work/out1" || fail "build failed"
build "$work/out2" || fail "second build failed"

export PIP_CONFIG_FILE=/dev/null  # no user pip config

# 1. Exactly four wheels with the exact names, and no sdist.
actual="$(cd "$work/out1" && ls | sort)"
expected="$(for t in "${tags[@]}"; do echo "stackspirit-$version-py3-none-$t.whl"; done | sort)"
[ "$actual" = "$expected" ] || fail "wheel set differs:\n$actual"
ok "four wheels with exact names"

# 2. Members, tags, RECORD hashes and byte-identical rebuilds.
python3 -I - "$work/out1" "$work/out2" "$version" "${tags[@]}" <<'PY'
import base64, hashlib, stat, sys, zipfile
from pathlib import Path
out1, out2, version, *tags = sys.argv[1:]
d = f"stackspirit-{version}.dist-info"
want = sorted([
    "stackspirit/__init__.py", "stackspirit/launcher.py",
    "stackspirit/bin/stackspirit", "stackspirit/bin/vowp",
    "stackspirit/bin/stackspirit-daemon", "stackspirit/bin/vowp-daemon",
    f"{d}/METADATA", f"{d}/WHEEL", f"{d}/entry_points.txt",
    f"{d}/licenses/LICENSE", f"{d}/RECORD"])
for tag in tags:
    name = f"stackspirit-{version}-py3-none-{tag}.whl"
    a, b = Path(out1, name).read_bytes(), Path(out2, name).read_bytes()
    assert a == b, f"{name}: two builds differ"
    with zipfile.ZipFile(Path(out1, name)) as z:
        assert sorted(z.namelist()) == want, (name, z.namelist())
        assert f"Tag: py3-none-{tag}\n" in z.read(f"{d}/WHEEL").decode(), name
        for i in z.infolist():
            mode = stat.S_IMODE(i.external_attr >> 16)
            assert mode == (0o755 if "/bin/" in i.filename else 0o644), (name, i.filename, oct(mode))
            assert i.date_time == (1980, 1, 1, 0, 0, 0)
        meta = z.read(f"{d}/METADATA").decode()
        assert "Private ::" not in meta and "Name: stackspirit\n" in meta
        for line in z.read(f"{d}/RECORD").decode().splitlines():
            path, h, size = line.rsplit(",", 2)
            if path.endswith("/RECORD"):
                assert h == "" and size == ""
                continue
            data = z.read(path)
            exp = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
            assert h == "sha256=" + exp and int(size) == len(data), (name, path)
print("members, tags, modes, RECORD hashes verified; two builds byte-identical")
PY
ok "members, tags, RECORD hashes, reproducible"

# 3. Refusals: non-final version, sdist.
if bash "$root/scripts/release/build-pypi-wheels.sh" --version 0.6.0-rc.1 --target-dir "$work/target" --out "$work/bad" 2>"$work/err"; then
  fail "non-final version was accepted"
fi
grep -q "non-final" "$work/err" || fail "no non-final message"
if bash "$root/scripts/release/build-pypi-wheels.sh" --sdist 2>"$work/err"; then fail "sdist accepted"; fi
grep -q "sdist" "$work/err" || fail "no sdist message"
[ ! -e "$work/bad" ] || fail "refused build left output behind"
ok "non-final version and sdist refused"

# 4. pip install --no-index into a temp venv runs all four console scripts (host wheel only).
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) host_tag=manylinux_2_35_x86_64; host_triple=x86_64-unknown-linux-gnu ;;
  Linux-aarch64) host_tag=manylinux_2_35_aarch64; host_triple=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64) host_tag=macosx_10_12_x86_64; host_triple=x86_64-apple-darwin ;;
  Darwin-arm64) host_tag=macosx_11_0_arm64; host_triple=aarch64-apple-darwin ;;
  *) echo "skip: no wheel for this host"; exit 0 ;;
esac
python3 -m venv --without-pip "$work/venv"
wheels="$work/out1"
python3 -m pip --python "$work/venv/bin/python" install --no-index --find-links "$wheels" \
  --disable-pip-version-check --quiet "stackspirit==$version" || fail "offline pip install failed"
for n in "${names[@]}"; do
  got="$("$work/venv/bin/$n" --version x)"
  [ "$got" = "fake:$n:--version x:$host_triple" ] || fail "$n printed '$got'"
done
ok "pip install --no-index runs all four console scripts (argv[0] and args preserved)"

# 5. uvx-style run.
if command -v uv >/dev/null 2>&1; then
  got="$(UV_NO_CONFIG=1 uv tool run --offline --no-cache --from "$wheels/stackspirit-$version-py3-none-$host_tag.whl" stackspirit --version)" \
    || fail "uv tool run failed"
  [ "$got" = "fake:stackspirit:--version:$host_triple" ] || fail "uv printed '$got'"
  ok "uv tool run --from <wheel> stackspirit --version"
elif [ "${REQUIRE_UV:-0}" = 1 ]; then
  fail "uv is required (REQUIRE_UV=1) but not installed"
else
  echo "SKIP: uv is not installed; the uvx-style test did not run (CI sets REQUIRE_UV=1)"
fi

echo "pypi.sh: all checks passed"
