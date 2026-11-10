#!/usr/bin/env python3
"""A tiny stdio MCP server with one tool, used by the S3 app-surface spike.

Speaks newline-delimited JSON-RPC 2.0 on stdin/stdout. It deliberately has no
dependencies and never touches the network. If MCP_DUMMY_LOG is set, it appends
the method names it receives (and the tool names it returns) to that file, so a
caller can prove which client actually connected.
"""
import json
import os
import sys

NAME = "vowp-spike"
VERSION = "1.0.0"
TOOL = {
    "name": "vowp_spike_echo",
    "description": "Echo a string back. Used to prove an MCP server is loaded.",
    "inputSchema": {
        "type": "object",
        "properties": {"text": {"type": "string", "description": "Text to echo"}},
        "required": ["text"],
        "additionalProperties": False,
    },
}


def log(line):
    path = os.environ.get("MCP_DUMMY_LOG")
    if not path:
        return
    try:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
    except OSError:
        pass


def send(message):
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def result(msg_id, value):
    send({"jsonrpc": "2.0", "id": msg_id, "result": value})


def error(msg_id, code, message):
    send({"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}})


def main():
    log("start pid=%d" % os.getpid())
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = msg.get("method")
        if "id" not in msg:
            log("notification %s" % method)
            continue  # notification, including notifications/initialized
        log("request %s" % method)
        msg_id = msg.get("id")
        params = msg.get("params") or {}
        if method == "initialize":
            result(msg_id, {
                "protocolVersion": params.get("protocolVersion", "2024-11-05"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": NAME, "version": VERSION},
            })
        elif method == "ping":
            result(msg_id, {})
        elif method == "tools/list":
            log("tools_list %s" % TOOL["name"])
            result(msg_id, {"tools": [TOOL]})
        elif method == "tools/call":
            name = params.get("name")
            args = params.get("arguments") or {}
            if name != TOOL["name"]:
                error(msg_id, -32602, "unknown tool: %s" % name)
                continue
            text = str(args.get("text", ""))
            result(msg_id, {
                "content": [{"type": "text", "text": "echo: " + text}],
                "isError": False,
            })
        elif method == "resources/list":
            result(msg_id, {"resources": []})
        elif method == "prompts/list":
            result(msg_id, {"prompts": []})
        else:
            error(msg_id, -32601, "method not found: %s" % method)


if __name__ == "__main__":
    main()
