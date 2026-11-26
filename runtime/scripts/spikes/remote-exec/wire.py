#!/usr/bin/env python3
"""Wire helpers for the remote-exec streaming spike.

The spike models the v0.2 remote-executor path without touching the real VOWP
home, database, tmux server, network or harnesses:

    client (laptop)                         home (VPS)
    fake harness event  --JSON line-->  session ingest
         ^                                  |
         |          ack / notice            |
         +---------- JSON line -------------+

`wire.py` owns the frame format, the append-only home-log scanner, event
generation and the latency summary used by every other module.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

PROTOCOL_VERSION = 1
LEASE_POLL_SECONDS = 0.1

# Frame types on the client -> home stream.
HELLO = "hello"
EVENT = "event"
HEARTBEAT = "heartbeat"
END = "end"

# Frame types on the home -> client stream.
HELLO_ACK = "hello.ack"
ACK = "ack"
HEARTBEAT_ACK = "heartbeat.ack"
LEASE_REVOKED = "lease.revoked"
DONE = "done"
ERROR = "error"


class WireError(Exception):
    """A malformed frame or an invalid home log."""


def iso_now() -> str:
    """UTC timestamp with millisecond precision, like the VOWP wire types."""
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def mononow_ms() -> float:
    """Monotonic milliseconds, used only for latency measurements."""
    return time.monotonic_ns() / 1_000_000.0


def canonical_bytes(value: Any) -> bytes:
    """Stable JSON bytes: sorted keys, no whitespace, UTF-8."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def frame_bytes(frame: dict[str, Any]) -> bytes:
    """One wire frame as a single newline-terminated JSON line."""
    return canonical_bytes(frame) + b"\n"


def parse_frame(line: bytes | str) -> dict[str, Any]:
    """Parse one frame; reject a blank line, non-object and non-JSON line."""
    if isinstance(line, bytes):
        text = line.decode("utf-8", "strict").strip()
    else:
        text = line.strip()
    if not text:
        raise WireError("blank frame")
    try:
        value = json.loads(text)
    except json.JSONDecodeError as err:
        raise WireError(f"invalid JSON frame: {err}") from err
    if not isinstance(value, dict):
        raise WireError("frame is not a JSON object")
    if not isinstance(value.get("type"), str):
        raise WireError("frame has no string `type`")
    return value


def event_hash(event: dict[str, Any]) -> str:
    """Content hash used to detect a reused event id with different content."""
    return hashlib.sha256(canonical_bytes(event)).hexdigest()


def make_event(n: int, session: str, text_size: int = 48) -> dict[str, Any]:
    """A deterministic fake harness event. `n` starts at 1."""
    text = (f"event {n:04d} " + ("x" * max(0, text_size)))[: max(16, text_size)]
    return {
        "type": EVENT,
        "id": f"evt-{n:06d}",
        "session": session,
        "event": {
            "type": "assistant.message",
            "actor": "assistant",
            "ts": iso_now(),
            "payload": {"n": n, "text": text},
        },
    }


def make_event_frames(count: int, session: str, text_size: int = 48) -> list[dict[str, Any]]:
    return [make_event(n, session, text_size) for n in range(1, count + 1)]


def make_home_record(seq: int, frame: dict[str, Any]) -> dict[str, Any]:
    """What the home appends to its log for one accepted event frame."""
    return {
        "seq": seq,
        "id": frame["id"],
        "session": frame["session"],
        "received_at": iso_now(),
        "event": frame["event"],
    }


@dataclass
class HomeLog:
    """The result of scanning a home log before the writer appends."""

    records: list[dict[str, Any]] = field(default_factory=list)
    by_id: dict[str, int] = field(default_factory=dict)
    hashes: dict[str, str] = field(default_factory=dict)
    next_seq: int = 1
    torn_bytes: int = 0
    truncated: bool = False

    @property
    def last_seq(self) -> int:
        return self.next_seq - 1


def scan_home_log(path: Path) -> HomeLog:
    """Read and verify an append-only home log, truncating a torn final line.

    A torn final line is bytes after the last newline: the process may have been
    killed mid-write. A *complete* line (newline present) that is invalid JSON or
    has the wrong sequence number is corruption and is never silently repaired.
    """
    result = HomeLog()
    if not path.exists():
        return result

    data = path.read_bytes()
    if data and not data.endswith(b"\n"):
        cut = data.rfind(b"\n") + 1
        result.torn_bytes = len(data) - cut
        with path.open("r+b") as handle:
            handle.truncate(cut)
            handle.flush()
            os.fsync(handle.fileno())
        result.truncated = True
        data = data[:cut]

    expected = 1
    for raw in data.splitlines():
        if not raw.strip():
            continue
        try:
            record = json.loads(raw)
        except json.JSONDecodeError as err:
            raise WireError(f"home log line {expected}: invalid JSON: {err}") from err
        if not isinstance(record, dict):
            raise WireError(f"home log line {expected}: not an object")
        if record.get("seq") != expected:
            raise WireError(
                f"home log line {expected}: seq {record.get('seq')!r} expected {expected}"
            )
        event_id = record.get("id")
        event = record.get("event")
        if not isinstance(event_id, str) or not isinstance(event, dict):
            raise WireError(f"home log line {expected}: missing id or event")
        if event_id in result.by_id:
            raise WireError(f"home log line {expected}: duplicate event id {event_id}")
        result.by_id[event_id] = expected
        result.hashes[event_id] = event_hash(event)
        result.records.append(record)
        expected += 1
    result.next_seq = expected
    return result


