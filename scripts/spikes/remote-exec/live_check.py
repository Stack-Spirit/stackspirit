#!/usr/bin/env python3
"""Opt-in live measurements over a private loopback sshd.

Nothing here touches the real VOWP home, tmux, a harness or an external host.
It starts `/usr/sbin/sshd` as the current user on a free 127.0.0.1 port, with
temporary host/client keys and an `AllowUsers` entry for that user. The optional
TCP delay proxy is another loopback listener; it only makes the ControlMaster
and pipelining trade-off visible without a real WAN.

On a machine without sshd the hermetic tests still run; this script exits 0 with
a `skipped` marker when invoked through `run.sh --live`.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

import wire

SSHD_CANDIDATES = ("/usr/sbin/sshd", "/usr/local/sbin/sshd")


def find_sshd() -> str | None:
    env = os.environ.get("SSHD")
    if env:
        return env if Path(env).is_file() else None
    for candidate in SSHD_CANDIDATES:
        if Path(candidate).is_file():
            return candidate
    return shutil.which("sshd")


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_port(port: int, timeout: float = 10.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return True
        except OSError:
            time.sleep(0.05)
    return False


def ssh_version() -> str:
    try:
        out = subprocess.run(["ssh", "-V"], capture_output=True, text=True, check=False)
        return (out.stderr or out.stdout).strip().splitlines()[0]
    except OSError:
        return "ssh not found"


class PrivateSshd:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)
        self.port = 0
        self.process: subprocess.Popen[bytes] | None = None
        self.log_path = root / "sshd.log"
        self.key_path = root / "client_key"
        self.host_key = root / "host_key"
        self.authorized_keys = root / "authorized_keys"
        self.user = os.environ.get("USER") or os.environ.get("LOGNAME") or "user"

    def start(self) -> None:
        subprocess.run(
            ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.host_key)],
            check=True,
            capture_output=True,
        )
        subprocess.run(
            ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.key_path)],
            check=True,
            capture_output=True,
        )
        self.key_path.chmod(0o600)
        self.authorized_keys.write_bytes(self.key_path.with_suffix(".pub").read_bytes())
        self.authorized_keys.chmod(0o600)

        sshd = find_sshd()
        if not sshd:
            raise RuntimeError("sshd not found")
        last_error = ""
        for _attempt in range(3):
            self.port = free_port()
            config = self.root / "sshd_config"
            config.write_text(
                "\n".join(
                    [
                        f"Port {self.port}",
                        "ListenAddress 127.0.0.1",
                        f"HostKey {self.host_key}",
                        f"PidFile {self.root / 'sshd.pid'}",
                        f"AuthorizedKeysFile {self.authorized_keys}",
                        "PasswordAuthentication no",
                        "KbdInteractiveAuthentication no",
                        "ChallengeResponseAuthentication no",
                        "UsePAM no",
                        "StrictModes no",
                        "PermitRootLogin no",
                        f"AllowUsers {self.user}",
                        "PrintMotd no",
                        "LogLevel VERBOSE",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            self.process = subprocess.Popen(
                [sshd, "-D", "-f", str(config), "-E", str(self.log_path)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                start_new_session=True,
            )
            if wait_port(self.port, timeout=5.0):
                return
            last_error = self.log_path.read_text(encoding="utf-8", errors="replace")[-2000:] if self.log_path.exists() else ""
            self.stop()
        raise RuntimeError(f"private sshd did not start: {last_error}")

    def target(self) -> str:
        return f"{self.user}@127.0.0.1"

    def stop(self) -> None:
        if self.process is None:
            return
        try:
            self.process.terminate()
            self.process.wait(timeout=5)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            try:
                os.killpg(self.process.pid, 15)
            except ProcessLookupError:
                pass
        self.process = None


class DelayProxy:
    """A tiny TCP proxy that sleeps before every forwarded chunk."""

    def __init__(self, upstream_port: int, delay_ms: float) -> None:
        self.upstream_port = upstream_port
        self.delay = delay_ms / 1000.0
        self.stop_event = threading.Event()
        self.listener: socket.socket | None = None
        self.port = 0
        self.threads: list[threading.Thread] = []
        self.sockets: list[socket.socket] = []

    def start(self) -> None:
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(16)
        self.listener.settimeout(0.2)
        self.port = int(self.listener.getsockname()[1])
        thread = threading.Thread(target=self._accept, daemon=True)
        thread.start()
        self.threads.append(thread)

    def _accept(self) -> None:
        assert self.listener is not None
        while not self.stop_event.is_set():
            try:
                client, _addr = self.listener.accept()
            except (socket.timeout, OSError):
                continue
            try:
                upstream = socket.create_connection(("127.0.0.1", self.upstream_port), timeout=5)
            except OSError:
                client.close()
                continue
            self.sockets.extend([client, upstream])
            for src, dst in ((client, upstream), (upstream, client)):
                thread = threading.Thread(target=self._pump, args=(src, dst), daemon=True)
                thread.start()
                self.threads.append(thread)

    def _pump(self, src: socket.socket, dst: socket.socket) -> None:
        try:
            while not self.stop_event.is_set():
                data = src.recv(65536)
                if not data:
                    try:
                        dst.shutdown(socket.SHUT_WR)
                    except OSError:
                        pass
                    break
                if self.delay:
                    time.sleep(self.delay)
                dst.sendall(data)
        except OSError:
            pass
        finally:
            try:
                src.close()
            except OSError:
                pass

    def stop(self) -> None:
        self.stop_event.set()
        if self.listener is not None:
            try:
                self.listener.close()
            except OSError:
                pass
        for sock in self.sockets:
            try:
                sock.close()
            except OSError:
                pass


def ssh_base(port: int, key: Path, target: str, mux_socket: Path | None = None) -> list[str]:
    command = [
        "ssh",
        "-T",
        "-F",
        "/dev/null",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "IdentityAgent=none",
        "-o",
        "PreferredAuthentications=publickey",
        "-p",
        str(port),
        "-i",
        str(key),
    ]
    if mux_socket is not None:
        command += ["-S", str(mux_socket)]
    command.append(target)
    return command


def timed(command: list[str], timeout: float = 30.0) -> dict[str, Any]:
    started = time.monotonic()
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
        return {
            "ok": result.returncode == 0,
            "code": result.returncode,
            "ms": round((time.monotonic() - started) * 1000.0, 3),
            "stderr": result.stderr.strip()[-300:],
        }
    except subprocess.TimeoutExpired:
        return {
            "ok": False,
            "code": None,
            "ms": round((time.monotonic() - started) * 1000.0, 3),
            "stderr": "timeout",
        }


def stats(values: list[float]) -> dict[str, Any]:
    return wire.latency_summary(values)


class MuxMaster:
    def __init__(self, port: int, key: Path, target: str, socket_path: Path, log_path: Path) -> None:
        self.port = port
        self.key = key
        self.target = target
        self.socket_path = socket_path
        self.log_path = log_path
        self.process: subprocess.Popen[bytes] | None = None

    def start(self) -> None:
        self.socket_path.unlink(missing_ok=True)
        command = ssh_base(self.port, self.key, self.target)
        command = command[:-1] + [
            "-M",
            "-N",
            "-S",
            str(self.socket_path),
        ] + [self.target]
        log = self.log_path.open("ab")
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            stderr=log,
            start_new_session=True,
        )
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"ssh master exited: {self.log_path.read_text(errors='replace')[-1000:]}")
            # `ssh -S` silently falls back to a normal connection when the
            # control socket is absent. Wait for the socket itself, otherwise a
            # "mux" measurement is really a cold handshake.
            if self.socket_path.exists() and self.socket_path.is_socket():
                probe = timed(ssh_base(self.port, self.key, self.target, self.socket_path) + ["true"], timeout=5)
                if probe["ok"]:
                    return
            time.sleep(0.05)
        raise RuntimeError("ssh master did not create its control socket")

    def ensure(self) -> None:
        """Restart a master that died between measurements."""
        if self.process is not None and self.process.poll() is None:
            return
        self.start()

    def alive(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def stop(self) -> None:
        if self.process is None:
            return
        try:
            subprocess.run(
                ssh_base(self.port, self.key, self.target, self.socket_path) + ["-O", "exit"],
                capture_output=True,
                timeout=5,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            pass
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, 15)
            except ProcessLookupError:
                pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        if self.process.stdin is not None:
            try:
                self.process.stdin.close()
            except OSError:
                pass
        self.socket_path.unlink(missing_ok=True)
        self.process = None


def run_client(root: Path, name: str, *, port: int, key: Path, target: str, events: int,
               window: int, mux_socket: Path | None = None, kill_after: int = 0,
               event_interval: float = 0.0) -> dict[str, Any]:
    spike = Path(__file__).with_name("remote_exec_spike.py").resolve()
    workdir = root / name
    workdir.mkdir(parents=True, exist_ok=True)
    home_dir = workdir / "home"
    lease_file = workdir / "lease.json"
    wire.write_json_atomic(lease_file, wire.lease_document("tok-live"))
    command = [
        sys.executable,
        str(spike),
        "run",
        "--workdir",
        str(workdir),
        "--home-dir",
        str(home_dir),
        "--lease-file",
        str(lease_file),
        "--lease-token",
        "tok-live",
        "--session",
        "ses-live",
        "--transport",
        "real-ssh",
        "--ssh-target",
        target,
        "--ssh-port",
        str(port),
        "--ssh-key",
        str(key),
        "--events",
        str(events),
        "--window",
        str(window),
        "--heartbeat-interval",
        "0.5",
        "--max-reconnects",
        "3",
    ]
    if mux_socket is not None:
        command += ["--ssh-mux-socket", str(mux_socket)]
    if kill_after:
        command += ["--kill-ssh-after-events", str(kill_after)]
    if event_interval:
        command += ["--event-interval", str(event_interval)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=120, check=False)
    summary_path = workdir / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8")) if summary_path.exists() else {}
    verification = wire.verify_stream(home_dir / "log.jsonl", workdir / "spool.jsonl")
    return {
        "name": name,
        "exit": result.returncode,
        "stdout": scrub(result.stdout.strip()[-1000:], root),
        "stderr": scrub(result.stderr.strip()[-1000:], root),
        "summary": summary,
        "verify": verification,
    }


def scrub(text: str, root: Path) -> str:
    """Keep temporary operator paths out of committed evidence."""
    return text.replace(str(root), "<work>")


def run_takeover_client(root: Path, *, port: int, key: Path, target: str,
                        mux_socket: Path | None = None) -> dict[str, Any]:
    spike = Path(__file__).with_name("remote_exec_spike.py").resolve()
    workdir = root / "lease-revoke"
    workdir.mkdir(parents=True, exist_ok=True)
    home_dir = workdir / "home"
    lease_file = workdir / "lease.json"
    wire.write_json_atomic(lease_file, wire.lease_document("tok-live"))
    command = [
        sys.executable,
        str(spike),
        "run",
        "--workdir",
        str(workdir),
        "--home-dir",
        str(home_dir),
        "--lease-file",
        str(lease_file),
        "--lease-token",
        "tok-live",
        "--session",
        "ses-live",
        "--transport",
        "real-ssh",
        "--ssh-target",
        target,
        "--ssh-port",
        str(port),
        "--ssh-key",
        str(key),
        "--events",
        "30",
        "--window",
        "1",
        "--event-interval",
        "0.2",
        "--heartbeat-interval",
        "0.2",
        "--max-reconnects",
        "1",
    ]
    if mux_socket is not None:
        command += ["--ssh-mux-socket", str(mux_socket)]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(0.8)
    takeover = subprocess.run(
        [
            sys.executable,
            str(spike),
            "takeover",
            "--lease-file",
            str(lease_file),
            "--current-token",
            "tok-live",
            "--by",
            "phone",
            "--reason",
            "taken over by phone",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    written_at = json.loads(takeover.stdout)["written_at"] if takeover.returncode == 0 else None
    stdout, stderr = process.communicate(timeout=30)
    summary_path = workdir / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8")) if summary_path.exists() else {}
    latency = None
    if written_at is not None and summary.get("notice_received_at") is not None:
        latency = round((summary["notice_received_at"] - written_at) * 1000.0, 3)
    return {
        "name": "lease-revoke",
        "exit": process.returncode,
        "stdout": scrub(stdout.strip()[-1000:], root),
        "stderr": scrub(stderr.strip()[-1000:], root),
        "summary": summary,
        "takeover_written_at": written_at,
        "notice_latency_ms": latency,
        "verify": wire.verify_stream(home_dir / "log.jsonl", workdir / "spool.jsonl"),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="live loopback measurements for the remote-exec spike")
    parser.add_argument("--samples", type=int, default=8)
    parser.add_argument("--events", type=int, default=400)
    parser.add_argument("--delay-ms", type=float, default=40.0)
    parser.add_argument("--out", default=None)
    parser.add_argument("--keep", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if not find_sshd():
        print("live: skipped: sshd not found (set SSHD=/path/to/sshd to enable)", flush=True)
        return 0
    if not shutil.which("ssh") or not shutil.which("ssh-keygen"):
        print("live: skipped: ssh or ssh-keygen not found", flush=True)
        return 0

    root = Path(tempfile.mkdtemp(prefix="vowp-remote-exec-live."))
    sshd = PrivateSshd(root / "sshd")
    proxy: DelayProxy | None = None
    masters: list[MuxMaster] = []
    report: dict[str, Any] = {
        "generated_at": wire.iso_now(),
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "ssh": ssh_version(),
        },
        "delay_ms": args.delay_ms,
        "bench": {},
        "streams": {},
    }
    try:
        sshd.start()
        target = sshd.target()
        print(f"live: private sshd port={sshd.port} user={sshd.user}", flush=True)

        # Direct loopback handshake cost: cold TCP+auth vs a reused master.
        cold = [timed(ssh_base(sshd.port, sshd.key_path, target) + ["true"]) for _ in range(args.samples)]
        cold_ok = [item["ms"] for item in cold if item["ok"]]
        master = MuxMaster(
            sshd.port,
            sshd.key_path,
            target,
            root / "mux-loopback.sock",
            root / "mux-loopback.log",
        )
        master.start()
        masters.append(master)
        muxed = [
            timed(ssh_base(sshd.port, sshd.key_path, target, root / "mux-loopback.sock") + ["true"])
            for _ in range(args.samples)
        ]
        muxed_ok = [item["ms"] for item in muxed if item["ok"]]
        report["bench"]["loopback"] = {
            "cold": stats(cold_ok),
            "mux": stats(muxed_ok),
            "cold_failures": sum(1 for item in cold if not item["ok"]),
            "mux_failures": sum(1 for item in muxed if not item["ok"]),
        }

        # Streams on direct loopback.
        for name, mux_socket in (("loopback-cold-w64", None), ("loopback-mux-w64", root / "mux-loopback.sock")):
            if mux_socket is not None:
                master.ensure()
            result = run_client(
                root,
                name,
                port=sshd.port,
                key=sshd.key_path,
                target=target,
                events=args.events,
                window=64,
                mux_socket=mux_socket,
            )
            if mux_socket is not None:
                result["master_alive_after"] = master.alive()
            report["streams"][name] = result
            print(f"live: {name}: exit={result['exit']} verify={result['verify']['ok']}", flush=True)

        # A loopback TCP proxy with one-way delay, standing in for a WAN.
        proxy = DelayProxy(sshd.port, args.delay_ms)
        proxy.start()
        delay_port = proxy.port
        print(f"live: delay proxy port={delay_port} one-way={args.delay_ms}ms", flush=True)

        delay_cold = [timed(ssh_base(delay_port, sshd.key_path, target) + ["true"]) for _ in range(max(3, args.samples // 2))]
        delay_cold_ok = [item["ms"] for item in delay_cold if item["ok"]]
        delay_master = MuxMaster(
            delay_port,
            sshd.key_path,
            target,
            root / "mux-delay.sock",
            root / "mux-delay.log",
        )
        delay_master.start()
        masters.append(delay_master)
        delay_muxed = [
            timed(ssh_base(delay_port, sshd.key_path, target, root / "mux-delay.sock") + ["true"])
            for _ in range(max(3, args.samples // 2))
        ]
        delay_muxed_ok = [item["ms"] for item in delay_muxed if item["ok"]]
        report["bench"]["delay"] = {
            "cold": stats(delay_cold_ok),
            "mux": stats(delay_muxed_ok),
            "cold_failures": sum(1 for item in delay_cold if not item["ok"]),
            "mux_failures": sum(1 for item in delay_muxed if not item["ok"]),
        }

        for name, window, events, mux_socket in (
            ("delay-cold-w1", 1, 16, None),
            ("delay-cold-w64", 64, 120, None),
            ("delay-mux-w64", 64, 120, root / "mux-delay.sock"),
        ):
            if mux_socket is not None:
                delay_master.ensure()
            result = run_client(
                root,
                name,
                port=delay_port,
                key=sshd.key_path,
                target=target,
                events=events,
                window=window,
                mux_socket=mux_socket,
            )
            if mux_socket is not None:
                result["master_alive_after"] = delay_master.alive()
            report["streams"][name] = result
            print(f"live: {name}: exit={result['exit']} verify={result['verify']['ok']}", flush=True)

        # Reconnect cost after a forced SSH death, with and without a master.
        reconnect_loop_master = MuxMaster(
            sshd.port,
            sshd.key_path,
            target,
            root / "mux-reconnect-loop.sock",
            root / "mux-reconnect-loop.log",
        )
        reconnect_loop_master.start()
        masters.append(reconnect_loop_master)
        for name, mux_socket in (
            ("reconnect-cold", None),
            ("reconnect-mux", root / "mux-reconnect-loop.sock"),
        ):
            if mux_socket is not None:
                reconnect_loop_master.ensure()
            result = run_client(
                root,
                name,
                port=sshd.port,
                key=sshd.key_path,
                target=target,
                events=300,
                window=8,
                mux_socket=mux_socket,
                kill_after=100,
            )
            if mux_socket is not None:
                result["master_alive_after"] = reconnect_loop_master.alive()
            report["streams"][name] = result
            print(
                f"live: {name}: exit={result['exit']} verify={result['verify']['ok']} "
                f"reconnects={result['summary'].get('reconnects')}",
                flush=True,
            )

        master.ensure()
        lease = run_takeover_client(root, port=sshd.port, key=sshd.key_path, target=target, mux_socket=root / "mux-loopback.sock")
        lease["master_alive_after"] = master.alive()
        report["lease"] = lease
        print(
            f"live: lease-revoke: exit={lease['exit']} notice_latency_ms={lease['notice_latency_ms']}",
            flush=True,
        )
    finally:
        for master in masters:
            master.stop()
        if proxy is not None:
            proxy.stop()
        sshd.stop()
        if not args.keep:
            shutil.rmtree(root, ignore_errors=True)

    if args.out:
        wire.write_json_atomic(Path(args.out), report)
        print(f"live: evidence written to {args.out}", flush=True)
    else:
        print(json.dumps(report, indent=2, sort_keys=True), flush=True)

    ok = (
        all(item["verify"]["ok"] and item["exit"] == 0 for item in report["streams"].values())
        and report["lease"]["summary"].get("status") == "revoked"
        and report["lease"]["exit"] == 75
        and report["lease"]["notice_latency_ms"] is not None
        and report["lease"]["notice_latency_ms"] < 2000.0
    )
    if not ok:
        print("live: FAIL: a stream did not verify or the lease notice missed the 2 s bound", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
