#!/usr/bin/env python3
"""Hermetic tests for the remote-exec streaming spike.

Every test uses a temporary HOME, VOWP_HOME and session directory, the fake SSH
proxy and a synthetic event source. Nothing here starts sshd, tmux, a real
harness, or touches the repository's real home.
"""

from __future__ import annotations

import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

SPIKE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SPIKE))

import wire  # noqa: E402

SESSION = "ses-test"
TOKEN = "tok-test"


class SpikeTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="vowp-remote-exec-test.")
        self.root = Path(self.temp.name)
        self.work = self.root / "work"
        self.home = self.root / "home"
        self.work.mkdir()
        self.home.mkdir()
        (self.root / "fake-home").mkdir()
        (self.root / "tmp").mkdir()
        (self.root / "vowp-home").mkdir()
        self.lease_file = self.root / "lease.json"
        self.plan_file = self.root / "plan.json"
        self.trace_file = self.root / "trace.jsonl"
        self.env = os.environ.copy()
        self.env.update(
            {
                "HOME": str(self.root / "fake-home"),
                "TMPDIR": str(self.root / "tmp"),
                "VOWP_HOME": str(self.root / "vowp-home"),
            }
        )
        self.env.pop("TMUX", None)
        self.env.pop("TMUX_PANE", None)
        self.write_lease()
        self.write_plan([])

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write_lease(self, *, token: str = TOKEN, state: str = "active", by: str | None = None,
                    reason: str | None = None) -> None:
        wire.write_json_atomic(
            self.lease_file,
            wire.lease_document(token, state=state, by=by, reason=reason),
        )

    def write_plan(self, cuts: list[dict[str, object]], *, name: str = "plan.json") -> Path:
        path = self.root / name
        path.write_text(json.dumps({"cuts": cuts}), encoding="utf-8")
        return path

    def spike_args(self) -> list[str]:
        return [
            sys.executable,
            "-B",
            str(SPIKE / "remote_exec_spike.py"),
        ]

    def run_client(
        self,
        *,
        events: int,
        window: int = 4,
        plan: Path | None = None,
        max_reconnects: int = 5,
        event_interval: float = 0.0,
        heartbeat_interval: float = 5.0,
        resume: bool = False,
        extra: list[str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        command = self.spike_args()
        command.append("resume" if resume else "run")
        command += [
            "--workdir", str(self.work),
            "--home-dir", str(self.home),
            "--lease-file", str(self.lease_file),
            "--lease-token", TOKEN,
            "--session", SESSION,
            "--fake-ssh-plan", str(plan or self.plan_file),
            "--fake-ssh-trace", str(self.trace_file),
            "--window", str(window),
            "--heartbeat-interval", str(heartbeat_interval),
            "--max-reconnects", str(max_reconnects),
        ]
        if not resume:
            command += ["--events", str(events), "--event-interval", str(event_interval)]
        command += extra or []
        return subprocess.run(
            command,
            cwd=SPIKE,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )

    def summary(self) -> dict[str, object]:
        return json.loads((self.work / "summary.json").read_text(encoding="utf-8"))

    def verify(self) -> dict[str, object]:
        return wire.verify_stream(self.home / "log.jsonl", self.work / "spool.jsonl")

    def trace(self) -> list[dict[str, object]]:
        if not self.trace_file.exists():
            return []
        return [json.loads(line) for line in self.trace_file.read_text(encoding="utf-8").splitlines() if line]

    def start_ingest(self) -> tuple[subprocess.Popen[bytes], "queue.Queue[dict[str, object]]"]:
        process = subprocess.Popen(
            [
                sys.executable,
                "-B",
                str(SPIKE / "ingest.py"),
                "--session-dir",
                str(self.home),
                "--session",
                SESSION,
                "--lease-file",
                str(self.lease_file),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=self.env,
        )
        frames: "queue.Queue[dict[str, object]]" = queue.Queue()

        def read() -> None:
            assert process.stdout is not None
            for line in process.stdout:
                try:
                    frames.put(wire.parse_frame(line))
                except wire.WireError:
                    pass

        threading.Thread(target=read, daemon=True).start()
        return process, frames

    def ingest_once(
        self,
        frames: list[dict[str, object]],
        *,
        lease_token: str = TOKEN,
        lease_state: str = "active",
    ) -> subprocess.CompletedProcess[bytes]:
        self.write_lease(token=lease_token, state=lease_state)
        payload = b"".join(wire.frame_bytes(frame) for frame in frames)
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(SPIKE / "ingest.py"),
                "--session-dir",
                str(self.home),
                "--session",
                SESSION,
                "--lease-file",
                str(self.lease_file),
            ],
            input=payload,
            capture_output=True,
            timeout=15,
            env=self.env,
            check=False,
        )

    @staticmethod
    def hello(token: str = TOKEN) -> dict[str, object]:
        return {"type": "hello", "version": 1, "session": SESSION, "lease": token}

    @staticmethod
    def ack_frames(result: subprocess.CompletedProcess[bytes]) -> list[dict[str, object]]:
        return [json.loads(line) for line in result.stdout.decode().splitlines() if line.strip()]

    def home_lines(self) -> list[dict[str, object]]:
        path = self.home / "log.jsonl"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]

    # --- end-to-end replay -------------------------------------------------

    def test_stream_survives_three_ssh_cuts(self) -> None:
        self.write_plan(
            [
                {"connection": 0, "after_lines": 37, "mode": "ack_lost"},
                {"connection": 1, "after_lines": 41, "mode": "mid_line"},
                {"connection": 2, "after_lines": 23, "mode": "ack_lost"},
            ]
        )
        result = self.run_client(events=200, window=4, max_reconnects=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        summary = self.summary()
        self.assertEqual(summary["status"], "ok")
        self.assertGreaterEqual(summary["reconnects"], 3)
        self.assertGreaterEqual(summary["resent"], 1)
        self.assertGreaterEqual(summary["duplicate_acks"], 1)

        report = self.verify()
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["events"], 200)
        self.assertEqual(report["home_records"], 200)
        self.assertEqual(report["last_seq"], 200)

        cuts = [event for event in self.trace() if event["event"] == "cut"]
        self.assertEqual(len(cuts), 3)
        self.assertEqual(
            {cut["mode"] for cut in cuts},
            {"ack_lost", "mid_line"},
        )
        self.assertTrue(any(cut["mode"] == "ack_lost" and cut["ack_seen"] for cut in cuts))
        mid_line = next(cut for cut in cuts if cut["mode"] == "mid_line")
        self.assertGreater(mid_line["bytes"], 0)

    def test_large_events_survive_a_cut(self) -> None:
        self.write_plan([{"connection": 0, "after_lines": 10, "mode": "ack_lost"}])
        result = self.run_client(
            events=20,
            window=2,
            max_reconnects=2,
            extra=["--text-size", "8192"],
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = self.verify()
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["home_records"], 20)

    def test_client_crash_resumes_from_spool(self) -> None:
        self.write_plan([{"connection": 0, "after_lines": 60, "mode": "ack_lost"}])
        first = self.run_client(events=120, window=1, max_reconnects=0)
        self.assertEqual(first.returncode, 75, first.stderr)
        first_summary = self.summary()
        self.assertEqual(first_summary["status"], "reconnect-limit")
        before = self.home_lines()
        self.assertGreater(len(before), 0)
        self.assertLess(len(before), 120)
        spool_lines = (self.work / "spool.jsonl").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(spool_lines), 120)

        plan = self.write_plan([], name="resume-plan.json")
        second = self.run_client(
            events=0,
            window=4,
            plan=plan,
            max_reconnects=2,
            resume=True,
        )
        self.assertEqual(second.returncode, 0, second.stderr)
        second_summary = self.summary()
        self.assertEqual(second_summary["status"], "ok")
        self.assertGreaterEqual(second_summary["duplicate_acks"], 1)
        report = self.verify()
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["home_records"], 120)

    # --- ingest invariants -------------------------------------------------

    def test_ingest_assigns_contiguous_seq_and_dedupes_by_id(self) -> None:
        one = wire.make_event(1, SESSION)
        two = wire.make_event(2, SESSION)
        result = self.ingest_once([self.hello(), one, one, two, {"type": "end", "last_client_seq": 2}])
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        frames = self.ack_frames(result)
        acks = [frame for frame in frames if frame["type"] == "ack"]
        self.assertEqual([frame["seq"] for frame in acks], [1, 1, 2])
        self.assertEqual([frame["duplicate"] for frame in acks], [False, True, False])
        done = next(frame for frame in frames if frame["type"] == "done")
        self.assertEqual(done["events"], 2)
        self.assertEqual(done["last_seq"], 2)
        lines = self.home_lines()
        self.assertEqual([line["seq"] for line in lines], [1, 2])
        self.assertEqual([line["id"] for line in lines], [one["id"], two["id"]])

    def test_ingest_rejects_reused_id_with_different_content(self) -> None:
        one = wire.make_event(1, SESSION)
        changed = json.loads(json.dumps(one))
        changed["event"]["payload"]["text"] = "different"
        result = self.ingest_once([self.hello(), one, changed])
        self.assertEqual(result.returncode, 2)
        frames = self.ack_frames(result)
        error = next(frame for frame in frames if frame["type"] == "error")
        self.assertEqual(error["code"], "duplicate-conflict")
        self.assertEqual(len(self.home_lines()), 1)

    def test_ingest_rejects_a_lease_that_is_not_active(self) -> None:
        one = wire.make_event(1, SESSION)
        result = self.ingest_once([self.hello("tok-wrong"), one])
        self.assertIn(result.returncode, (2, 75))
        frames = self.ack_frames(result)
        self.assertTrue(any(frame["type"] in ("error", "lease.revoked") for frame in frames))
        self.assertEqual(self.home_lines(), [])

    def test_ingest_truncates_a_torn_final_line_before_appending(self) -> None:
        log = self.home / "log.jsonl"
        for number in (1, 2, 3):
            frame = wire.make_event(number, SESSION)
            record = wire.make_home_record(number, frame)
            with log.open("ab") as handle:
                handle.write(wire.frame_bytes(record))
        with log.open("ab") as handle:
            handle.write(b'{"seq":4,"id":"evt-000004"')
        result = self.ingest_once([self.hello(), wire.make_event(4, SESSION), {"type": "end"}])
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn("truncated", result.stderr.decode())
        lines = self.home_lines()
        self.assertEqual([line["seq"] for line in lines], [1, 2, 3, 4])
        self.assertTrue(log.read_bytes().endswith(b"\n"))

    def test_scan_home_log_rejects_a_gap(self) -> None:
        log = self.home / "log.jsonl"
        one = wire.make_home_record(1, wire.make_event(1, SESSION))
        three = wire.make_home_record(3, wire.make_event(3, SESSION))
        log.write_bytes(wire.frame_bytes(one) + wire.frame_bytes(three))
        with self.assertRaises(wire.WireError):
            wire.scan_home_log(log)

    def test_verify_rejects_a_log_with_a_gap(self) -> None:
        log = self.home / "log.jsonl"
        one = wire.make_event(1, SESSION)
        two = wire.make_event(2, SESSION)
        (self.work / "spool.jsonl").write_bytes(wire.frame_bytes(one) + wire.frame_bytes(two))
        log.write_bytes(
            wire.frame_bytes(wire.make_home_record(1, one))
            + wire.frame_bytes(wire.make_home_record(3, two))
        )
        report = wire.verify_stream(log, self.work / "spool.jsonl")
        self.assertFalse(report["ok"])
        self.assertTrue(report["errors"])

    # --- lease takeover ----------------------------------------------------

    def test_lease_revoked_notice_reaches_the_client_within_2s(self) -> None:
        process, frames = self.start_ingest()
        try:
            assert process.stdin is not None
            process.stdin.write(wire.frame_bytes(self.hello()))
            process.stdin.write(wire.frame_bytes(wire.make_event(1, SESSION)))
            process.stdin.flush()
            self.assertEqual(frames.get(timeout=5)["type"], "hello.ack")
            self.assertEqual(frames.get(timeout=5)["type"], "ack")

            started = time.monotonic()
            wire.write_json_atomic(
                self.lease_file,
                wire.lease_document(
                    TOKEN,
                    state="revoked",
                    by="phone",
                    reason="taken over by phone",
                ),
            )
            notice = frames.get(timeout=2.5)
            elapsed_ms = (time.monotonic() - started) * 1000.0
            self.assertEqual(notice["type"], "lease.revoked")
            self.assertLess(elapsed_ms, 2000.0)
            self.assertEqual(process.wait(timeout=5), 75)
            time.sleep(0.3)
            self.assertEqual(len(self.home_lines()), 1)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream is not None:
                    stream.close()


if __name__ == "__main__":
    unittest.main()