def append_home_record(path: Path, record: dict[str, Any], fsync: bool) -> None:
    """Append exactly one line with one write, optionally fsyncing it."""
    path.parent.mkdir(parents=True, exist_ok=True)
    line = frame_bytes(record)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        written = os.write(fd, line)
        if written != len(line):
            raise WireError(f"short append: wrote {written} of {len(line)} bytes")
        if fsync:
            os.fsync(fd)
    finally:
        os.close(fd)


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    values: list[dict[str, Any]] = []
    with path.open("rb") as handle:
        for number, line in enumerate(handle, 1):
            if not line.strip():
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as err:
                raise WireError(f"{path.name} line {number}: invalid JSON: {err}") from err
            if not isinstance(value, dict):
                raise WireError(f"{path.name} line {number}: not an object")
            values.append(value)
    return values


def write_json_atomic(path: Path, value: Any) -> None:
    """Write a JSON document (tmp + rename + fsync) so readers never see half."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.name}.tmp.{os.getpid()}.{time.monotonic_ns()}")
    with tmp.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, sort_keys=True, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(tmp, path)


def read_json(path: Path, default: Any = None) -> Any:
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except FileNotFoundError:
        return default
    except json.JSONDecodeError as err:
        raise WireError(f"{path.name}: invalid JSON: {err}") from err


def percentile(sorted_values: list[float], fraction: float) -> float:
    if not sorted_values:
        return 0.0
    index = int(math.ceil(fraction * len(sorted_values))) - 1
    index = min(max(index, 0), len(sorted_values) - 1)
    return sorted_values[index]


def latency_summary(values: Iterable[float]) -> dict[str, Any]:
    """p50/p90/p95/p99/max in milliseconds, rounded for human evidence."""
    raw = [float(value) for value in values]
    if not raw:
        return {"count": 0}
    ordered = sorted(raw)

    def rounded(value: float) -> float:
        return round(value, 3)

    return {
        "count": len(ordered),
        "mean_ms": rounded(sum(ordered) / len(ordered)),
        "p50_ms": rounded(percentile(ordered, 0.50)),
        "p90_ms": rounded(percentile(ordered, 0.90)),
        "p95_ms": rounded(percentile(ordered, 0.95)),
        "p99_ms": rounded(percentile(ordered, 0.99)),
        "max_ms": rounded(ordered[-1]),
        "min_ms": rounded(ordered[0]),
    }


def verify_stream(home_log: Path, spool: Path) -> dict[str, Any]:
    """Compare the home log with the client's complete spool.

    A correct stream has exactly one home record per spool event, in the same
    order, with seq 1..N and identical event content. Replayed duplicates are
    allowed on the wire but must not appear in the home log.
    """
    errors: list[str] = []
    spool_frames = read_jsonl(spool)
    expected = [frame for frame in spool_frames if frame.get("type") == EVENT]

    try:
        home = scan_home_log(home_log).records
    except WireError as err:
        return {"ok": False, "errors": [str(err)], "events": len(expected)}

    if len(home) != len(expected):
        errors.append(f"home has {len(home)} records, spool has {len(expected)} events")
    if len({record.get("id") for record in home}) != len(home):
        errors.append("home log has duplicate event ids")
    for index, (record, frame) in enumerate(zip(home, expected), 1):
        if record.get("seq") != index:
            errors.append(f"record {index}: seq {record.get('seq')!r}")
        if record.get("id") != frame.get("id"):
            errors.append(
                f"record {index}: id {record.get('id')!r} expected {frame.get('id')!r}"
            )
        elif event_hash(record.get("event", {})) != event_hash(frame.get("event", {})):
            errors.append(f"record {index}: event content differs for {record.get('id')}")
    return {
        "ok": not errors,
        "errors": errors,
        "events": len(expected),
        "home_records": len(home),
        "last_seq": home[-1]["seq"] if home else 0,
    }


def lease_document(
    token: str,
    *,
    state: str = "active",
    holder: str = "laptop",
    by: str | None = None,
    reason: str | None = None,
) -> dict[str, Any]:
    return {
        "token": token,
        "state": state,
        "holder": holder,
        "by": by,
        "reason": reason,
        "at": iso_now(),
    }
