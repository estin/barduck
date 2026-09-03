#!/usr/bin/env python3
"""Mock upstream APIs for the home dashboard demo.

Serves slowly-drifting JSON so the dashboard shows changing values:
  GET /bank   -> {"balance": <float>}
  GET /work   -> {"hours_this_week": <float>}
  GET /report -> {"report": "# Weekly report ...markdown..."}
Run: python3 mock_server.py [port]   (default 8431)
"""
import json
import random
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

START = time.time()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        hours = 40.0 * (time.time() - START) % 40 + random.uniform(0, 0.5)
        report = (
            "# Weekly report\n\n"
            f"Hours logged: **{hours:.1f} h**\n\n"
            "- [x] incident review\n"
            "- [ ] on-call handover\n"
        )
        body = json.dumps(
            {
                "/bank": {"balance": round(1500 + random.uniform(-50, 50), 2)},
                "/work": {"hours_this_week": round(hours, 1)},
                "/report": {"report": report},
            }.get(self.path, {})
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass  # keep the demo terminal quiet


if __name__ == "__main__":
    import sys

    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8431
    print(f"mock upstream listening on http://127.0.0.1:{port}")
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
