"""
Playwright browser test for native WASM compilation.

Uses subprocess isolation to avoid asyncio/threading conflicts on Windows.
"""

import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

FASTLED_JS = Path.home() / "dev" / "fastled7" / "examples" / "Blink" / "fastled_js"

# Inline server script - runs in its own subprocess
SERVER_SCRIPT = r'''
import sys, socket
from pathlib import Path
from flask import Flask, Response, send_from_directory
from flask_cors import CORS
from werkzeug.serving import make_server

app = Flask(__name__)
CORS(app)
D = sys.argv[1]

@app.after_request
def h(r):
    r.headers["Cross-Origin-Embedder-Policy"] = "credentialless"
    r.headers["Cross-Origin-Opener-Policy"] = "same-origin"
    r.headers["Cache-Control"] = "no-cache"
    return r

@app.route("/")
def idx():
    return send_from_directory(D, "index.html")

@app.route("/<path:p>")
def f(p):
    r = send_from_directory(D, p)
    if p.endswith(".js"): r.headers["Content-Type"] = "text/javascript; charset=utf-8"
    elif p.endswith(".wasm"): r.headers["Content-Type"] = "application/wasm"
    elif p.endswith(".css"): r.headers["Content-Type"] = "text/css"
    elif p.endswith(".json"): r.headers["Content-Type"] = "application/json"
    return r

srv = make_server("127.0.0.1", 0, app)
port = srv.server_address[1]
print(f"PORT:{port}", flush=True)
srv.serve_forever()
'''

# Inline Playwright script - runs in its own subprocess
PLAYWRIGHT_SCRIPT = r'''
import json, os, sys, time
from pathlib import Path

port = int(sys.argv[1])
url = f"http://localhost:{port}/"

browsers_path = Path.home() / ".fastled" / "playwright"
if browsers_path.exists():
    os.environ["PLAYWRIGHT_BROWSERS_PATH"] = str(browsers_path)

from playwright.sync_api import sync_playwright

console_msgs = []
errors = []

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, args=["--no-sandbox","--disable-web-security","--allow-insecure-localhost"])
    page = browser.new_page()

    page.on("console", lambda m: console_msgs.append(m.text))
    page.on("pageerror", lambda e: errors.append(str(e)))

    page.goto(url, wait_until="domcontentloaded")

    success_signals = [
        "extern_setup() completed",
        "FastLED initialized",
        "PROXY_TO_PTHREAD setup complete",
        "async platform pump",
        "main() pthread ready",
        "extern_loop()",
    ]

    setup_ok = False
    deadline = time.time() + 45
    while time.time() < deadline:
        for msg in console_msgs:
            if any(sig in msg for sig in success_signals):
                setup_ok = True
                break
        if setup_ok:
            break
        page.wait_for_timeout(500)

    if setup_ok:
        page.wait_for_timeout(2000)

    try: title = page.title()
    except: title = "?"

    try:
        canvas = page.evaluate("() => { const c = document.querySelector('canvas'); return c ? {exists:true,w:c.width,h:c.height} : {exists:false}; }")
    except: canvas = None

    try: sab = page.evaluate("typeof SharedArrayBuffer !== 'undefined'")
    except: sab = None

    browser.close()

print(json.dumps({
    "setup_ok": setup_ok,
    "title": title,
    "canvas": canvas,
    "sab": sab,
    "console_count": len(console_msgs),
    "relevant": [m[:200] for m in console_msgs if any(kw in m.lower() for kw in ["fastled","worker","extern_setup","extern_loop","initialized","animation","error","failed","not defined","import.meta","mime","pthread","platform pump"])],
    "errors": [e[:200] for e in errors],
}))
'''


def main() -> int:
    if not FASTLED_JS.exists():
        print(f"ERROR: {FASTLED_JS} not found. Run native compile first.")
        return 1

    for f in ["index.html", "fastled.js", "fastled.wasm", "fastled_background_worker.js"]:
        if not (FASTLED_JS / f).exists():
            print(f"ERROR: Missing {f}")
            return 1

    print("=" * 60)
    print("  PLAYWRIGHT BROWSER TEST - NATIVE WASM BUILD")
    print("=" * 60)

    # Step 1: Start server subprocess
    print("\n[1/2] Starting HTTP server...")
    server_proc = subprocess.Popen(
        [sys.executable, "-u", "-c", SERVER_SCRIPT, str(FASTLED_JS.resolve())],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    port = None
    deadline = time.time() + 15
    while time.time() < deadline:
        line = server_proc.stdout.readline().strip()  # type: ignore
        if line.startswith("PORT:"):
            port = int(line.split(":")[1])
            break

    if port is None:
        print("ERROR: Server didn't report port")
        server_proc.kill()
        return 1

    # Wait until connectable
    for _ in range(30):
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                break
        except OSError:
            time.sleep(0.3)
    else:
        print("ERROR: Server not connectable")
        server_proc.kill()
        return 1

    print(f"  Server ready on port {port}")

    # Step 2: Run Playwright in separate subprocess
    print("\n[2/2] Running Playwright browser validation...")
    try:
        pw_result = subprocess.run(
            [sys.executable, "-u", "-c", PLAYWRIGHT_SCRIPT, str(port)],
            capture_output=True,
            text=True,
            timeout=90,
        )
        result = json.loads(pw_result.stdout.strip().split("\n")[-1])
    except subprocess.TimeoutExpired:
        print("  Playwright subprocess timed out after 90s")
        result = {"setup_ok": False, "relevant": [], "errors": ["timeout"], "console_count": 0}
    except Exception as e:
        print(f"  Playwright error: {e}")
        if 'pw_result' in dir():
            print(f"  stdout: {pw_result.stdout[:500]}")  # type: ignore
            print(f"  stderr: {pw_result.stderr[:500]}")  # type: ignore
        result = {"setup_ok": False, "relevant": [], "errors": [str(e)], "console_count": 0}
    finally:
        server_proc.kill()
        server_proc.wait()

    # Report
    print(f"\n  Title: {result.get('title', '?')}")
    print(f"  Canvas: {result.get('canvas')}")
    print(f"  SharedArrayBuffer: {result.get('sab')}")

    print("\n" + "=" * 60)
    print("  RESULTS")
    print("=" * 60)

    relevant = result.get("relevant", [])
    print(f"\nConsole messages ({result.get('console_count', 0)} total, {len(relevant)} relevant):")
    for msg in relevant:
        print(f"  {msg}")

    page_errors = result.get("errors", [])
    if page_errors:
        print(f"\nPage errors ({len(page_errors)}):")
        for err in page_errors:
            print(f"  ERROR: {err}")

    setup_ok = result.get("setup_ok", False)
    print()
    if setup_ok and not page_errors:
        print("PASS: FastLED.js initialized (WASM loaded, worker active, no page errors)")
    elif setup_ok:
        print("WARN: FastLED.js initialized but page errors occurred")
    else:
        print("FAIL: FastLED.js did NOT complete initialization")

    return 0 if setup_ok else 1


if __name__ == "__main__":
    sys.exit(main())
