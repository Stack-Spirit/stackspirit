#!/usr/bin/env python3
"""Home-side ingest process for the remote-exec streaming spike.

This is the prototype of `vowp session ingest <id> --lease <token>`:

* reads JSON frames from stdin (the SSH channel),
* owns the single writer lock and assigns `seq` in append order,
* appends one line per new event to an append-only home log,
* acknowledges by event id and never appends the same id twice,
* truncates a torn final line before appending,
* checks the lease file while idle and pushes `lease.revoked` on takeover.

It is deliberately not a second store implementation: the log format mirrors
`vowp-store::session_log` (one EventEnvelope per line, seq starts at 1, torn tail
is truncated) so F1 can port the semantics into the real Rust code.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import queue
import signal
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from wire import (
    ACK,
    DONE,
    END,
    ERROR,
    EVENT,
    HEARTBEAT,
    HEARTBEAT_ACK,
    HELLO,
    HELLO_ACK,
    LEASE_POLL_SECONDS,
    LEASE_REVOKED,
    PROTOCOL_VERSION,
    WireError,
    event_hash,
    frame_bytes,
    make_home_record,
    parse_frame,
    read_json,
    scan_home_log,
)

EXIT_REVOKED = 75


@dataclass
class LeaseStatus:
    active: bool
    reason: str = ""
    by: str | None = None
    at: str | None = None


def check_lease(path: Path, token: str) -> LeaseStatus:
    """Return whether `token` is still the active lease."""
    document = read_json(path, default=None)
    if not isinstance(document, dict):
        return LeaseStatus(False, "lease file missing or invalid")
    if document.get("state") != "active":
        return LeaseStatus(
            False,
            str(document.get("reason") or f"lease state is {document.get('state')!r}"),
            document.get("by"),
            document.get("at"),
        )
    if document.get("token") != token:
        return LeaseStatus(
            False,
            str(document.get("reason") or "lease token changed"),
            document.get("by"),
            document.get("at"),
        )
    return LeaseStatus(True)


def _acquire_lock(path: Path, timeout: float = 10.0) -> int:
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    deadline = time.monotonic() + timeout
    while True:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            if time.monotonic() >= deadline:
                os.close(fd)
                raise WireError(f"session writer lock is still held: {path}")
            time.sleep(0.05)
    os.ftruncate(fd, 0)
    os.write(fd, f"{os.getpid()}\n".encode())
    os.fsync(fd)
    return fd


def _read_stdin(frames: "queue.Queue[bytes | None]") -> None:
    """Read raw bytes so a blocked reader never holds the buffered-stdin lock.

    The parent closes stdin when it is done. Reading through
    `sys.stdin.buffer` makes interpreter shutdown wait for a daemon thread that
    is blocked in a buffered read; `os.read` does not.
    """
    fd = sys.stdin.fileno()
    pending = b""
    try:
        while True:
            chunk = os.read(fd, 65536)
            if not chunk:
                break
            pending += chunk
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                frames.put(line + b"\n")
        if pending.strip():
            frames.put(pending)
    except OSError:
        pass
    finally:
        frames.put(None)


def _send(frame: dict[str, Any]) -> None:
    sys.stdout.buffer.write(frame_bytes(frame))
    sys.stdout.buffer.flush()


def _error(code: str, message: str) -> None:
    _send({"type": ERROR, "code": code, "message": message})


def _revoked_notice(status: LeaseStatus, session: str, last_seq: int) -> None:
    _send(
        {
            "type": LEASE_REVOKED,
            "session": session,
            "reason": status.reason,
            "by": status.by,
            "at": status.at,
            "last_seq": last_seq,
        }
    )


def _sigterm(_signum: int, _frame: Any) -> None:
    raise SystemExit(0)


def run_ingest(args: argparse.Namespace) -> int:
    session_dir = Path(args.session_dir)
    session_dir.mkdir(parents=True, exist_ok=True)
    log_path = session_dir / "log.jsonl"
    lease_path = Path(args.lease_file)

    lock_fd = _acquire_lock(session_dir / ".lock")
    log_fd: int | None = None
    try:
        home = scan_home_log(log_path)
        if home.truncated:
            print(
                f"ingest: truncated {home.torn_bytes} torn tail byte(s) from {log_path}",
                file=sys.stderr,
                flush=True,
            )
        log_fd = os.open(log_path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
        print(
            f"ingest: ready pid={os.getpid()} events={len(home.records)} last_seq={home.last_seq}",
            file=sys.stderr,
            flush=True,
        )

        signal.signal(signal.SIGHUP, _sigterm)
        signal.signal(signal.SIGTERM, _sigterm)

        frames: "queue.Queue[bytes | None]" = queue.Queue()
        reader = threading.Thread(target=_read_stdin, args=(frames,), daemon=True)
        reader.start()

        hello_token: str | None = None
        duplicates = 0

        def revoked(status: LeaseStatus) -> int:
            _revoked_notice(status, args.session, home.last_seq)
            print(
                f"ingest: lease revoked reason={status.reason!r} by={status.by!r}",
                file=sys.stderr,
                flush=True,
            )
            return EXIT_REVOKED

        while True:
            try:
                raw = frames.get(timeout=LEASE_POLL_SECONDS)
            except queue.Empty:
                if hello_token is not None:
                    status = check_lease(lease_path, hello_token)
                    if not status.active:
                        return revoked(status)
                continue

            if raw is None:
                # SSH dropped (EOF) without an `end` frame. Everything accepted
                # so far is already appended; the client replays the unacked tail.
                print("ingest: EOF", file=sys.stderr, flush=True)
                return 0

            try:
                frame = parse_frame(raw)
            except WireError as err:
                _error("bad-frame", str(err))
                return 2

            kind = frame["type"]
            if kind == HELLO:
                if hello_token is not None:
                    _error("duplicate-hello", "hello was already received")
                    return 2
                if frame.get("version") != PROTOCOL_VERSION:
                    _error("bad-version", f"protocol version is {PROTOCOL_VERSION}")
                    return 2
                if frame.get("session") != args.session:
                    _error("bad-session", f"session is {args.session!r}")
                    return 2
                token = frame.get("lease")
                if not isinstance(token, str) or not token:
                    _error("lease-invalid", "hello has no lease token")
                    return 2
                status = check_lease(lease_path, token)
                if not status.active:
                    return revoked(status)
                hello_token = token
                _send(
                    {
                        "type": HELLO_ACK,
                        "session": args.session,
                        "last_seq": home.last_seq,
                        "events": len(home.records),
                    }
                )
                continue

            if hello_token is None:
                _error("hello-required", "first frame must be hello")
                return 2

            status = check_lease(lease_path, hello_token)
            if not status.active:
                return revoked(status)

            if kind == HEARTBEAT:
                _send({"type": HEARTBEAT_ACK, "seq": home.last_seq, "ts": frame.get("ts")})
                continue

            if kind == END:
                if log_fd is not None:
                    os.fsync(log_fd)
                _send(
                    {
                        "type": DONE,
                        "session": args.session,
                        "events": len(home.records),
                        "last_seq": home.last_seq,
                        "duplicates": duplicates,
                        "last_client_seq": frame.get("last_client_seq"),
                    }
                )
                print(
                    f"ingest: done events={len(home.records)} last_seq={home.last_seq} duplicates={duplicates}",
                    file=sys.stderr,
                    flush=True,
                )
                return 0

            if kind != EVENT:
                _error("bad-frame", f"unknown frame type {kind!r}")
                return 2

            if frame.get("session") != args.session:
                _error("bad-session", f"event session is not {args.session!r}")
                return 2
            event_id = frame.get("id")
            event = frame.get("event")
            if not isinstance(event_id, str) or not event_id:
                _error("bad-event", "event has no id")
                return 2
            if not isinstance(event, dict):
                _error("bad-event", f"event {event_id} has no object payload")
                return 2

            digest = event_hash(event)
            existing = home.by_id.get(event_id)
            if existing is not None:
                if home.hashes[event_id] != digest:
                    _error(
                        "duplicate-conflict",
                        f"event id {event_id} was already stored with different content",
                    )
                    return 2
                duplicates += 1
                _send({"type": ACK, "id": event_id, "seq": existing, "duplicate": True})
                continue

            record = make_home_record(home.next_seq, frame)
            line = frame_bytes(record)
            written = os.write(log_fd, line)
            if written != len(line):
                _error("short-write", f"wrote {written} of {len(line)} bytes")
                return 2
            if args.fsync_each:
                os.fsync(log_fd)
            home.records.append(record)
            home.by_id[event_id] = record["seq"]
            home.hashes[event_id] = digest
            home.next_seq += 1
            _send({"type": ACK, "id": event_id, "seq": record["seq"], "duplicate": False})
    except BrokenPipeError:
        # The SSH channel went away while writing an ack. The append (if any)
        # stays; the client replays and the idempotency check answers it again.
        return 0
    finally:
        if log_fd is not None:
            try:
                os.fsync(log_fd)
            except OSError:
                pass
            os.close(log_fd)
        try:
            fcntl.flock(lock_fd, fcntl.LOCK_UN)
        except OSError:
            pass
        os.close(lock_fd)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="home-side ingest for the remote-exec spike")
    parser.add_argument("--session-dir", required=True, help="directory holding log.jsonl and .lock")
    parser.add_argument("--session", required=True, help="session id")
    parser.add_argument("--lease-file", required=True, help="JSON lease document watched while idle")
    parser.add_argument(
        "--fsync-each",
        action="store_true",
        help="fsync after every event (measurement option; production syncs at turn boundaries)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return run_ingest(args)
    except (WireError, OSError) as err:
        print(f"ingest: {err}", file=sys.stderr, flush=True)
        return 2


if __name__ == "__main__":
    sys.exit(main())
