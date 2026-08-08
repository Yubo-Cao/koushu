#!/usr/bin/env python3
"""Minimal OpenAI-compatible /v1/audio/transcriptions server for testing."""
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8811
MODE = sys.argv[2] if len(sys.argv) > 2 else "ok"

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        auth = self.headers.get("Authorization", "")
        ctype = self.headers.get("Content-Type", "")
        if MODE == "error":
            p = json.dumps({"error": {"message": "Invalid model 'nope'"}}).encode()
            self.send_response(404); self.send_header("Content-Type","application/json")
            self.send_header("Content-Length", str(len(p))); self.end_headers()
            self.wfile.write(p); return
        model = "?"
        if b'name="model"' in body:
            seg = body.split(b'name="model"')[1]
            model = seg.split(b"\r\n\r\n")[1].split(b"\r\n")[0].decode()
        has_file = b'name="file"' in body and b"RIFF" in body
        p = json.dumps({"text": f"MOCK model={model} multipart={'yes' if 'multipart/form-data' in ctype else 'no'} "
                                f"wav={'yes' if has_file else 'no'} auth={'yes' if auth.startswith('Bearer ') else 'no'} "
                                f"bytes={len(body)}"}).encode()
        self.send_response(200); self.send_header("Content-Type","application/json")
        self.send_header("Content-Length", str(len(p))); self.end_headers()
        self.wfile.write(p)

print(f"mock ASR on :{PORT} mode={MODE}", flush=True)
HTTPServer(("127.0.0.1", PORT), H).serve_forever()
