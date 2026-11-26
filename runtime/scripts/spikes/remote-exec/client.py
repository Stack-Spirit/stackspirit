#!/usr/bin/env python3
"""Client side of the remote-exec streaming spike.

The client is the prototype of `vowp __exec`:

* writes every event to a durable local spool before sending it,
* streams JSON lines to the home over an SSH-like transport,
* records acks by event id and re-sends every unacked event after a drop,
* can resume a spool left by a killed client,
* follows lease-revoked notices from the home without waiting for the turn,
* keeps enough timing data to compare transports, windows and ControlMaster.
"""

from __future__ import annotations

import argparse
import os
import queue
import shlex
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from wire import (
    ACK,
    DONE,
    END,
    ERROR,
    EVENT,
    HEARTBEAT,
    HELLO,
    HELLO_ACK,
    LEASE_REVOKED,
    PROTOCOL_VERSION,
    WireError,
    frame_bytes,
    latency_summary,
    make_event_frames,
    mononow_ms,
    parse_frame,
    read_jsonl,
    write_json_atomic,
)

EXIT_REVOKED = 75
EXIT_FAILED = 75


class ConnectionLost(Exception):
    pass


class Revoked(Exception):
    def __init__(self, frame: dict[str, Any]) -> None:
        super().__init__(str(frame.get("reason", "lease revoked")))
        self.frame = frame


class FatalFrame(Exception):
    def __init__(self, frame: dict[str, Any]) -> None:
        super().__init__(str(frame.get("message", "home error")))
        self.frame = frame


class FrameConnection:
    """One SSH channel with a reader thread and a frame queue."""

    def __init__(self, process: subprocess.Popen[bytes], stderr_path: Path) -> None:
        self.process = process
        self.stderr_path = stderr_path
        self.frames: "queue.Queue[dict[str, Any]]" = queue.Queue()
        self.closed = threading.Event()
        self.read_error: str | None = None
        self._reader = threading.Thread(target=self._read, daemon=True)

    def start(self) -> None:
        self._reader.start()

    def _read(self) -> None:
        assert self.process.stdout is not None
        try:
            for raw in self.process.stdout:
                try:
                    frame = parse_frame(raw)
                except WireError:
                    continue
                self.frames.put(frame)
        except (OSError, ValueError):
            pass
        finally:
            self.closed.set()

    def send(self, frame: dict[str, Any]) -> None:
        if self.closed.is_set() or self.process.stdin is None:
            raise ConnectionLost("channel closed")
        try:
            self.process.stdin.write(frame_bytes(frame))
            self.process.stdin.flush()
        except (BrokenPipeError, OSError, ValueError) as err:
            raise ConnectionLost(str(err)) from err

    def get(self, timeout: float) -> dict[str, Any] | None:
        try:
            return self.frames.get(timeout=timeout)
        except queue.Empty:
            return None

    def kill(self) -> None:
        if self.process.poll() is not None:
            return
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def _close_stderr(self) -> None:
        stderr = getattr(self.process, "_spike_stderr", None)
        if stderr is not None:
            try:
                stderr.close()
            except OSError:
                pass

    def close(self, graceful: bool = True) -> None:
        try:
            if self.process.stdin is not None:
                self.process.stdin.close()
        except (BrokenPipeError, OSError):
            pass
        if graceful:
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.kill()
        else:
            self.kill()
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        self._reader.join(timeout=2)
        self._close_stderr()


