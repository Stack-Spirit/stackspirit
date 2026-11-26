#!/usr/bin/env python3
"""CLI for the remote-exec streaming spike.

Subcommands:
    run       stream a synthetic harness batch through a transport
    resume    send the unacked remainder of a spool left by a killed client
    ingest    the home-side ingest process (normally started by the transport)
    fake-ssh  the hermetic SSH-shaped proxy used by tests
    takeover  revoke or replace the lease document the home watches
    verify    compare a home log with the client spool
    live      start a private loopback sshd and run the live measurements

The hermetic commands never use a network socket, the real `~/.vowp`, tmux or a
real harness. `live` binds a private `sshd` to 127.0.0.1 and is opt-in.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

import client
import fake_ssh
import ingest
import wire


def takeover_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="revoke or replace the spike lease")
    parser.add_argument("--lease-file", required=True)
    parser.add_argument("--current-token", default="tok-spike")
    parser.add_argument("--new-token", default=None)
    parser.add_argument("--by", default="phone")
    parser.add_argument("--reason", default="taken over by phone")
    args = parser.parse_args(argv)
    if args.new_token:
        document = wire.lease_document(args.new_token, state="active", holder=args.by)
    else:
        document = wire.lease_document(
            args.current_token,
            state="revoked",
            holder="laptop",
            by=args.by,
            reason=args.reason,
        )
    written_at = time.time()
    wire.write_json_atomic(Path(args.lease_file), document)
    print(json.dumps({"written_at": written_at, "document": document}, sort_keys=True))
    return 0


def verify_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="verify a spike home log against the spool")
    parser.add_argument("--home-log", required=True)
    parser.add_argument("--spool", required=True)
    parser.add_argument("--out", default=None)
    args = parser.parse_args(argv)
    report = wire.verify_stream(Path(args.home_log), Path(args.spool))
    if args.out:
        wire.write_json_atomic(Path(args.out), report)
    print(json.dumps(report, sort_keys=True, indent=2))
    return 0 if report["ok"] else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="remote-exec streaming spike")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("run", help="stream a synthetic batch")
    sub.add_parser("resume", help="resume an existing spool")
    sub.add_parser("ingest", help="home-side ingest")
    sub.add_parser("fake-ssh", help="hermetic SSH-shaped proxy")
    sub.add_parser("takeover", help="revoke the lease document")
    sub.add_parser("verify", help="verify the home log")
    sub.add_parser("live", help="run the opt-in loopback sshd measurements")
    return parser


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        build_parser().print_help()
        return 2
    command, rest = argv[0], argv[1:]
    if command == "run":
        return client.main(rest, resume=False)
    if command == "resume":
        return client.main(rest, resume=True)
    if command == "ingest":
        return ingest.main(rest)
    if command == "fake-ssh":
        return fake_ssh.main(rest)
    if command == "takeover":
        return takeover_main(rest)
    if command == "verify":
        return verify_main(rest)
    if command == "live":
        import live_check

        return live_check.main(rest)
    print(f"unknown command: {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
