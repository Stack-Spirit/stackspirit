#!/usr/bin/env python3
"""Map a captured opencode `/api/event` SSE stream to VOWP-shaped events.

This is a spike mapping, not a production adapter: it records the fields the
VOWP session log would need (type, actor, source, payload, timestamp) and
coalesces opencode's streaming/chunked events into durable log events.

Usage:
  map_events.py --sse events.sse --session ses_... --out vowp-events.jsonl
"""
import argparse
import collections
import datetime
import json
import sys


def rfc3339_millis(ms):
    dt = datetime.datetime.fromtimestamp(ms / 1000.0, tz=datetime.timezone.utc)
    return dt.strftime("%Y-%m-%dT%H:%M:%S.") + "%03dZ" % (int(ms) % 1000)


def event_ts(event):
    data = event.get("data") or {}
    ms = event.get("created")
    if ms is None:
        ms = (data.get("time") or {}).get("created")
    if ms is None:
        ms = 0
    return rfc3339_millis(int(ms))


def session_of(event):
    data = event.get("data") or {}
    sid = data.get("sessionID")
    if sid is None and event.get("type") == "session.created":
        sid = (data.get("data") or {}).get("sessionID")
    return sid


def map_event(event):
    """Returns a list of (vowp_type, actor, payload) tuples."""
    t = event.get("type")
    d = event.get("data") or {}
    if t == "session.created":
        return [("session.created", "system", {"title": d.get("title", "")})]
    if t == "session.inbox.enqueued":
        item = d.get("item") or {}
        if item.get("type") != "user":
            return []
        payload = item.get("payload") or {}
        return [("user.message", "user", {"text": payload.get("text", "")})]
    if t == "session.execution.started":
        return [("turn.started", "system", {})]
    if t == "session.text.ended":
        return [("assistant.message", "assistant", {"text": d.get("text", "")})]
    if t == "session.tool.called":
        return [("tool.call", "assistant", {"tool_call_id": d.get("id"), "input": d.get("input", {})})]
    if t == "session.tool.success":
        return [("tool.result", "tool", {"tool_call_id": d.get("id"), "ok": True, "content": d.get("content")})]
    if t == "session.tool.failed":
        err = (d.get("error") or {}).get("message", "")
        return [("tool.result", "tool", {"tool_call_id": d.get("id"), "ok": False, "error": err})]
    if t == "session.usage.updated":
        return [("cost", "system", {"usd": d.get("cost", 0), "tokens": d.get("tokens", {})})]
    if t == "session.execution.succeeded":
        return [("turn.ended", "system", {"outcome": "succeeded"})]
    if t == "session.execution.interrupted":
        return [("turn.ended", "system", {"outcome": "interrupted"})]
    if t == "session.execution.failed":
        err = (d.get("error") or {}).get("message", "session execution failed")
        return [("error", "system", {"message": err}), ("turn.ended", "system", {"outcome": "failed"})]
    if t == "session.step.failed":
        err = (d.get("error") or {}).get("message", "step failed")
        return [("error", "system", {"message": err})]
    return []


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sse", required=True)
    parser.add_argument("--session", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--model", default="fake-model")
    args = parser.parse_args()

    raw = []
    with open(args.sse, encoding="utf-8") as handle:
        for line in handle:
            if not line.startswith("data: "):
                continue
            try:
                event = json.loads(line[6:])
            except json.JSONDecodeError:
                continue
            if session_of(event) != args.session:
                continue
            raw.append(event)

    out = []
    skipped = collections.Counter()
    for event in raw:
        mapped = map_event(event)
        if not mapped:
            skipped[event.get("type", "?")] += 1
            continue
        for vowp_type, actor, payload in mapped:
            seq = len(out) + 1
            out.append({
                "id": "evt_s3_%04d" % seq,
                "session_id": args.session,
                "seq": seq,
                "ts": event_ts(event),
                "type": vowp_type,
                "actor": actor,
                "source": {"harness": "opencode", "model": args.model, "native_session_id": args.session},
                "payload": payload,
                "replayable": True,
            })

    with open(args.out, "w", encoding="utf-8") as handle:
        for envelope in out:
            handle.write(json.dumps(envelope, separators=(",", ":")) + "\n")

    counts = collections.Counter(e["type"] for e in out)
    summary = {
        "session_id": args.session,
        "vowp_events": dict(sorted(counts.items())),
        "opencode_events": dict(sorted(collections.Counter(e.get("type") for e in raw).items())),
        "skipped_opencode_events": dict(sorted(skipped.items())),
        "total_vowp_events": len(out),
    }
    print(json.dumps(summary, indent=2))
    if counts["user.message"] < 1 or counts["assistant.message"] < 1 or counts["turn.ended"] < 1:
        print("map_events: missing a user message, assistant message or turn end", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
