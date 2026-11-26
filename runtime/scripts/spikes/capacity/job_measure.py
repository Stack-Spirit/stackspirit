#!/usr/bin/env python3
"""Measure one capacity-spike job with cgroup v2 and with a /proc tree walk.

One "job" here is the shape of a VOWP worker run: a harness process holds a
small resident set while the job's command gate (`cargo test --workspace`)
runs underneath it.  Both methods sample the same job at the same time, so the
result is a direct comparison rather than two separate experiments.

Cgroup method
    The script is expected to run inside a transient user scope created by
    `run_one.sh` (`systemd-run --user --scope`).  It creates three child
    cgroups inside that scope -- `main`, `harness` and `build` -- and starts
    each process in the matching child.  The kernel then gives exact
    `memory.peak` and `cpu.stat` values for each phase and for the whole job.
    For a build that the night-shift cargo shim runs in Docker, the container's
    own systemd cgroup is sampled too, because it is not a descendant of the
    job scope.

/proc method
    Every sample the script walks the job's process tree, sums `VmRSS` and the
    `utime`/`stime` deltas of every process in it.  This is the fallback VOWP
    needs when user cgroups are not delegated: it needs no privileges but misses
    processes that reparent away (or live in a container) and sums shared pages
    more than once.  The walk follows `/proc/<pid>/task/<pid>/children`, so its
    own cost does not grow with the number of unrelated processes on the host.

The script never uses the network, a real harness or the default tmux server.
The harness is `fake_harness.py`; the caller owns the temporary `TMUX_TMPDIR`
and `VOWP_HOME`.

Usage:
    run_one.sh python3 job_measure.py --label host-1 --cargo-mode host \
        --out results/host-1.json --log results/host-1.log
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

CGROUP_ROOT = Path("/sys/fs/cgroup")
MIB = 1024 * 1024
PROC = Path("/proc")


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


def read_pairs(path: Path) -> dict[str, int]:
    result: dict[str, int] = {}
    text = read_text(path)
    if text is None:
        return result
    for line in text.splitlines():
        key, _, value = line.partition(" ")
        if value.strip().lstrip("-").isdigit():
            result[key] = int(value.strip())
    return result


def cgroup_path_for_pid(pid: int) -> str | None:
    text = read_text(PROC / str(pid) / "cgroup")
    if text is None:
        return None
    for line in text.splitlines():
        parts = line.split(":", 2)
        if len(parts) == 3 and parts[0] == "0":
            return parts[2]
    return None


def cgroup_dir(path: str | None) -> Path | None:
    if not path:
        return None
    return CGROUP_ROOT / path.lstrip("/")


def cgroup_members(path: str | None) -> set[int]:
    directory = cgroup_dir(path)
    if directory is None:
        return set()
    text = read_text(directory / "cgroup.procs")
    if text is None:
        return set()
    members: set[int] = set()
    for line in text.splitlines():
        if line.isdigit():
            members.add(int(line))
    return members


def cgroup_stats(path: str | None) -> dict[str, Any]:
    """Read the files a cgroup v2 job scope exposes, tolerating races."""
    directory = cgroup_dir(path)
    if directory is None:
        return {"readable": False, "path": path}
    cpu = read_pairs(directory / "cpu.stat")
    return {
        "readable": directory.is_dir(),
        "path": path,
        "current_bytes": read_int(directory / "memory.current"),
        "peak_bytes": read_int(directory / "memory.peak"),
        "high_bytes": read_int(directory / "memory.high"),
        "max_bytes": read_int(directory / "memory.max"),
        "cpu_usage_usec": cpu.get("usage_usec"),
        "cpu_user_usec": cpu.get("user_usec"),
        "cpu_system_usec": cpu.get("system_usec"),
        "memory_stat": read_pairs(directory / "memory.stat"),
        "memory_events": read_pairs(directory / "memory.events"),
    }


def cgroup_sample(path: str | None) -> dict[str, Any]:
    """Read only the cheap files needed on every sample."""
    directory = cgroup_dir(path)
    if directory is None:
        return {"current_bytes": 0, "anon_bytes": 0, "kernel_bytes": 0, "cpu_usage_usec": 0}
    stat = read_pairs(directory / "memory.stat")
    cpu = read_pairs(directory / "cpu.stat")
    return {
        "current_bytes": read_int(directory / "memory.current") or 0,
        "anon_bytes": stat.get("anon", 0),
        "kernel_bytes": stat.get("kernel", 0),
        "cpu_usage_usec": cpu.get("usage_usec", 0),
    }


def parse_proc_stat(text: str) -> tuple[int, int, int] | None:
    """Return (ppid, utime_ticks, stime_ticks) from /proc/<pid>/stat."""
    close = text.rfind(")")
    if close < 0:
        return None
    fields = text[close + 2 :].split()
    if len(fields) < 13:
        return None
    try:
        return int(fields[1]), int(fields[11]), int(fields[12])
    except ValueError:
        return None


def parse_status_rss(text: str) -> int | None:
    for line in text.splitlines():
        if line.startswith("VmRSS:"):
            parts = line.split()
            if len(parts) >= 2 and parts[1].isdigit():
                return int(parts[1]) * 1024
    return None


def children_of(pid: int) -> list[int]:
    text = read_text(PROC / str(pid) / "task" / str(pid) / "children")
    if text is None:
        return []
    return [int(value) for value in text.split() if value.isdigit()]


def walk_tree(root: int) -> set[int]:
    """Return root and its descendants using the kernel's children lists."""
    seen: set[int] = set()
    stack = [root]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(children_of(pid))
    return seen


