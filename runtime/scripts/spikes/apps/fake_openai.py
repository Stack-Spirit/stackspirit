#!/usr/bin/env python3
"""A tiny OpenAI-compatible chat-completions server for the S3 spike.

It binds 127.0.0.1 only, has no provider keys, and answers every request
locally. With FAKE_TOOL_CALL=1 it first asks for the first tool offered by the
client (which proves the client exposed its MCP tool to the model) and, after
the tool result comes back, returns a short final answer.

The port is printed on stdout as the first line, so the caller can use port 0.
"""
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPLY = "hello from the fake local model"
FINAL_REPLY = "the fake model received the tool result"


def tool_name_from_request(req):
    """Returns the configured spike tool if the client offered it."""
    wanted = os.environ.get("FAKE_TOOL_NAME", "vowp_spike_echo")
    for tool in req.get("tools") or []:
        name = (tool.get("function") or {}).get("name")
        if name and (name == wanted or name.endswith("_" + wanted) or name.endswith("__" + wanted)):
            return name
    return None


def has_tool_result(req):
    for message in req.get("messages") or []:
        if message.get("role") == "tool":
            return True
    return False


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("fake-openai: " + (fmt % args) + "\n")
        sys.stderr.flush()

    def _json(self, status, body):
        data = json.dumps(body).encode()
        self.close_connection = True
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def _sse(self, chunks):
        self.close_connection = True
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        for chunk in chunks:
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def do_GET(self):
        if self.path.rstrip("/") == "/v1/models":
            self._json(200, {"object": "list", "data": [{"id": "fake-model", "object": "model", "created": 0, "owned_by": "spike"}]})
            return
        self._json(404, {"error": {"message": "not found"}})

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            req = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            req = {}
        if self.path.rstrip("/") != "/v1/chat/completions":
            self._json(404, {"error": {"message": "not found"}})
            return
        offered = [((t.get("function") or {}).get("name")) for t in (req.get("tools") or [])]
        tool = tool_name_from_request(req)
        want_tool = os.environ.get("FAKE_TOOL_CALL") == "1" and tool is not None and not has_tool_result(req)
        sys.stderr.write(
            "fake-openai: POST model=%s stream=%s tools=%s want_tool=%s tool_result=%s\n"
            % (req.get("model"), req.get("stream"), offered, want_tool, has_tool_result(req))
        )
        sys.stderr.flush()
        now = int(time.time())
        model = req.get("model", "fake-model")
        if req.get("stream"):
            chunks = [
                {"id": "chatcmpl-spike", "object": "chat.completion.chunk", "created": now, "model": model,
                 "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": None}]},
            ]
            if want_tool:
                args = json.dumps({"text": "mcp roundtrip"})
                chunks.append({
                    "id": "chatcmpl-spike", "object": "chat.completion.chunk", "created": now, "model": model,
                    "choices": [{"index": 0, "delta": {"tool_calls": [{
                        "index": 0, "id": "call_spike_1", "type": "function",
                        "function": {"name": tool, "arguments": args},
                    }]}, "finish_reason": None}],
                })
                chunks.append({"id": "chatcmpl-spike", "object": "chat.completion.chunk", "created": now, "model": model,
                               "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})
            else:
                text = FINAL_REPLY if has_tool_result(req) else REPLY
                chunks.append({"id": "chatcmpl-spike", "object": "chat.completion.chunk", "created": now, "model": model,
                               "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": None}]})
                chunks.append({"id": "chatcmpl-spike", "object": "chat.completion.chunk", "created": now, "model": model,
                               "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                               "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18}})
            self._sse(chunks)
            return
        if want_tool:
            self._json(200, {
                "id": "chatcmpl-spike", "object": "chat.completion", "created": now, "model": model,
                "choices": [{"index": 0, "message": {"role": "assistant", "content": None, "tool_calls": [{
                    "id": "call_spike_1", "type": "function",
                    "function": {"name": tool, "arguments": json.dumps({"text": "mcp roundtrip"})},
                }]}, "finish_reason": "tool_calls"}],
            })
            return
        text = FINAL_REPLY if has_tool_result(req) else REPLY
        self._json(200, {
            "id": "chatcmpl-spike", "object": "chat.completion", "created": now, "model": model,
            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18},
        })


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(server.server_address[1], flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
