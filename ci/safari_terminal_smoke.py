"""Real Safari smoke test for the interactive terminal (macOS only, #259).

Serves a compiled sketch with the shipped binary's headless server, then drives
Safari through safaridriver:

1. Four WebSocket clients open PTYs, make the foreground program stop reading
   stdin, send more input than the terminal queue holds and disconnect. All four
   terminal slots must come back well before that program (`sleep 30`) exits;
   this is the #256 regression, observed from Safari's own WebSocket.
2. The page's terminal dialog connects, a typed command runs in the shell, and
   its output reaches xterm's rendered rows.

Playwright WebKit on Linux covers the same engine in
`linux-x86-terminal-test.yml`; only this check proves Safari itself, which
CLAUDE.md makes a required target. A WebDriver screenshot of the terminal is
kept as an artifact.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path

from safari_smoke import start_safaridriver, start_server, webdriver

MARKER_COMMAND = "printf 'SAFARI%s\\n' 240"
MARKER = "SAFARI240"
# The W3C WebDriver key code for Enter.
ENTER = "\ue007"

# WebDriver `execute/async` passes its completion callback as the last argument.
BLOCKED_WRITERS_SCRIPT = """
const done = arguments[arguments.length - 1];
(async () => {
  const url = location.origin.replace('http:', 'ws:') + '/terminal/ws';
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve, reject) => {
      const ws = new WebSocket(url);
      ws.binaryType = 'arraybuffer';
      const timer = setTimeout(() => { ws.close(); reject(new Error('PTY ready timeout')); }, 15000);
      let output = '';
      let blocked = false;
      ws.onerror = () => { clearTimeout(timer); reject(new Error('upgrade rejected')); };
      ws.onopen = () => ws.send(JSON.stringify({type: 'input', data:
        "stty raw -echo; printf 'BLOCK%s\\\\n' READY240; sleep 30\\r"}));
      ws.onmessage = event => {
        if (!(event.data instanceof ArrayBuffer)) return;
        ws.send(JSON.stringify({type: 'ack'}));
        output += new TextDecoder().decode(event.data);
        if (!blocked && output.includes('BLOCKREADY240')) {
          blocked = true;
          ws.send(JSON.stringify({type: 'input', data: 'x'.repeat(60000)}));
          setTimeout(() => { clearTimeout(timer); ws.close(); resolve(true); }, 200);
        }
      };
    });
  }
  const started = performance.now();
  const deadline = started + 5000;
  while (performance.now() < deadline) {
    const sockets = [];
    const opened = await Promise.all([0, 1, 2, 3].map(() => new Promise(resolve => {
      const ws = new WebSocket(url);
      sockets.push(ws);
      ws.onopen = () => resolve(true);
      ws.onerror = () => resolve(false);
    })));
    sockets.forEach(ws => ws.close());
    if (opened.every(Boolean)) return {recoveredMs: performance.now() - started};
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  return {recoveredMs: null};
})().then(done, error => done({error: String(error)}));
"""

TERMINAL_STATE_SCRIPT = """
const status = document.getElementById('terminal-status');
const rows = document.querySelector('.xterm-rows');
return {
  status: status ? status.textContent : null,
  rows: rows ? rows.innerText : null,
  userAgent: navigator.userAgent
};
"""


def find(session: str, selector: str) -> str:
    value = webdriver(
        "POST",
        f"{session}/element",
        {"using": "css selector", "value": selector},
    )["value"]
    return next(iter(value.values()))


def wait_for(session: str, predicate, timeout: float, what: str) -> dict:
    deadline = time.monotonic() + timeout
    state: dict = {}
    while time.monotonic() < deadline:
        state = webdriver(
            "POST",
            f"{session}/execute/sync",
            {"script": TERMINAL_STATE_SCRIPT, "args": []},
        )["value"]
        if predicate(state):
            return state
        time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for {what}: {json.dumps(state)[-2000:]}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fastled", required=True)
    parser.add_argument("--serve-dir", required=True)
    parser.add_argument("--artifacts", required=True, type=Path)
    args = parser.parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True)

    server, page_url = start_server(
        args.fastled, args.serve_dir, args.artifacts / "safari-terminal-server.log"
    )
    driver, base = start_safaridriver()
    session = None
    try:
        created = webdriver(
            "POST",
            f"{base}/session",
            {"capabilities": {"alwaysMatch": {"browserName": "safari"}}},
        )
        session = f"{base}/session/{created['value']['sessionId']}"
        webdriver("POST", f"{session}/timeouts", {"script": 90000})
        webdriver("POST", f"{session}/url", {"url": page_url}, timeout=120)

        # The blocked writers run before the dialog opens its own session, so
        # all four slots are free for them.
        blocked = webdriver(
            "POST",
            f"{session}/execute/async",
            {"script": BLOCKED_WRITERS_SCRIPT, "args": []},
            timeout=120,
        )["value"]
        print(f"Safari terminal: blocked writers -> {blocked}")
        if blocked.get("error"):
            print(f"Safari terminal setup failed: {blocked['error']}", file=sys.stderr)
            return 1
        if blocked.get("recoveredMs") is None:
            print(
                "Safari terminal: disconnected blocked writers leaked terminal slots",
                file=sys.stderr,
            )
            return 1

        # Give the check sockets' sessions a moment to release their slots.
        time.sleep(1)
        webdriver(
            "POST", f"{session}/element/{find(session, '#terminal-open')}/click", {}
        )
        wait_for(
            session,
            lambda state: "Connected" in (state.get("status") or ""),
            30,
            "the terminal to connect",
        )
        # xterm's input textarea is deliberately off-screen, which WebDriver's
        # element APIs can refuse as not interactable. Focus it and type as a user.
        webdriver(
            "POST",
            f"{session}/execute/sync",
            {
                "script": "document.querySelector('.xterm-helper-textarea').focus();",
                "args": [],
            },
        )
        keys = []
        for key in MARKER_COMMAND + ENTER:
            keys += [{"type": "keyDown", "value": key}, {"type": "keyUp", "value": key}]
        webdriver(
            "POST",
            f"{session}/actions",
            {"actions": [{"type": "key", "id": "keyboard", "actions": keys}]},
        )
        webdriver("DELETE", f"{session}/actions")
        state = wait_for(
            session,
            # The echoed command contains `SAFARI%s`; only the output line has
            # the formatted marker.
            lambda state: MARKER in (state.get("rows") or ""),
            30,
            f"{MARKER} in the rendered terminal",
        )
        png = base64.b64decode(webdriver("GET", f"{session}/screenshot")["value"])
        (args.artifacts / "safari-terminal.png").write_bytes(png)
        (args.artifacts / "safari-terminal-state.json").write_text(
            json.dumps({"blocked": blocked, **state}, indent=2)
        )
        print(f"Safari terminal: {state['userAgent']}")
        print(f"Safari terminal: rendered {MARKER}; status={state['status']!r}")
        return 0
    finally:
        if session:
            try:
                png = base64.b64decode(
                    webdriver("GET", f"{session}/screenshot")["value"]
                )
                shot = args.artifacts / "safari-terminal.png"
                if not shot.exists():
                    shot.write_bytes(png)
            except (OSError, RuntimeError, ValueError, KeyError):
                pass
            try:
                webdriver("DELETE", session, timeout=30)
            except (OSError, RuntimeError):
                pass
        driver.terminate()
        server.terminate()


if __name__ == "__main__":
    sys.exit(main())