def all_pids() -> list[int]:
    return [int(entry) for entry in os.listdir(PROC) if entry.isdigit()]


def read_proc_metrics(pid: int) -> dict[str, Any] | None:
    base = PROC / str(pid)
    try:
        stat_text = (base / "stat").read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    parsed = parse_proc_stat(stat_text)
    if parsed is None:
        return None
    ppid, utime, stime = parsed
    try:
        status_text = (base / "status").read_text(encoding="utf-8", errors="replace")
    except OSError:
        status_text = ""
    return {
        "ppid": ppid,
        "rss_bytes": parse_status_rss(status_text) or 0,
        "cpu_ticks": utime + stime,
    }


def descendants_from_metrics(metrics: dict[int, dict[str, Any]], root: int) -> set[int]:
    children: dict[int, list[int]] = {}
    for pid, info in metrics.items():
        children.setdefault(int(info["ppid"]), []).append(pid)
    seen: set[int] = set()
    stack = [root]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        stack.extend(children.get(pid, []))
    return seen


def under(path: str | None, directory: str | None) -> bool:
    if not path or not directory:
        return False
    return path == directory or path.startswith(directory.rstrip("/") + "/")


class CgroupSet:
    """The child cgroups used to separate harness, build and observer."""

    def __init__(self) -> None:
        self.scope_path: str | None = None
        self.scope_dir: Path | None = None
        self.nested = False
        self.main_path: str | None = None
        self.harness_path: str | None = None
        self.build_path: str | None = None
        self.error: str | None = None

    def setup(self) -> bool:
        if os.environ.get("CAP_SCOPE") != "1":
            self.error = "not started by run_one.sh inside a transient user scope"
            return False
        self.scope_path = cgroup_path_for_pid(os.getpid())
        self.scope_dir = cgroup_dir(self.scope_path)
        if self.scope_dir is None or not self.scope_dir.is_dir():
            self.error = "no cgroup v2 directory for this process"
            return False
        try:
            main = self.scope_dir / "main"
            main.mkdir(exist_ok=True)
            # Move the observer out of the scope root so the root can enable
            # controllers; a cgroup with processes cannot become a subtree root.
            (main / "cgroup.procs").write_text(f"{os.getpid()}\n", encoding="utf-8")
            (self.scope_dir / "cgroup.subtree_control").write_text("+memory +pids\n", encoding="utf-8")
            for name in ("harness", "build"):
                (self.scope_dir / name).mkdir(exist_ok=True)
            self.main_path = f"{self.scope_path}/main"
            self.harness_path = f"{self.scope_path}/harness"
            self.build_path = f"{self.scope_path}/build"
            self.nested = True
            return True
        except OSError as error:
            self.error = f"nested cgroups unavailable: {error}"
            return False

    def launch(self, component: str, argv: list[str], env: dict[str, str], log: Any) -> subprocess.Popen[bytes]:
        target = {
            "harness": self.harness_path,
            "build": self.build_path,
            "main": self.main_path,
        }.get(component)
        if not self.nested or not target:
            return subprocess.Popen(argv, env=env, stdout=log, stderr=subprocess.STDOUT)
        # The tiny shell moves itself into the child cgroup and then execs the
        # real command, so the command's first instruction already runs there.
        launcher = [
            "/bin/sh",
            "-c",
            'echo $$ > "$1"; shift; exec "$@"',
            "sh",
            str(cgroup_dir(target) / "cgroup.procs"),
            *argv,
        ]
        return subprocess.Popen(launcher, env=env, stdout=log, stderr=subprocess.STDOUT)


