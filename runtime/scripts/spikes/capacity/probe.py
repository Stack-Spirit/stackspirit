#!/usr/bin/env python3
"""Probe cgroup v2 capacity accounting on a Linux node.

Answers the questions in the S1 card:

* does `systemd-run --user --scope` work for this user?
* are `memory.peak`, `cpu.stat` and `MemoryHigh` available, and is the soft
  limit actually enforced?
* is `/proc` available for the fallback tree walk?

The probe is read-only with respect to VOWP state.  It creates short-lived
transient user scopes with `--collect`, never touches the default tmux server,
never calls a real harness and never uses the network.  It prints a readable
summary and can also write the same facts as JSON.

Usage:
    python3 probe.py [--json FILE]
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
from pathlib import Path
from typing import Any


CGROUP_ROOT = Path("/sys/fs/cgroup")
MIB = 1024 * 1024


def read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return None


def read_int(path: Path) -> int | None:
    text = read_text(path)
    try:
        return int(text) if text is not None else None
    except ValueError:
        return None


def cgroup_path_for_pid(pid: int) -> str | None:
    text = read_text(Path(f"/proc/{pid}/cgroup"))
    if text is None:
        return None
    for line in text.splitlines():
        parts = line.split(":", 2)
        if len(parts) == 3 and parts[0] == "0":
            return parts[2]
    return None


def run(argv: list[str], timeout: float = 30.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )


def systemd_run_version() -> str:
    result = run(["systemd-run", "--version"], timeout=5)
    return (result.stdout or result.stderr).splitlines()[0] if (result.stdout or result.stderr) else "unknown"


def probe_basic_scope(tmp: Path) -> dict[str, Any]:
    """Run one transient user scope and record its cgroup files."""
    payload = tmp / "scope_probe.py"
    payload.write_text(
        textwrap.dedent(
            """
            import json, os, time
            root = "/sys/fs/cgroup"
            text = open("/proc/self/cgroup", encoding="utf-8").read()
            path = next(line.split(":", 2)[2] for line in text.splitlines() if line.startswith("0::"))
            cg = root + path
            def read(name):
                try:
                    return open(cg + "/" + name, encoding="utf-8").read().strip()
                except OSError:
                    return None
            def stat():
                try:
                    return dict(line.split() for line in open(cg + "/cpu.stat", encoding="utf-8"))
                except OSError:
                    return None
            before = stat()
            buf = bytearray(32 * 1024 * 1024)
            for i in range(0, len(buf), 4096):
                buf[i] = 1
            deadline = time.monotonic() + 0.15
            n = 0
            while time.monotonic() < deadline:
                n = (n + 1) & 0xFFFFFFFF
            after = stat()
            print(json.dumps({
                "cgroup_path": path,
                "memory_peak": read("memory.peak"),
                "memory_high": read("memory.high"),
                "memory_max": read("memory.max"),
                "memory_events": read("memory.events"),
                "cpu_before": before,
                "cpu_after": after,
                "allocation_bytes": len(buf),
            }))
            """
        ),
        encoding="utf-8",
    )
    unit = f"vowp-cap-probe-scope-{os.getpid()}-{int(time.time() * 1000) % 100000}"
    result = run(
        [
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            f"--unit={unit}",
            sys.executable,
            str(payload),
        ],
        timeout=30,
    )
    parsed: dict[str, Any] = {
        "command": "systemd-run --user --scope",
        "exit_code": result.returncode,
        "stderr": result.stderr.strip(),
    }
    if result.returncode == 0 and result.stdout.strip():
        try:
            parsed.update(json.loads(result.stdout.strip().splitlines()[-1]))
        except json.JSONDecodeError:
            parsed["parse_error"] = result.stdout[-2000:]
    return parsed


def probe_memory_high(tmp: Path) -> dict[str, Any]:
    """Run a scope with MemoryHigh=64M and prove the soft limit throttles."""
    payload = tmp / "memory_high_probe.py"
    payload.write_text(
        textwrap.dedent(
            """
            import json, time
            root = "/sys/fs/cgroup"
            text = open("/proc/self/cgroup", encoding="utf-8").read()
            path = next(line.split(":", 2)[2] for line in text.splitlines() if line.startswith("0::"))
            cg = root + path
            def read(name):
                try:
                    return open(cg + "/" + name, encoding="utf-8").read().strip()
                except OSError:
                    return None
            high_before = read("memory.high")
            events_before = read("memory.events")
            buf = bytearray(96 * 1024 * 1024)
            for i in range(0, len(buf), 4096):
                buf[i] = 1
            # Give the kernel a moment to apply pressure and count it.
            time.sleep(0.5)
            print(json.dumps({
                "cgroup_path": path,
                "memory_high": high_before,
                "memory_high_after": read("memory.high"),
                "memory_peak": read("memory.peak"),
                "memory_events_before": events_before,
                "memory_events_after": read("memory.events"),
                "allocation_bytes": len(buf),
                "alive": True,
            }))
            """
        ),
        encoding="utf-8",
    )
    unit = f"vowp-cap-probe-high-{os.getpid()}-{int(time.time() * 1000) % 100000}"
    result = run(
        [
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            f"--unit={unit}",
            "-p",
            "MemoryHigh=64M",
            sys.executable,
            str(payload),
        ],
        timeout=30,
    )
    parsed: dict[str, Any] = {
        "command": "systemd-run --user --scope -p MemoryHigh=64M",
        "exit_code": result.returncode,
        "stderr": result.stderr.strip(),
    }
    if result.returncode == 0 and result.stdout.strip():
        try:
            parsed.update(json.loads(result.stdout.strip().splitlines()[-1]))
        except json.JSONDecodeError:
            parsed["parse_error"] = result.stdout[-2000:]
    return parsed


def memory_events_high(events: str | None) -> int | None:
    if not events:
        return None
    for line in events.splitlines():
        key, _, value = line.partition(" ")
        if key == "high":
            try:
                return int(value)
            except ValueError:
                return None
    return None


def host_facts() -> dict[str, Any]:
    meminfo: dict[str, int] = {}
    text = read_text(Path("/proc/meminfo")) or ""
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        value = rest.strip().split(" ")[0]
        if value.isdigit():
            meminfo[key] = int(value) * 1024

    pressure: dict[str, str] = {}
    for name in ("cpu", "memory", "io"):
        value = read_text(Path(f"/proc/pressure/{name}"))
        if value is not None:
            pressure[name] = value.replace("\n", "; ")

    load = read_text(Path("/proc/loadavg"))
    return {
        "os_release": next(
            (line.split("=", 1)[1].strip().strip('"') for line in read_text(Path("/etc/os-release")).splitlines() if line.startswith("PRETTY_NAME=")),
            platform.platform(),
        )
        if read_text(Path("/etc/os-release"))
        else platform.platform(),
        "kernel": platform.release(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "uid": os.getuid(),
        "user": os.environ.get("USER", ""),
        "mem_total_bytes": meminfo.get("MemTotal"),
        "mem_available_bytes": meminfo.get("MemAvailable"),
        "swap_total_bytes": meminfo.get("SwapTotal"),
        "swap_free_bytes": meminfo.get("SwapFree"),
        "nproc": os.cpu_count(),
        "loadavg": load,
        "pressure": pressure,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=Path, help="write the probe facts as JSON")
    args = parser.parse_args(argv)

    if not sys.platform.startswith("linux"):
        print("probe: this probe is Linux-only; no cgroup facts on this platform", file=sys.stderr)
        return 2

    cgroup_mount = read_text(Path("/proc/mounts")) or ""
    cgroup2 = any(line.split()[2] == "cgroup2" and line.split()[1] == "/sys/fs/cgroup" for line in cgroup_mount.splitlines() if len(line.split()) >= 3)
    own_cgroup = cgroup_path_for_pid(os.getpid())
    own_dir = CGROUP_ROOT / own_cgroup.lstrip("/") if own_cgroup else None

    with tempfile.TemporaryDirectory(prefix="vowp-cap-probe-") as tmp_name:
        tmp = Path(tmp_name)
        scope = probe_basic_scope(tmp)
        high = probe_memory_high(tmp)

    facts: dict[str, Any] = {
        "schema": 1,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": host_facts(),
        "cgroup": {
            "version": "v2" if cgroup2 else "unknown",
            "mount": cgroup2,
            "own_path": own_cgroup,
            "own_controllers": read_text(own_dir / "cgroup.controllers") if own_dir else None,
            "own_subtree_control": read_text(own_dir / "cgroup.subtree_control") if own_dir else None,
            "memory_peak_readable": (own_dir / "memory.peak").exists() if own_dir else False,
            "memory_high_exists": (own_dir / "memory.high").exists() if own_dir else False,
            "memory_max_exists": (own_dir / "memory.max").exists() if own_dir else False,
            "cpu_stat_readable": (own_dir / "cpu.stat").exists() if own_dir else False,
            "cpu_max": read_text(own_dir / "cpu.max") if own_dir else None,
        },
        "tools": {
            "systemd_run": shutil.which("systemd-run"),
            "systemd_run_version": systemd_run_version() if shutil.which("systemd-run") else None,
            "systemctl": shutil.which("systemctl"),
            "loginctl": shutil.which("loginctl"),
            "docker": shutil.which("docker"),
        },
        "scope": scope,
        "memory_high_test": high,
        "proc": {
            "procfs": Path("/proc/self/stat").exists(),
            "pressure_memory": Path("/proc/pressure/memory").exists(),
            "pressure_cpu": Path("/proc/pressure/cpu").exists(),
        },
    }

    checks = {
        "cgroup_v2": bool(cgroup2),
        "transient_user_scope": scope.get("exit_code") == 0,
        "memory_peak": bool(scope.get("memory_peak") and int(scope["memory_peak"]) >= 32 * MIB),
        "cpu_stat": bool(
            scope.get("cpu_before")
            and scope.get("cpu_after")
            and int(scope["cpu_after"].get("usage_usec", "0")) > int(scope["cpu_before"].get("usage_usec", "0"))
        ),
        "memory_high_property": high.get("exit_code") == 0
        and str(high.get("memory_high")) == str(64 * MIB),
        "memory_high_enforced": (memory_events_high(high.get("memory_events_after")) or 0) > 0,
        "proc_tree_walk": Path("/proc/self/stat").exists(),
    }
    facts["checks"] = checks
    facts["ok"] = all(checks.values())

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(facts, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print("capacity probe")
    print(f"  host:        {facts['host']['os_release']} / {facts['host']['kernel']} / {facts['host']['machine']}")
    print(f"  user:        {facts['host']['user']} (uid {facts['host']['uid']}), {facts['host']['nproc']} CPUs")
    print(f"  memory:      {facts['host']['mem_total_bytes'] and facts['host']['mem_total_bytes'] // MIB} MiB total, "
          f"{facts['host']['mem_available_bytes'] and facts['host']['mem_available_bytes'] // MIB} MiB available")
    print(f"  cgroup:      {facts['cgroup']['version']} mounted at /sys/fs/cgroup; own path {own_cgroup}")
    print(f"  systemd-run: {facts['tools']['systemd_run_version']}")
    print()
    for name, ok in checks.items():
        print(f"  {'ok  ' if ok else 'FAIL'} {name}")
    print()
    if scope.get("exit_code") == 0:
        print(f"  scope memory.peak: {scope.get('memory_peak')} bytes for a {scope.get('allocation_bytes')} byte allocation")
        print(f"  scope cpu.stat:    {scope.get('cpu_before')} -> {scope.get('cpu_after')}")
    else:
        print(f"  scope failed ({scope.get('exit_code')}): {scope.get('stderr')}")
    if high.get("exit_code") == 0:
        print(f"  MemoryHigh:        {high.get('memory_high')} bytes, events before/after: "
              f"{high.get('memory_events_before')!r} / {high.get('memory_events_after')!r}")
    else:
        print(f"  MemoryHigh test failed ({high.get('exit_code')}): {high.get('stderr')}")
    print()
    print(f"result: {'ok' if facts['ok'] else 'not all checks passed'}")
    return 0 if facts["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
