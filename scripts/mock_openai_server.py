#!/usr/bin/env python3
"""最小 OpenAI 兼容 /v1/chat/completions 服务器，用来验证流式客户端。

覆盖真实服务端会出现的几种情况：SSE 分块、keepalive 注释行、
非 data 行、[DONE] 结束、以及错误响应体。
"""
import json, sys, time
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8799
MODE = sys.argv[2] if len(sys.argv) > 2 else "ok"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        auth = self.headers.get("Authorization", "")

        if MODE == "error":
            payload = json.dumps({
                "error": {"message": "Incorrect API key provided: sk-***"}
            }).encode()
            self.send_response(401)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return

        if MODE == "empty":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
            return

        # Echo enough back that the test can assert the request was well formed.
        sys_prompt = next((m["content"] for m in body["messages"]
                           if m["role"] == "system"), "")
        user_text = next((m["content"] for m in body["messages"]
                          if m["role"] == "user"), "")
        marker = "STREAM" if body.get("stream") else "NOSTREAM"
        has_auth = "AUTH" if auth.startswith("Bearer ") else "NOAUTH"
        pieces = [
            "# ", "Formatted\n\n",
            f"{marker} {has_auth} ",
            f"model={body.get('model')} ",
            f"sys={len(sys_prompt)}chars ",
            f"got={user_text!r}",
        ]

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        # A real server interleaves comments and blank lines; the client must
        # skip them rather than treat them as content.
        self.wfile.write(b": ping\n\n")
        self.wfile.flush()
        for piece in pieces:
            chunk = {"choices": [{"delta": {"content": piece}}]}
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()
            time.sleep(0.05)
        # An unknown chunk shape must not abort the stream.
        self.wfile.write(b"data: {\"choices\":[{\"delta\":{}}]}\n\n")
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


print(f"mock LLM on :{PORT} mode={MODE}", flush=True)
HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