class FakeSshTransport:
    """Runs `fake_ssh.py`, which starts `ingest.py` locally without a socket."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args

    def start(self, conn_index: int) -> FrameConnection:
        trace = Path(self.args.fake_ssh_trace)
        trace.parent.mkdir(parents=True, exist_ok=True)
        command = [
            sys.executable,
            "-B",
            str(Path(self.args.fake_ssh_script).resolve()),
            "--conn-index",
            str(conn_index),
            "--trace",
            str(trace),
        ]
        if self.args.fake_ssh_plan:
            command += ["--plan", str(Path(self.args.fake_ssh_plan).resolve())]
        command += ["--", sys.executable, "-B", str(Path(self.args.ingest_script).resolve())]
        command += ingest_argv(self.args)
        stderr_path = Path(self.args.workdir) / f"conn-{conn_index:03d}.stderr"
        stderr = stderr_path.open("ab")
        process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr,
            bufsize=0,
            start_new_session=True,
        )
        process._spike_stderr = stderr  # type: ignore[attr-defined]
        conn = FrameConnection(process, stderr_path)
        conn.start()
        return conn


class RealSshTransport:
    """A real `ssh -T` channel to the home (loopback in the live check)."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args

    def start(self, conn_index: int) -> FrameConnection:
        remote = [
            self.args.remote_python,
            "-B",
            str(Path(self.args.ingest_script).resolve()),
            *ingest_argv(self.args),
        ]
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
            str(self.args.ssh_port),
            "-i",
            str(Path(self.args.ssh_key).resolve()),
        ]
        if self.args.ssh_mux_socket:
            command += ["-S", str(Path(self.args.ssh_mux_socket).resolve())]
        command += [self.args.ssh_target, shlex.join(remote)]
        stderr_path = Path(self.args.workdir) / f"conn-{conn_index:03d}.stderr"
        stderr = stderr_path.open("ab")
        process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr,
            bufsize=0,
            start_new_session=True,
        )
        process._spike_stderr = stderr  # type: ignore[attr-defined]
        conn = FrameConnection(process, stderr_path)
        conn.start()
        return conn


def ingest_argv(args: argparse.Namespace) -> list[str]:
    argv = [
        "--session-dir",
        str(Path(args.home_dir).resolve()),
        "--session",
        args.session,
        "--lease-file",
        str(Path(args.lease_file).resolve()),
    ]
    if args.fsync_each:
        argv.append("--fsync-each")
    return argv


def _read_acks(path: Path) -> dict[str, int]:
    acked: dict[str, int] = {}
    for frame in read_jsonl(path):
        event_id = frame.get("id")
        seq = frame.get("seq")
        if isinstance(event_id, str) and isinstance(seq, int):
            acked[event_id] = seq
    return acked


@dataclass
class StreamConfig:
    args: argparse.Namespace
    events: list[dict[str, Any]]
    spool: Path
    ack_file: Path
    acked: dict[str, int] = field(default_factory=dict)

    @property
    def workdir(self) -> Path:
        return Path(self.args.workdir)


def _connect(
    transport: FakeSshTransport | RealSshTransport,
    args: argparse.Namespace,
    index: int,
) -> tuple[FrameConnection, float]:
    started = mononow_ms()
    conn = transport.start(index)
    try:
        conn.send(
            {
                "type": HELLO,
                "version": PROTOCOL_VERSION,
                "session": args.session,
                "lease": args.lease_token,
                "client": "remote-exec-spike",
            }
        )
    except ConnectionLost:
        conn.close(graceful=False)
        raise
    deadline = time.monotonic() + 20.0
    while time.monotonic() < deadline:
        frame = conn.get(0.1)
        if frame is None:
            if conn.closed.is_set():
                conn.close(graceful=False)
                raise ConnectionLost("channel closed before hello.ack")
            continue
        kind = frame.get("type")
        if kind == HELLO_ACK:
            return conn, mononow_ms() - started
        if kind == LEASE_REVOKED:
            conn.close(graceful=False)
            raise Revoked(frame)
        if kind == ERROR:
            conn.close(graceful=False)
            raise FatalFrame(frame)
    conn.close(graceful=False)
    raise ConnectionLost("timed out waiting for hello.ack")


def _first_unacked(events: list[dict[str, Any]], acked: dict[str, int]) -> int:
    for index, frame in enumerate(events):
        if frame.get("id") not in acked:
            return index
    return len(events)


