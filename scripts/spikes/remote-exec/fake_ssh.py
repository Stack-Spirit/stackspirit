#!/usr/bin/env python3
"""A fake `ssh` for hermetic remote-exec tests.

The real design runs the ingest process on the home over an SSH channel. This
proxy has the same shape (client stdin -> remote stdin, remote stdout -> client
stdout) and can cut the channel at a chosen line, either after the home appended
the event but before its ack reached the client (`ack_lost`) or in the middle of
a line (`mid_line`). It never opens a socket.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

from wire import parse_frame

CUT_EXIT = 255


class FakeSsh:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.proc: subprocess.Popen[bytes] | None = None
        self.hold_output = False
        self.stop = threading.Event()
        self.cut_ack_seen = threading.Event()
        self.cut_id: str | None = None
        self.trace_lock = threading.Lock()
        self.trace_path = Path(args.trace)
        self.trace_path.parent.mkdir(parents=True, exist_ok=True)

    def trace(self, event: str, **fields: Any) -> None:
        line = {
            "at": time.time(),
            "conn": self.args.conn_index,
            "event": event,
            **fields,
        }
        with self.trace_lock:
            with self.trace_path.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps(line, sort_keys=True) + "\n")

    def _load_cut(self) -> dict[str, Any] | None:
        if not self.args.plan:
            return None
        try:
            plan = json.loads(Path(self.args.plan).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as err:
            self.trace("plan-error", error=str(err))
            return None
        for cut in plan.get("cuts", []):
            if cut.get("connection") == self.args.conn_index:
                return cut
        return None

    def _pump_output(self) -> None:
        assert self.proc is not None and self.proc.stdout is not None
        try:
            for raw in self.proc.stdout:
                parsed: dict[str, Any] = {}
                try:
                    parsed = parse_frame(raw)
                except Exception:
                    pass
                if parsed.get("type") == "ack" and parsed.get("id") == self.cut_id:
                    self.cut_ack_seen.set()
                if self.hold_output:
                    self.trace("held_output", kind=parsed.get("type"), id=parsed.get("id"))
                    continue
                sys.stdout.buffer.write(raw)
                sys.stdout.buffer.flush()
        except (BrokenPipeError, OSError):
            pass

    def _kill_ingest(self) -> None:
        if self.proc is None or self.proc.poll() is not None:
            return
        try:
            os.killpg(self.proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass

    def _signal(self, signum: int, _frame: Any) -> None:
        self.stop.set()
        self._kill_ingest()
        raise SystemExit(128 + signum)

    def run(self) -> int:
        cut = self._load_cut()
        self.trace("connect", cut=cut)
        self.proc = subprocess.Popen(
            self.args.command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,
            bufsize=0,
            start_new_session=True,
        )
        signal.signal(signal.SIGTERM, self._signal)
        signal.signal(signal.SIGINT, self._signal)
        output = threading.Thread(target=self._pump_output, daemon=True)
        output.start()

        assert self.proc.stdin is not None
        line_number = 0
        try:
            for raw in sys.stdin.buffer:
                if not raw:
                    break
                line_number += 1
                frame_id = None
                frame_type = None
                try:
                    parsed = parse_frame(raw)
                    frame_type = parsed.get("type")
                    frame_id = parsed.get("id")
                except Exception:
                    pass

                if cut is not None and line_number == int(cut.get("after_lines", 0)):
                    mode = str(cut.get("mode", "ack_lost"))
                    self.trace(
                        "cut_begin",
                        mode=mode,
                        line=line_number,
                        id=frame_id,
                        frame_type=frame_type,
                    )
                    self.hold_output = True
                    if mode == "mid_line":
                        prefix = raw[: max(1, len(raw) // 2)]
                        self.proc.stdin.write(prefix)
                        self.proc.stdin.flush()
                        time.sleep(0.05)
                        self.trace("cut", mode=mode, line=line_number, ack_seen=False, bytes=len(prefix))
                    else:
                        self.cut_id = frame_id
                        self.proc.stdin.write(raw)
                        self.proc.stdin.flush()
                        ack_seen = self.cut_ack_seen.wait(timeout=2.0)
                        self.trace(
                            "cut",
                            mode=mode,
                            line=line_number,
                            id=frame_id,
                            ack_seen=ack_seen,
                        )
                    self._kill_ingest()
                    return CUT_EXIT

                try:
                    self.proc.stdin.write(raw)
                    self.proc.stdin.flush()
                except (BrokenPipeError, OSError):
                    break
                if line_number <= 20 or line_number % 100 == 0:
                    self.trace("forward", line=line_number, id=frame_id, frame_type=frame_type)
        except (BrokenPipeError, OSError):
            pass
        finally:
            try:
                if self.proc.stdin is not None:
                    self.proc.stdin.close()
            except OSError:
                pass

        try:
            code = self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self._kill_ingest()
            code = CUT_EXIT
        output.join(timeout=5)
        self.trace("exit", code=code)
        return code


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="fake ssh transport for the remote-exec spike")
    parser.add_argument("--plan", help="JSON cut plan")
    parser.add_argument("--conn-index", type=int, required=True)
    parser.add_argument("--trace", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER, help="command to run as the home ingest")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command and args.command[0] == "--":
        args.command = args.command[1:]
    if not args.command:
        print("fake-ssh: no command", file=sys.stderr)
        return 2
    return FakeSsh(args).run()


if __name__ == "__main__":
    sys.exit(main())
