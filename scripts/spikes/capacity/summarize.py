#!/usr/bin/env python3
"""Turn capacity-spike measurement JSON files into a table.

Usage:
    python3 summarize.py --out results/2026...-host [--mode host]
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
import sys
from pathlib import Path
from typing import Any

MIB = 1024 * 1024


def mib(value: Any) -> float | None:
    return round(value / MIB, 1) if isinstance(value, (int, float)) else None


def secs(value: Any) -> float | None:
    return round(value / 1_000_000, 2) if isinstance(value, (int, float)) else None


def load(path: Path) -> dict[str, Any] | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"summarize: skipping {path}: {error}", file=sys.stderr)
        return None


def row(data: dict[str, Any]) -> dict[str, Any]:
    summary = data.get("summary") or {}
    cgroup = data.get("cgroup") or {}
    proc = data.get("proc") or {}
    duration = data.get("duration_s")
    job_cpu = summary.get("job_cpu_usec")
    cores = round(job_cpu / 1_000_000 / duration, 2) if job_cpu and duration else None
    cgroup_peak = summary.get("job_peak_bytes")
    components = summary.get("job_peak_excluding_observer_bytes")
    proc_peak = summary.get("proc_job_peak_rss_bytes")
    return {
        "label": data.get("label"),
        "mode": data.get("cargo_mode"),
        "exit": data.get("build_exit_code"),
        "duration_s": duration,
        "build_duration_s": data.get("build_duration_s"),
        "job_peak_mib": mib(cgroup_peak),
        "components_mib": mib(components),
        "job_peak_upper_mib": mib(summary.get("job_peak_upper_bound_bytes")),
        "job_peak_anon_mib": mib(summary.get("job_peak_anon_bytes")),
        "harness_peak_mib": mib(summary.get("harness_peak_bytes")),
        "build_peak_mib": mib(summary.get("build_peak_bytes")),
        "proc_peak_mib": mib(proc_peak),
        "proc_job_peak_mib": mib(proc.get("job_peak_rss_bytes")),
        "proc_container_peak_mib": mib(proc.get("container_peak_rss_bytes")),
        "proc_over_cgroup": round(proc_peak / cgroup_peak, 3) if proc_peak and cgroup_peak else None,
        "proc_over_components": round(proc_peak / components, 3) if proc_peak and components else None,
        "job_cpu_s": secs(job_cpu),
        "observer_cpu_s": secs(summary.get("observer_cpu_usec")),
        "harness_cpu_s": secs(summary.get("harness_cpu_usec")),
        "build_cpu_s": secs(summary.get("build_cpu_usec")),
        "proc_cpu_s": secs(summary.get("proc_job_cpu_usec")),
        "avg_cores": cores,
        "proc_samples": proc.get("samples"),
        "container_found": bool((cgroup.get("container") or {}).get("id")),
        "errors": "; ".join(data.get("errors") or []),
    }


def mean(rows: list[dict[str, Any]], key: str) -> float | None:
    values = [r[key] for r in rows if isinstance(r.get(key), (int, float))]
    return round(statistics.fmean(values), 2) if values else None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="directory containing *.json")
    parser.add_argument("--mode", help="only include runs with this cargo mode")
    args = parser.parse_args(argv)

    files = sorted(args.out.glob("*.json"))
    rows = []
    for path in files:
        data = load(path)
        if data is None:
            continue
        if args.mode and data.get("cargo_mode") != args.mode:
            continue
        rows.append(row(data))
    if not rows:
        print(f"summarize: no measurement JSON in {args.out}", file=sys.stderr)
        return 1

    columns = [
        ("label", "run"),
        ("mode", "mode"),
        ("exit", "exit"),
        ("duration_s", "dur s"),
        ("job_peak_mib", "cg peak MiB"),
        ("components_mib", "cg parts MiB"),
        ("job_peak_anon_mib", "cg anon MiB"),
        ("harness_peak_mib", "harness MiB"),
        ("build_peak_mib", "build MiB"),
        ("proc_peak_mib", "proc peak MiB"),
        ("proc_over_cgroup", "proc/cg"),
        ("proc_over_components", "proc/parts"),
        ("job_cpu_s", "cpu s"),
        ("observer_cpu_s", "obs s"),
        ("harness_cpu_s", "h cpu s"),
        ("build_cpu_s", "b cpu s"),
        ("avg_cores", "cores"),
    ]
    header = "| " + " | ".join(title for _, title in columns) + " |"
    rule = "|" + "|".join("---" for _ in columns) + "|"
    lines = [header, rule]
    for r in rows:
        lines.append("| " + " | ".join("" if r[key] is None else str(r[key]) for key, _ in columns) + " |")

    means = {key: mean(rows, key) for key, _ in columns if key not in ("label", "mode", "exit")}
    lines.append("")
    lines.append("means: " + ", ".join(f"{title}={means[key]}" for key, title in columns if key in means and means[key] is not None))
    table = "\n".join(lines)

    summary = {
        "mode": args.mode,
        "runs": len(rows),
        "rows": rows,
        "means": means,
    }
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (args.out / "summary.md").write_text(table + "\n", encoding="utf-8")

    with open(args.out / "summary.csv", "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[key for key, _ in columns],
            extrasaction="ignore",
        )
        writer.writeheader()
        writer.writerows(rows)

    print(table)
    return 0


if __name__ == "__main__":
    sys.exit(main())
