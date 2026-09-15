"""Exercise shipped viewer bootstrap, HTTP logs/capture, and security teardown.

Run with a built fastled binary inside an isolated native desktop (e.g. Xvfb).
The fixture uses a real 2D canvas and a minimal ready-event source, not a WASM
sketch or render worker. This does not replace real sketch/media/DPI validation.
"""

import argparse
import json
import struct
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    token = "native-viewer-fixture"
    done = threading.Event()
    screenshots: list[bytes] = []
    logs: list[str] = []
    failures: list[str] = []

    class Handler(BaseHTTPRequestHandler):
        def setup(self) -> None:
            super().setup()
            self.connection.settimeout(5)

        def log_message(self, format: str, *args: object) -> None:
            pass

        def reply(self, body: bytes = b"", status: int = 200) -> None:
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self) -> None:
            if self.path == "/":
                page = f"""<!doctype html><canvas id="myCanvas" width="32" height="32"></canvas>
<script>
const canvas = document.getElementById('myCanvas');
const ctx = canvas.getContext('2d');
ctx.fillStyle = 'red'; ctx.fillRect(0, 0, 16, 32);
ctx.fillStyle = 'blue'; ctx.fillRect(16, 0, 16, 32);
// Exercise the shipped canvas PNG fallback, not platform screen capture.
canvas.captureStream = undefined;
window.fastLEDWorkerManager = {{ isWorkerActive: true }};
window.fastLEDEvents = {{ on: (_event, callback) => setTimeout(callback, 0) }};
const valid = window.__fastled_test_token === '{token}' && location.hash === ''
  && typeof window.ipc === 'undefined' && typeof window.__TAURI_INTERNALS__ === 'undefined';
console.log(valid ? 'native-viewer-bootstrap-ok' : 'native-viewer-bootstrap-failed');
async function finish() {{
  if (await (await fetch('/probe-done')).json()) {{
    location.href = 'http://localhost:{self.server.server_port}/rejected';
  }} else {{ setTimeout(finish, 100); }}
}}
finish();
</script>"""
                self.reply(page.encode())
            elif self.path == "/test-config":
                self.reply(json.dumps({"waitMs": 0, "intervalMs": 0, "screenshotNames": ["probe.png"]}).encode())
            elif self.path == "/probe-done":
                self.reply(json.dumps(done.is_set()).encode())
            elif self.path == "/favicon.ico":
                self.reply(status=204)
            else:
                failures.append(f"unexpected GET {self.path}")
                self.reply(status=404)

        def do_POST(self) -> None:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 <= size <= 1024 * 1024:
                failures.append("oversize fixture request")
                self.reply(status=413)
                return
            body = self.rfile.read(size)
            if self.headers.get("Authorization") != f"Bearer {token}":
                failures.append(f"missing authorization: {self.path}")
                self.reply(status=403)
                return
            if self.path == "/viewer-log" and len(logs) < 128:
                logs.append(body.decode())
            elif self.path == "/viewer-screenshot?name=probe.png" and not screenshots:
                screenshots.append(body)
            elif self.path == "/test-done":
                if body != b"0":
                    failures.append(f"capture failed: {body!r}")
                done.set()
            elif self.path != "/test-ready" and not self.path.startswith("/test-sleep?ms="):
                failures.append(f"unexpected POST {self.path}")
            self.reply()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        result = subprocess.run(
            [str(args.binary.resolve()), "--internal-viewer", f"http://127.0.0.1:{server.server_port}/#fastled-test-token={token}", "--viewer-inject-test-runtime"],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    assert not failures, failures
    assert done.is_set(), (logs, result.stderr)
    assert "log: native-viewer-bootstrap-ok" in logs, logs
    assert len(screenshots) == 1, logs
    png = screenshots[0]
    assert png.startswith(b"\x89PNG\r\n\x1a\n") and len(png) > 32
    assert struct.unpack(">II", png[16:24]) == (32, 32)
    assert result.returncode == 1, result
    assert "navigation was rejected" in result.stderr, result.stderr
    print("native viewer bootstrap, authenticated logs, 32x32 PNG capture and rejection teardown passed")


if __name__ == "__main__":
    main()