def run_stream(config: StreamConfig) -> dict[str, Any]:
    args = config.args
    events = config.events
    workdir = Path(args.workdir)
    workdir.mkdir(parents=True, exist_ok=True)
    config.ack_file.parent.mkdir(parents=True, exist_ok=True)

    transport: FakeSshTransport | RealSshTransport
    if args.transport == "real-ssh":
        transport = RealSshTransport(args)
    else:
        transport = FakeSshTransport(args)

    summary: dict[str, Any] = {
        "status": "running",
        "session": args.session,
        "events": len(events),
        "window": args.window,
        "transport": args.transport,
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    started = mononow_ms()
    acked = dict(config.acked)
    ack_rtts: list[float] = []
    connect_times: list[float] = []
    reconnect_times: list[float] = []
    reconnects = 0
    connect_attempts = 0
    forced_cuts = 0
    kill_after_done = False
    sends = 0
    sent_counts: dict[str, int] = {}
    duplicate_acks = 0
    notice: dict[str, Any] | None = None
    notice_received_at: float | None = None
    notice_latency_ms: float | None = None
    fatal: dict[str, Any] | None = None
    done_frame: dict[str, Any] | None = None
    conn: FrameConnection | None = None
    conn_index = 0
    status = "ok"

    ack_handle = config.ack_file.open("ab", buffering=0)

    def write_ack(frame: dict[str, Any]) -> None:
        line = frame_bytes(
            {
                "id": frame.get("id"),
                "seq": frame.get("seq"),
                "duplicate": bool(frame.get("duplicate")),
                "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            }
        )
        ack_handle.write(line)

    def connect_next() -> FrameConnection:
        nonlocal conn_index, connect_attempts
        connect_attempts += 1
        new_conn, elapsed = _connect(transport, args, conn_index)
        connect_times.append(elapsed)
        if conn_index > 0:
            reconnect_times.append(elapsed)
        conn_index += 1
        return new_conn

    try:
        # First connection. A hello error (bad lease, bad session) is fatal;
        # a transport failure is retried up to the same reconnect budget.
        while conn is None and notice is None and fatal is None:
            try:
                conn = connect_next()
            except Revoked as err:
                notice = err.frame
                notice_received_at = time.time()
            except FatalFrame as err:
                fatal = err.frame
            except ConnectionLost:
                if connect_attempts >= args.max_reconnects + 1:
                    status = "reconnect-limit"
                    break
                time.sleep(min(1.0, 0.05 * (2 ** (connect_attempts - 1))))

        if conn is not None and notice is None and fatal is None:
            next_index = _first_unacked(events, acked)
            inflight: dict[str, float] = {}
            next_heartbeat_at = mononow_ms()

            def apply_frame(frame: dict[str, Any]) -> None:
                nonlocal notice, notice_received_at, fatal, duplicate_acks
                kind = frame.get("type")
                if kind == ACK:
                    event_id = frame.get("id")
                    sent_at = inflight.pop(event_id, None)
                    if sent_at is not None:
                        ack_rtts.append(mononow_ms() - sent_at)
                    if event_id not in acked:
                        acked[str(event_id)] = int(frame.get("seq", 0))
                    if frame.get("duplicate"):
                        duplicate_acks += 1
                    write_ack(frame)
                elif kind == LEASE_REVOKED:
                    notice = frame
                    notice_received_at = time.time()
                elif kind == ERROR:
                    fatal = frame

            def drain(connection: FrameConnection, timeout: float = 0.0) -> None:
                """Consume frames that arrived before EOF, so a final ack/notice wins."""
                while True:
                    queued = connection.get(timeout)
                    if queued is None:
                        return
                    apply_frame(queued)
                    if notice is not None or fatal is not None:
                        return
                    timeout = 0.0

            while True:
                if notice is not None or fatal is not None:
                    break
                if conn is None or conn.closed.is_set():
                    if conn is not None:
                        # The home may have written an ack or a lease notice just
                        # before the channel closed. Read those frames before
                        # deciding that the connection is gone.
                        drain(conn)
                        if notice is not None or fatal is not None:
                            break
                        conn.close(graceful=False)
                        conn = None
                    if next_index >= len(events) and not inflight:
                        break
                    if reconnects >= args.max_reconnects:
                        status = "reconnect-limit"
                        break
                    reconnects += 1
                    time.sleep(min(1.0, 0.05 * (2 ** (reconnects - 1))))
                    try:
                        conn = connect_next()
                    except Revoked as err:
                        notice = err.frame
                        notice_received_at = time.time()
                        break
                    except FatalFrame as err:
                        fatal = err.frame
                        break
                    except ConnectionLost:
                        continue
                    inflight.clear()
                    next_index = _first_unacked(events, acked)
                    next_heartbeat_at = mononow_ms()
                    continue

                now = mononow_ms()
                if now >= next_heartbeat_at:
                    try:
                        conn.send({"type": HEARTBEAT, "ts": now})
                    except ConnectionLost:
                        continue
                    next_heartbeat_at = now + args.heartbeat_interval * 1000.0

                while (
                    next_index < len(events)
                    and len(inflight) < args.window
                    and conn is not None
                    and not conn.closed.is_set()
                ):
                    frame = events[next_index]
                    try:
                        conn.send(frame)
                    except ConnectionLost:
                        break
                    sends += 1
                    sent_counts[frame["id"]] = sent_counts.get(frame["id"], 0) + 1
                    inflight[frame["id"]] = mononow_ms()
                    next_index += 1
                    if args.event_interval > 0:
                        time.sleep(args.event_interval)
                    if (
                        args.kill_ssh_after_events
                        and not kill_after_done
                        and sends >= args.kill_ssh_after_events
                    ):
                        kill_after_done = True
                        forced_cuts += 1
                        conn.kill()
                        break

                if conn is None or conn.closed.is_set():
                    continue
                if next_index >= len(events) and not inflight:
                    break

                frame = conn.get(0.05)
                if frame is None:
                    continue
                apply_frame(frame)

            if status == "ok" and notice is None and fatal is None:
                # All events are acked. Send `end` and wait for the home's summary.
                end_deadline: float | None = None
                while True:
                    if notice is not None or fatal is not None:
                        break
                    if conn is None or conn.closed.is_set():
                        if conn is not None:
                            drain(conn)
                            if notice is not None or fatal is not None:
                                break
                            conn.close(graceful=False)
                            conn = None
                        if reconnects >= args.max_reconnects:
                            status = "reconnect-limit"
                            break
                        reconnects += 1
                        time.sleep(min(1.0, 0.05 * (2 ** (reconnects - 1))))
                        try:
                            conn = connect_next()
                        except Revoked as err:
                            notice = err.frame
                            notice_received_at = time.time()
                            break
                        except FatalFrame as err:
                            fatal = err.frame
                            break
                        except ConnectionLost:
                            continue
                        end_deadline = None
                    try:
                        conn.send({"type": END, "last_client_seq": len(events)})
                    except ConnectionLost:
                        end_deadline = None
                        continue
                    if end_deadline is None:
                        end_deadline = time.monotonic() + 20.0
                    if time.monotonic() > end_deadline:
                        status = "end-timeout"
                        break
                    frame = conn.get(0.1)
                    if frame is None:
                        continue
                    kind = frame.get("type")
                    if kind == DONE:
                        done_frame = frame
                        break
                    apply_frame(frame)
    finally:
        ack_handle.close()
        if conn is not None:
            conn.close(graceful=True)

    duration = mononow_ms() - started
    if notice is not None:
        status = "revoked"
    elif fatal is not None:
        status = "error"
    if notice_received_at is not None and args.takeover_written_at:
        notice_latency_ms = max(0.0, (notice_received_at - float(args.takeover_written_at)) * 1000.0)
    summary.update(
        {
            "status": status,
            "finished_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "duration_ms": round(duration, 3),
            "reconnects": reconnects,
            "connect_attempts": connect_attempts,
            "forced_cuts": forced_cuts,
            "sends": sends,
            "resent": sum(max(0, count - 1) for count in sent_counts.values()),
            "acks": len(acked),
            "duplicate_acks": duplicate_acks,
            "ack_rtt": latency_summary(ack_rtts),
            "connect": latency_summary(connect_times),
            "reconnect": latency_summary(reconnect_times),
            "notice": notice,
            "notice_received_at": notice_received_at,
            "notice_latency_ms": round(notice_latency_ms, 3) if notice_latency_ms is not None else None,
            "error": fatal,
            "done": done_frame,
        }
    )
    if args.kill_ssh_after_events and forced_cuts == 0:
        summary["forced_cut_missed"] = True
    write_json_atomic(Path(args.summary), summary)
    return summary


def prepare_spool(
    workdir: Path,
    spool: Path,
    ack_file: Path,
    events: list[dict[str, Any]] | None,
    fsync: bool,
) -> list[dict[str, Any]]:
    workdir.mkdir(parents=True, exist_ok=True)
    if events is not None:
        # A run pre-spools its synthetic batch. Write the whole file aside and
        # rename it, so a client killed while spooling never leaves a torn
        # spool that a later resume would reject as corruption.
        tmp = spool.with_name(f"{spool.name}.tmp.{os.getpid()}")
        with tmp.open("wb") as handle:
            for frame in events:
                handle.write(frame_bytes(frame))
                handle.flush()
                if fsync:
                    os.fsync(handle.fileno())
        os.replace(tmp, spool)
        ack_file.parent.mkdir(parents=True, exist_ok=True)
        ack_file.write_bytes(b"")
        return events
    return [frame for frame in read_jsonl(spool) if frame.get("type") == EVENT]


def run(args: argparse.Namespace) -> int:
    workdir = Path(args.workdir)
    spool = Path(args.spool) if args.spool else workdir / "spool.jsonl"
    ack_file = Path(args.acks) if args.acks else workdir / "acks.jsonl"
    events = make_event_frames(args.events, args.session, args.text_size) if not args.resume else None
    frames = prepare_spool(workdir, spool, ack_file, events, args.fsync_spool and not args.resume)
    acked = _read_acks(ack_file)
    config = StreamConfig(args=args, events=frames, spool=spool, ack_file=ack_file, acked=acked)
    summary = run_stream(config)
    print(
        "client: status={status} events={events} acks={acks} reconnects={reconnects} "
        "resent={resent} duplicate_acks={duplicate_acks} duration_ms={duration_ms}".format(**summary),
        flush=True,
    )
    if summary.get("notice"):
        print(
            "client: lease revoked: {reason} (by {by} at {at}); this surface is read-only".format(
                reason=summary["notice"].get("reason"),
                by=summary["notice"].get("by"),
                at=summary["notice"].get("at"),
            ),
            flush=True,
        )
    if summary.get("error"):
        print(f"client: home error: {summary['error']}", file=sys.stderr, flush=True)
    if summary["status"] == "ok":
        return 0
    if summary["status"] == "revoked":
        return EXIT_REVOKED
    return EXIT_FAILED


def add_common_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--workdir", required=True, help="spool, acks and summary directory")
    parser.add_argument("--spool", help="spool file (default WORKDIR/spool.jsonl)")
    parser.add_argument("--acks", help="ack file (default WORKDIR/acks.jsonl)")
    parser.add_argument("--summary", default=None, help="summary JSON path (default WORKDIR/summary.json)")
    parser.add_argument("--session", default="ses-spike")
    parser.add_argument("--home-dir", required=True, help="home session directory")
    parser.add_argument("--lease-file", required=True, help="home lease JSON file")
    parser.add_argument("--lease-token", default="tok-spike")
    parser.add_argument("--ingest-script", default=str(Path(__file__).with_name("ingest.py")))
    parser.add_argument("--fake-ssh-script", default=str(Path(__file__).with_name("fake_ssh.py")))
    parser.add_argument("--fake-ssh-plan")
    parser.add_argument("--fake-ssh-trace")
    parser.add_argument("--transport", choices=("fake-ssh", "real-ssh"), default="fake-ssh")
    parser.add_argument("--ssh-target")
    parser.add_argument("--ssh-port", type=int, default=22)
    parser.add_argument("--ssh-key")
    parser.add_argument("--ssh-mux-socket")
    parser.add_argument("--remote-python", default="python3")
    parser.add_argument("--window", type=int, default=1, help="maximum events in flight")
    parser.add_argument("--heartbeat-interval", type=float, default=0.5)
    parser.add_argument("--max-reconnects", type=int, default=5)
    parser.add_argument("--event-interval", type=float, default=0.0)
    parser.add_argument("--kill-ssh-after-events", type=int, default=0)
    parser.add_argument("--takeover-written-at", type=float, default=None)
    parser.add_argument("--fsync-each", action="store_true")
    parser.add_argument("--fsync-spool", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--quiet", action="store_true")


def run_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="remote-exec spike client")
    add_common_arguments(parser)
    parser.add_argument("--events", type=int, default=200)
    parser.add_argument("--text-size", type=int, default=48)
    parser.add_argument("--resume", action="store_true", help="send only unacked frames from an existing spool")
    return parser


def resume_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="resume a spool left by a killed remote-exec client")
    add_common_arguments(parser)
    parser.set_defaults(resume=True, events=0)
    return parser


def main(argv: list[str] | None = None, *, resume: bool = False) -> int:
    parser = resume_parser() if resume else run_parser()
    args = parser.parse_args(argv)
    if args.summary is None:
        args.summary = str(Path(args.workdir) / "summary.json")
    if not args.fake_ssh_trace:
        args.fake_ssh_trace = str(Path(args.workdir) / "fake-ssh-trace.jsonl")
    if args.transport == "real-ssh" and (not args.ssh_target or not args.ssh_key):
        parser.error("--transport real-ssh needs --ssh-target and --ssh-key")
    if args.window < 1:
        parser.error("--window must be >= 1")
    return run(args)


if __name__ == "__main__":
    sys.exit(main(resume="--resume" in sys.argv[1:]))