class ContainerWatcher(threading.Thread):
    """Follow the Docker container the night-shift cargo shim starts, if any.

    The shim labels its container `vowp.shimpid=<shim pid>`.  Docker creates the
    container in `system.slice`, outside the job's user scope, so VOWP's cgroup
    method would otherwise miss the whole Rust build on this build host.
    """

    def __init__(self, build_pid: int) -> None:
        super().__init__(name="capacity-container-watcher", daemon=True)
        self.build_pid = build_pid
        self.stop_event = threading.Event()
        self.container_id: str | None = None
        self.init_pid: int | None = None
        self.path: str | None = None
        self.peak_bytes: int | None = None
        self.current_bytes: int | None = None
        self.cpu_usage_usec: int | None = None
        self.memory_stat: dict[str, int] = {}
        self.memory_events: dict[str, int] = {}
        self.gone = False
        self.last_lookup = 0.0

    def run(self) -> None:
        while not self.stop_event.is_set():
            now = time.monotonic()
            if self.container_id is None and now - self.last_lookup >= 0.2:
                self.last_lookup = now
                self._find_container()
            if self.container_id is not None and self.path is None:
                self._find_cgroup()
            if self.path is not None:
                stats = cgroup_stats(self.path)
                if not stats.get("readable"):
                    self.gone = True
                else:
                    peak = stats.get("peak_bytes")
                    if isinstance(peak, int):
                        self.peak_bytes = max(self.peak_bytes or 0, peak)
                    current = stats.get("current_bytes")
                    if isinstance(current, int):
                        self.current_bytes = current
                    usage = stats.get("cpu_usage_usec")
                    if isinstance(usage, int):
                        self.cpu_usage_usec = max(self.cpu_usage_usec or 0, usage)
                    stat = stats.get("memory_stat")
                    if isinstance(stat, dict):
                        self.memory_stat = stat
                    events = stats.get("memory_events")
                    if isinstance(events, dict):
                        self.memory_events = events
            self.stop_event.wait(0.05)

    def _find_container(self) -> None:
        try:
            result = subprocess.run(
                [
                    "docker",
                    "ps",
                    "--no-trunc",
                    "--filter",
                    f"label=vowp.shimpid={self.build_pid}",
                    "--format",
                    "{{.ID}}",
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                timeout=3,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return
        lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
        if lines:
            self.container_id = lines[0]

    def _find_cgroup(self) -> None:
        assert self.container_id is not None
        try:
            result = subprocess.run(
                ["docker", "inspect", "-f", "{{.State.Pid}}", self.container_id],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                timeout=3,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return
        text = result.stdout.strip()
        if not text.isdigit():
            return
        init_pid = int(text)
        if init_pid <= 1:
            return
        path = cgroup_path_for_pid(init_pid)
        if path:
            self.init_pid = init_pid
            self.path = path

    def stats(self) -> dict[str, Any]:
        return {
            "id": self.container_id,
            "init_pid": self.init_pid,
            "cgroup_path": self.path,
            "peak_bytes": self.peak_bytes,
            "current_bytes": self.current_bytes,
            "cpu_usage_usec": self.cpu_usage_usec,
            "memory_stat": self.memory_stat,
            "memory_events": self.memory_events,
            "cgroup_gone": self.gone,
        }


def read_meminfo() -> dict[str, int]:
    result: dict[str, int] = {}
    text = read_text(PROC / "meminfo")
    if text is None:
        return result
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        parts = rest.split()
        if parts and parts[0].isdigit():
            result[key] = int(parts[0]) * 1024
    return result


def host_snapshot() -> dict[str, Any]:
    meminfo = read_meminfo()
    pressure: dict[str, str] = {}
    for name in ("cpu", "memory", "io"):
        value = read_text(PROC / "pressure" / name)
        if value is not None:
            pressure[name] = value.replace("\n", "; ")
    return {
        "mem_total_bytes": meminfo.get("MemTotal"),
        "mem_available_bytes": meminfo.get("MemAvailable"),
        "swap_total_bytes": meminfo.get("SwapTotal"),
        "swap_free_bytes": meminfo.get("SwapFree"),
        "loadavg": read_text(PROC / "loadavg"),
        "pressure": pressure,
        "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }


def build_env(cargo_mode: str) -> dict[str, str]:
    env = dict(os.environ)
    env["TERM"] = "dumb"
    env["CARGO_TERM_COLOR"] = "never"
    if cargo_mode == "host":
        env["NIGHT_CARGO_HOST"] = "1"
    else:
        env.pop("NIGHT_CARGO_HOST", None)
    return env


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--cargo-mode", choices=("shim", "host"), default="shim")
    parser.add_argument("--build-cmd", default="cargo test --workspace")
    parser.add_argument("--harness-mib", type=int, default=192)
    parser.add_argument("--harness-cpu-ms", type=int, default=400)
    parser.add_argument("--sample-ms", type=int, default=100)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    args = parser.parse_args(argv)

    import shlex

    build_argv = shlex.split(args.build_cmd)
    if not build_argv:
        parser.error("--build-cmd is empty")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.log.parent.mkdir(parents=True, exist_ok=True)

    result: dict[str, Any] = {
        "schema": 2,
        "label": args.label,
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "cargo_mode": args.cargo_mode,
        "build_cmd": build_argv,
        "harness": {"mib": args.harness_mib, "cpu_ms": args.harness_cpu_ms},
        "sample_interval_ms": args.sample_ms,
        "cwd": str(args.cwd),
        "host_before": host_snapshot(),
        "errors": [],
    }

    cgroups = CgroupSet()
    nested = cgroups.setup()
    result["cgroup_setup"] = {
        "nested": nested,
        "scope_path": cgroups.scope_path,
        "error": cgroups.error,
    }

    harness_proc: subprocess.Popen[bytes] | None = None
    build_proc: subprocess.Popen[bytes] | None = None
    watcher: ContainerWatcher | None = None
    ready = args.out.with_suffix(args.out.suffix + ".harness-ready")
    stop = args.out.with_suffix(args.out.suffix + ".harness-stop")
    for path in (ready, stop):
        try:
            path.unlink()
        except FileNotFoundError:
            pass

    env = build_env(args.cargo_mode)
    env["VOWP_CAPACITY_LABEL"] = args.label
    clk_tck = os.sysconf("SC_CLK_TCK")

    # /proc accounting state.
    last_cpu: dict[int, int] = {}
    job_cpu_usec = 0
    observer_cpu_usec = 0
    container_cpu_usec = 0
    job_peak_rss = 0
    observer_peak_rss = 0
    container_peak_rss = 0
    harness_peak_rss = 0
    build_peak_rss = 0
    combined_peak_rss = 0
    samples = 0

    # Cgroup sampled state.  The component sum excludes the observer.
    max_component_current = 0
    max_component_anon = 0
    max_component_kernel = 0
    max_scope_current = 0
    max_container_current = 0
    max_combined_current = 0

    job_start = time.monotonic()
    build_duration = 0.0
    try:
        with open(args.log, "wb") as log:
            log.write(f"# capacity spike job {args.label}\n".encode())
            log.write(f"# build: {' '.join(build_argv)}\n".encode())
            log.flush()
            harness_argv = [
                sys.executable,
                str(Path(__file__).with_name("fake_harness.py")),
                "--mib",
                str(args.harness_mib),
                "--cpu-ms",
                str(args.harness_cpu_ms),
                "--ready",
                str(ready),
                "--stop",
                str(stop),
            ]
            harness_proc = cgroups.launch("harness", harness_argv, env, log)
            deadline = time.monotonic() + 20
            while not ready.exists():
                if harness_proc.poll() is not None:
                    raise RuntimeError(f"harness exited before it was ready ({harness_proc.returncode})")
                if time.monotonic() > deadline:
                    raise RuntimeError("harness did not become ready within 20 s")
                time.sleep(0.02)

            build_proc = cgroups.launch("build", build_argv, env, log)
            if args.cargo_mode == "shim":
                watcher = ContainerWatcher(build_proc.pid)
                watcher.start()

            controller_pid = os.getpid()
            while build_proc.poll() is None:
                now = time.monotonic()

                main_members = cgroup_members(cgroups.main_path) if cgroups.nested else {controller_pid}
                harness_members = cgroup_members(cgroups.harness_path) if cgroups.nested else set()
                build_members = cgroup_members(cgroups.build_path) if cgroups.nested else set()
                container_members = cgroup_members(watcher.path) if watcher and watcher.path else set()

                host_pids = walk_tree(controller_pid)
                host_pids |= main_members | harness_members | build_members
                container_pids = set(container_members)
                if watcher is not None and watcher.init_pid is not None:
                    container_pids |= walk_tree(watcher.init_pid)

                metrics: dict[int, dict[str, Any]] = {}
                for pid in host_pids | container_pids:
                    info = read_proc_metrics(pid)
                    if info is not None:
                        metrics[pid] = info

                if not cgroups.nested:
                    harness_subtree = descendants_from_metrics(metrics, harness_proc.pid) if harness_proc else set()
                    build_subtree = descendants_from_metrics(metrics, build_proc.pid) if build_proc else set()
                else:
                    harness_subtree = set()
                    build_subtree = set()

                observer_pids = {controller_pid} | main_members
                job_rss = 0
                observer_rss = 0
                container_rss = 0
                component_rss = {"harness": 0, "build": 0}
                for pid in host_pids:
                    info = metrics.get(pid)
                    if info is None:
                        continue
                    rss = info["rss_bytes"]
                    if pid in observer_pids:
                        observer_rss += rss
                    else:
                        job_rss += rss
                        if pid in harness_members or (not cgroups.nested and pid in harness_subtree):
                            component_rss["harness"] += rss
                        elif pid in build_members or (not cgroups.nested and pid in build_subtree):
                            component_rss["build"] += rss
                    cpu = info["cpu_ticks"]
                    previous = last_cpu.get(pid)
                    delta = cpu if previous is None else max(cpu - previous, 0)
                    last_cpu[pid] = cpu
                    if pid in observer_pids:
                        observer_cpu_usec += delta * 1_000_000 // clk_tck
                    else:
                        job_cpu_usec += delta * 1_000_000 // clk_tck
                for pid in container_pids:
                    info = metrics.get(pid)
                    if info is None:
                        continue
                    container_rss += info["rss_bytes"]
                    cpu = info["cpu_ticks"]
                    previous = last_cpu.get(pid)
                    delta = cpu if previous is None else max(cpu - previous, 0)
                    last_cpu[pid] = cpu
                    container_cpu_usec += delta * 1_000_000 // clk_tck

                job_peak_rss = max(job_peak_rss, job_rss)
                observer_peak_rss = max(observer_peak_rss, observer_rss)
                container_peak_rss = max(container_peak_rss, container_rss)
                harness_peak_rss = max(harness_peak_rss, component_rss["harness"])
                build_peak_rss = max(build_peak_rss, component_rss["build"])
                combined_peak_rss = max(combined_peak_rss, job_rss + container_rss)

                scope_sample = cgroup_sample(cgroups.scope_path)
                harness_sample = cgroup_sample(cgroups.harness_path) if cgroups.nested else {"current_bytes": 0, "anon_bytes": 0, "kernel_bytes": 0}
                build_sample = cgroup_sample(cgroups.build_path) if cgroups.nested else {"current_bytes": 0, "anon_bytes": 0, "kernel_bytes": 0}
                container_sample = (
                    cgroup_sample(watcher.path) if watcher and watcher.path else {"current_bytes": 0, "anon_bytes": 0, "kernel_bytes": 0}
                )
                scope_current = scope_sample["current_bytes"]
                container_current = container_sample["current_bytes"]
                component_current = harness_sample["current_bytes"] + build_sample["current_bytes"] + container_current
                component_anon = harness_sample["anon_bytes"] + build_sample["anon_bytes"] + container_sample["anon_bytes"]
                component_kernel = harness_sample["kernel_bytes"] + build_sample["kernel_bytes"] + container_sample["kernel_bytes"]
                max_scope_current = max(max_scope_current, scope_current)
                max_container_current = max(max_container_current, container_current)
                max_combined_current = max(max_combined_current, scope_current + container_current)
                max_component_current = max(max_component_current, component_current)
                max_component_anon = max(max_component_anon, component_anon)
                max_component_kernel = max(max_component_kernel, component_kernel)
                samples += 1

                remaining = args.sample_ms / 1000.0 - (time.monotonic() - now)
                if remaining > 0:
                    time.sleep(remaining)

            build_rc = build_proc.wait()
            build_duration = time.monotonic() - job_start
        result["build_exit_code"] = build_rc
        if build_rc != 0:
            result["errors"].append(f"build command exited {build_rc}")
    except Exception as error:  # noqa: BLE001 - the spike records the failure
        result["errors"].append(f"{type(error).__name__}: {error}")
        result["build_exit_code"] = None
        build_duration = time.monotonic() - job_start
    finally:
        if build_proc is not None and build_proc.poll() is None:
            build_proc.terminate()
            try:
                build_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                build_proc.kill()
        if watcher is not None:
            watcher.stop_event.set()
            watcher.join(timeout=5)
        stop.write_text("stop\n", encoding="utf-8")
        if harness_proc is not None:
            try:
                harness_proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                harness_proc.terminate()
                try:
                    harness_proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    harness_proc.kill()
        try:
            stop.unlink()
            ready.unlink()
        except FileNotFoundError:
            pass

    job_duration = time.monotonic() - job_start
    result["duration_s"] = round(job_duration, 3)
    result["build_duration_s"] = round(build_duration, 3)
    result["host_after"] = host_snapshot()
    result["proc"] = {
        "samples": samples,
        "job_peak_rss_bytes": job_peak_rss,
        "observer_peak_rss_bytes": observer_peak_rss,
        "harness_peak_rss_bytes": harness_peak_rss,
        "build_peak_rss_bytes": build_peak_rss,
        "container_peak_rss_bytes": container_peak_rss,
        "combined_peak_rss_bytes": combined_peak_rss,
        "job_cpu_usec": job_cpu_usec,
        "observer_cpu_usec": observer_cpu_usec,
        "container_cpu_usec": container_cpu_usec,
        "combined_cpu_usec": job_cpu_usec + container_cpu_usec,
    }

    scope_stats = cgroup_stats(cgroups.scope_path)
    component_stats = {
        "main": cgroup_stats(cgroups.main_path),
        "harness": cgroup_stats(cgroups.harness_path),
        "build": cgroup_stats(cgroups.build_path),
    }
    container_stats = watcher.stats() if watcher else None
    build_component = container_stats if args.cargo_mode == "shim" else component_stats["build"]
    result["cgroup"] = {
        "available": cgroups.nested,
        "nested": cgroups.nested,
        "scope": scope_stats,
        "main": component_stats["main"],
        "harness": component_stats["harness"],
        "build": component_stats["build"],
        "container": container_stats,
        "build_effective": build_component,
        "sampled": {
            "scope_current_max_bytes": max_scope_current,
            "container_current_max_bytes": max_container_current,
            "combined_current_max_bytes": max_combined_current,
            "component_current_max_bytes": max_component_current,
            "component_anon_max_bytes": max_component_anon,
            "component_kernel_max_bytes": max_component_kernel,
        },
    }

    scope_peak = scope_stats.get("peak_bytes") or 0
    main_cpu = (component_stats["main"] or {}).get("cpu_usage_usec") or 0
    scope_cpu = scope_stats.get("cpu_usage_usec") or 0
    container_peak = (container_stats or {}).get("peak_bytes") or 0
    container_cpu = (container_stats or {}).get("cpu_usage_usec") or 0
    cgroup_available = cgroups.nested
    result["summary"] = {
        "cgroup_available": cgroup_available,
        "job_peak_bytes": ((max_combined_current if args.cargo_mode == "shim" else scope_peak) if cgroup_available else 0),
        "job_peak_upper_bound_bytes": (scope_peak + (container_peak if args.cargo_mode == "shim" else 0)) if cgroup_available else 0,
        "job_peak_excluding_observer_bytes": max_component_current if cgroup_available else 0,
        "job_peak_anon_bytes": max_component_anon if cgroup_available else 0,
        "job_peak_kernel_bytes": max_component_kernel if cgroup_available else 0,
        "harness_peak_bytes": ((component_stats["harness"] or {}).get("peak_bytes") or 0) if cgroup_available else 0,
        "build_peak_bytes": ((build_component or {}).get("peak_bytes") or 0) if cgroup_available else 0,
        "job_cpu_usec": (
            (max(scope_cpu - main_cpu, 0) + (container_cpu if args.cargo_mode == "shim" else 0))
            if cgroup_available
            else (job_cpu_usec + (container_cpu_usec if args.cargo_mode == "shim" else 0))
        ),
        "observer_cpu_usec": main_cpu if cgroup_available else observer_cpu_usec,
        "harness_cpu_usec": ((component_stats["harness"] or {}).get("cpu_usage_usec") or 0) if cgroup_available else 0,
        "build_cpu_usec": ((build_component or {}).get("cpu_usage_usec") or 0) if cgroup_available else 0,
        "proc_job_peak_rss_bytes": job_peak_rss + (container_peak_rss if args.cargo_mode == "shim" else 0),
        "proc_job_cpu_usec": job_cpu_usec + (container_cpu_usec if args.cargo_mode == "shim" else 0),
        "proc_observer_cpu_usec": observer_cpu_usec,
    }

    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        f"{args.label}: exit={result.get('build_exit_code')} "
        f"duration={job_duration:.1f}s "
        f"cgroup_peak={result['summary']['job_peak_bytes'] / MIB:.1f} MiB "
        f"components={result['summary']['job_peak_excluding_observer_bytes'] / MIB:.1f} MiB "
        f"proc_peak={result['summary']['proc_job_peak_rss_bytes'] / MIB:.1f} MiB "
        f"cpu={result['summary']['job_cpu_usec'] / 1_000_000:.1f}s"
    )
    return 0 if result.get("build_exit_code") == 0 and not result["errors"] else 1


if __name__ == "__main__":
    sys.exit(main())
