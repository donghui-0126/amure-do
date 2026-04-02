"""
amuredo Python HTTP Backend Server

Implements the amuredo backend protocol:
  GET  /health  -> {"status": "ok"}
  POST /exec    -> {"status": "ok", "output": "..."}

The /exec endpoint receives a JSON body with:
  {"code": "...", "timeout_secs": 300}

and executes the code string, capturing stdout.
"""

import json
import sys
import io
import os
import traceback
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path

# Optional: pre-import common analysis libraries so user code can use them
try:
    import pandas as pd
    import numpy as np
except ImportError:
    pd = None
    np = None

READY_FILE = Path("_ready")
HOST = "0.0.0.0"
PORT = int(os.environ.get("AMUREDO_PORT", "8090"))


def execute_code(code: str) -> tuple[str, bool]:
    """Execute a code string and return (output, success)."""
    stdout_capture = io.StringIO()
    old_stdout = sys.stdout
    sys.stdout = stdout_capture

    # Provide common imports in the execution namespace
    exec_globals = {
        "__builtins__": __builtins__,
        "pd": pd,
        "np": np,
        "json": json,
        "Path": Path,
    }

    try:
        exec(code, exec_globals)  # noqa: S102
        success = True
    except Exception:
        print(traceback.format_exc())
        success = False
    finally:
        sys.stdout = old_stdout

    return stdout_capture.getvalue(), success


class AmuredoHandler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # suppress default access logs
        pass

    def send_json(self, status: int, body: dict):
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        if self.path == "/health":
            self.send_json(200, {"status": "ok"})
        else:
            self.send_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/exec":
            self.send_json(404, {"error": "not found"})
            return

        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length)

        try:
            body = json.loads(raw)
        except json.JSONDecodeError:
            self.send_json(400, {"error": "invalid JSON"})
            return

        code = body.get("code", "")
        output, success = execute_code(code)

        if success:
            self.send_json(200, {"status": "ok", "output": output})
        else:
            self.send_json(200, {"status": "error", "output": output})


def main():
    server = HTTPServer((HOST, PORT), AmuredoHandler)
    print(f"amuredo backend listening on {HOST}:{PORT}", flush=True)

    # Signal readiness to amuredo engine
    READY_FILE.write_text("ready\n")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("Backend shutting down.")
    finally:
        if READY_FILE.exists():
            READY_FILE.unlink()


if __name__ == "__main__":
    main()
