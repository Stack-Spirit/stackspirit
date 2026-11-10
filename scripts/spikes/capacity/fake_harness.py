#!/usr/bin/env python3
"""A deterministic stand-in for a VOWP harness in the capacity spike.

The real harnesses (`opencode`, Claude Code) would call a model, spend tokens and
make a measurement non-reproducible.  This script exercises the same shape: it
allocates a fixed resident set, burns a little CPU, then stays alive while the
job's command gates run.  It never opens a socket and never starts another
harness.

It is deliberately small and boring so that the build, not the stand-in,
dominates the job's peak memory.  The real harness reference measured on this
VPS was about 90 MiB RSS per opencode worker (see the spike document).

Usage:
    fake_harness.py --mib 192 --cpu-ms 400 --ready /tmp/ready --stop /tmp/stop
"""

from __future__ import annotations

import argparse
import os
import sys
import time


def touch_pages(buf: bytearray, page: int = 4096) -> None:
    """Fault every page in so the allocation is resident, not lazy zero pages."""
    for offset in range(0, len(buf), page):
        buf[offset] = 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mib", type=int, default=192, help="resident memory to hold (MiB)")
    parser.add_argument("--cpu-ms", type=int, default=400, help="CPU time to burn before idling")
    parser.add_argument("--ready", required=True, help="file written once the harness is resident")
    parser.add_argument("--stop", required=True, help="file whose appearance ends the harness")
    args = parser.parse_args(argv)

    if args.mib < 0 or args.cpu_ms < 0:
        parser.error("--mib and --cpu-ms must be non-negative")

    buf = bytearray(args.mib * 1024 * 1024)
    touch_pages(buf)

    # Burn a small, fixed amount of CPU so the harness side of the CPU table is
    # not just process-start noise.  The loop is bounded by wall time spent in a
    # tight arithmetic loop, which is close enough for a spike.
    deadline = time.monotonic() + (args.cpu_ms / 1000.0)
    accumulator = 0
    while time.monotonic() < deadline:
        accumulator = (accumulator + 1) & 0xFFFFFFFF
    _ = accumulator

    with open(args.ready, "w", encoding="utf-8") as ready:
        ready.write(f"{os.getpid()}\n")
        ready.flush()
        os.fsync(ready.fileno())

    while not os.path.exists(args.stop):
        time.sleep(0.05)

    # Keep the pages referenced until the final read of /proc so the kernel does
    # not reclaim them before the last sample.
    touch_pages(buf)
    return 0


if __name__ == "__main__":
    sys.exit(main())
