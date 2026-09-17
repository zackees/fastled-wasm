"""Refs #240: real xterm -> WebSocket -> native PTY, without compiling WASM.

Run with FASTLED_TERMINAL_BINARY and FASTLED_ESBUILD set to local binaries:
uv run --with playwright --with psutil pytest tests/frontend/test_terminal.py -v
Install matching Playwright browsers separately. CI runs this file through
.github/workflows/_terminal-test.yml; see docs/interactive-terminal.md.
"""

import importlib
import os
import re
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parents[2]
FRONTEND = ROOT / "src/fastled/frontend"


@pytest.fixture(scope="module")
def terminal_server(tmp_path_factory: Any) -> Any:
    binary = os.environ.get("FASTLED_TERMINAL_BINARY")
    esbuild = os.environ.get("FASTLED_ESBUILD")
    if not binary or not esbuild:
        # CI must never report this suite green by skipping it.
        if os.environ.get("CI"):
            pytest.fail("CI requires FASTLED_TERMINAL_BINARY and FASTLED_ESBUILD")
        pytest.skip("set FASTLED_TERMINAL_BINARY and FASTLED_ESBUILD")
    root = tmp_path_factory.mktemp("terminal-240")
    # The served directory carries a space so the PTY cwd is exercised with one.
    # It must stay distinct from `launch`: the terminal must start in the
    # directory named on the command line, not the process's launch directory.
    served = root / "served project"
    served.mkdir()
    launch = root / "launch project"
    launch.mkdir()
    html = (FRONTEND / "index.html").read_text()
    controls = html[html.index('    <button id="terminal-open"') :]
    controls = controls[: controls.index('    <script type="module"')]
    (served / "index.html").write_text(
        '<link rel="stylesheet" href="/index.css"><pre id="output"></pre>'
        + controls
        + '<script type="module" src="/fixture.js"></script>'
    )
    entry = root / "fixture.ts"
    entry.write_text(
        f"import {{ installTerminal }} from '{FRONTEND}/terminal.ts';\n"
        f"import {{ state }} from '{FRONTEND}/state.ts';\n"
        f"import {{ installConsoleOverride, customPrintFunction }} from '{FRONTEND}/logging_setup.ts';\n"
        "state.containerId = 'fixture'; state.outputId = 'output';\n"
        "state.print = customPrintFunction; installConsoleOverride();\n"
        "console.log('SEPARATE_SKETCH_LOG'); installTerminal();\n"
    )
    subprocess.run(
        [
            esbuild,
            str(entry),
            "--bundle",
            "--format=esm",
            "--platform=browser",
            "--target=es2021",
            f"--outfile={served}/fixture.js",
        ],
        check=True,
    )
    subprocess.run(
        [
            esbuild,
            str(FRONTEND / "index.css"),
            "--bundle",
            "--target=es2021",
            "--external:./assets/*",
            f"--outfile={served}/index.css",
        ],
        check=True,
    )
    env = dict(os.environ, FASTLED_MANAGED_RUNTIME="1")
    # Desktop-like stripped parent PATH; the PTY must restore uv tool access.
    if os.name != "nt":
        env["PATH"] = "/usr/bin:/bin"
    process = subprocess.Popen(
        [binary, "--internal-serve-dir-headless", str(served)],
        cwd=launch,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    lines: list[str] = []

    def read_output() -> None:
        assert process.stdout is not None
        while line := process.stdout.readline():
            lines.append(line)

    threading.Thread(target=read_output, daemon=True).start()
    try:
        deadline = time.monotonic() + 20
        url = None
        while time.monotonic() < deadline:
            match = re.search(r"http://127\.0\.0\.1:\d+", "".join(lines))
            if match:
                url = match.group()
                break
            assert process.poll() is None, "".join(lines)
            time.sleep(0.05)
        assert url, "server did not announce its URL: " + "".join(lines)
        yield url, served, process.pid
    finally:
        process.terminate()
        process.wait(timeout=10)
        log_dir = os.environ.get("FASTLED_TERMINAL_LOG_DIR")
        if log_dir:
            Path(log_dir).mkdir(parents=True, exist_ok=True)
            (Path(log_dir) / "terminal-server.log").write_text("".join(lines))


def _require(module: str) -> Any:
    """Import a test-only dependency; CI fails rather than skipping without it."""
    if os.environ.get("CI"):
        return importlib.import_module(module)
    return pytest.importorskip(module)


def _launch(manager: Any, browser_name: str) -> Any:
    """Launch a browser, or attach to a Playwright server for WebKit.

    Playwright cannot install WebKit on some hosts (NixOS). There, run
    `playwright run-server` in the Playwright container with host networking and
    point FASTLED_PLAYWRIGHT_WEBKIT_ENDPOINT at it; see
    docs/interactive-terminal.md.
    """
    endpoint = os.environ.get("FASTLED_PLAYWRIGHT_WEBKIT_ENDPOINT")
    if browser_name == "webkit" and endpoint:
        return manager.webkit.connect(endpoint)
    return getattr(manager, browser_name).launch()


@pytest.mark.skipif(os.name == "nt", reason="drives a POSIX shell (printf, stty, cat)")
@pytest.mark.parametrize("browser_name", ["chromium", "webkit"])
def test_terminal_240_interactive_browser(
    terminal_server: Any, browser_name: str
) -> None:
    playwright = _require("playwright.sync_api")
    url, expected_cwd, _ = terminal_server
    with playwright.sync_playwright() as manager:
        browser = _launch(manager, browser_name)
        page = browser.new_page(viewport={"width": 1200, "height": 900})
        errors: list[str] = []
        output: list[str] = []
        page.on("pageerror", lambda error: errors.append(str(error)))

        def capture_socket(socket: Any) -> None:
            socket.on(
                "framereceived",
                lambda data: output.append(
                    data.decode("utf-8", errors="replace")
                    if isinstance(data, bytes)
                    else data
                ),
            )

        page.on("websocket", capture_socket)
        page.goto(url)
        page.evaluate("""() => {
            window.copied = '';
            Object.defineProperty(navigator, 'clipboard', { value: {
                writeText: async text => { window.copied = text; }
            }});
        }""")
        page.locator("#terminal-open").click()
        playwright.expect(page.locator("#terminal-status")).to_contain_text("Connected")

        def command(text: str) -> None:
            page.locator(".xterm-helper-textarea").focus()
            page.keyboard.insert_text(text)
            page.keyboard.press("Enter")

        def wait_output(text: str) -> None:
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                if text in "".join(output):
                    return
                page.wait_for_timeout(50)
            pytest.fail(
                f"missing {text!r} from terminal output: {''.join(output)[-3000:]}"
            )

        command(
            "printf '\\nCWD=%s\\n' \"$PWD\"; printf '\\033[31mCOLOR_é_OK\\033[0m\\n'"
        )
        wait_output(f"CWD={expected_cwd}\r\n")
        wait_output("\x1b[31mCOLOR_é_OK\x1b[0m")
        playwright.expect(page.locator(".xterm-rows")).to_contain_text("COLOR_é_OK")
        assert "SEPARATE_SKETCH_LOG" in page.locator("#output").inner_text()
        page.locator("#terminal-copy").click()
        copied = page.evaluate("window.copied")
        assert f"CWD={expected_cwd}" in copied
        assert "SEPARATE_SKETCH_LOG" not in copied
        command("export TERMINAL_240_KEEP=retained")
        page.locator("#terminal-close").click()
        assert page.locator("#terminal-open").evaluate(
            "element => element === document.activeElement"
        )
        page.locator("#terminal-open").click()
        command("printf '\\nKEEP=%s\\n' \"$TERMINAL_240_KEEP\"")
        wait_output("\r\nKEEP=retained\r\n")
        page.set_viewport_size({"width": 500, "height": 700})
        page.wait_for_timeout(200)
        command("stty size")
        wait_output("stty size")
        # The fitted PTY must now be narrower than its initial 80 columns.
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            sizes = re.findall(r"(?:\r|\n)(\d+) (\d+)\r?\n", "".join(output))
            if sizes:
                assert 1 <= int(sizes[-1][0]) <= 300
                assert 2 <= int(sizes[-1][1]) < 80
                break
            page.wait_for_timeout(50)
        else:
            pytest.fail(
                "stty did not report the resized PTY dimensions: "
                + "".join(output)[-2000:]
            )
        # A UTF-8 paste larger than one backend frame must be chunked intact.
        command("stty -echo; printf '\\nPASTE_%s\\n' READY; cat > paste-240.txt")
        wait_output("PASTE_READY\r\n")
        payload = ("é😀" * 50 + "\n") * 400
        page.evaluate(
            """text => {
            const clipboardData = new DataTransfer();
            clipboardData.setData('text/plain', text);
            document.querySelector('.xterm-helper-textarea').dispatchEvent(
                new ClipboardEvent('paste', {clipboardData, bubbles: true, cancelable: true}));
        }""",
            payload,
        )
        deadline = time.monotonic() + 10
        paste_file = expected_cwd / "paste-240.txt"
        while time.monotonic() < deadline:
            if paste_file.exists() and paste_file.stat().st_size == len(
                payload.encode()
            ):
                break
            page.wait_for_timeout(50)
        assert paste_file.read_bytes() == payload.encode(), (
            paste_file.stat().st_size,
            page.locator("#terminal-status").inner_text(),
        )
        page.keyboard.press("Control+d")
        command("stty echo; printf '\\nPASTE_BYTES='; wc -c < paste-240.txt")
        wait_output(f"PASTE_BYTES={len(payload.encode())}\r\n")
        # Optional host-specific check, never install or invoke an agent task.
        if os.environ.get("FASTLED_TEST_REAL_CLUD") == "1":
            command(
                "printf '\\nCLUD_PATH='; command -v clud; clud --help; printf '\\nCLUD_HELP_EXIT=%s\\n' \"$?\""
            )
            wait_output("CLUD_PATH=" + str(Path.home() / ".local/bin/clud"))
            wait_output("CLUD_HELP_EXIT=0\r\n")
        command("exit")
        playwright.expect(page.locator("#terminal-restart")).to_be_enabled(
            timeout=10000
        )
        page.locator("#terminal-restart").click()
        playwright.expect(page.locator("#terminal-status")).to_contain_text("Connected")
        command("printf '\\nNEW=%s\\n' \"${TERMINAL_240_KEEP-unset}\"")
        wait_output("NEW=unset\r\n")
        page.keyboard.press("Escape")
        assert page.locator("#terminal-dialog").evaluate("element => element.open")
        assert not errors
        browser.close()


# A foreground program that stops reading its terminal, prints a readiness
# marker, and outlives the test by a wide margin. The marker is split in the
# typed command so the shell's echo of that command cannot satisfy the wait.
if os.name == "nt":
    BLOCKING_COMMAND = "echo BLOCK^READY240 & ping -n 30 127.0.0.1 >NUL\r"
    BLOCKING_PROGRAM = "ping"
else:
    BLOCKING_COMMAND = "stty raw -echo; printf 'BLOCK%s\\n' READY240; sleep 30\r"
    BLOCKING_PROGRAM = "sleep"

BLOCKED_WRITERS_SCRIPT = """async command => {
    const url = location.origin.replace('http:', 'ws:') + '/terminal/ws';
    for (let i = 0; i < 4; i++) {
        await new Promise((resolve, reject) => {
            const ws = new WebSocket(url);
            ws.binaryType = 'arraybuffer';
            const timer = setTimeout(() => { ws.close(); reject(new Error('PTY ready timeout')); }, 15000);
            let output = '';
            let blocked = false;
            ws.onerror = () => { clearTimeout(timer); reject(new Error('upgrade rejected')); };
            ws.onopen = () => ws.send(JSON.stringify({type: 'input', data: command}));
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
    // Every slot must come back promptly, not just one of them, and long
    // before the blocked foreground programs would exit on their own.
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
        if (opened.every(Boolean)) return performance.now() - started;
        await new Promise(resolve => setTimeout(resolve, 100));
    }
    return null;
}"""


def _pty_holders(processes: list[Any]) -> list[int]:
    """Processes holding a Unix98 PTY master; Linux exposes this through /proc."""
    holders = []
    for process in processes:
        try:
            fds = list(Path(f"/proc/{process.pid}/fd").iterdir())
        except OSError:
            continue
        for fd in fds:
            try:
                if os.readlink(fd) == "/dev/ptmx":
                    holders.append(process.pid)
                    break
            except OSError:
                continue
    return holders


def _leaks(server_pid: int) -> list[str]:
    psutil = _require("psutil")
    server = psutil.Process(server_pid)
    descendants = server.children(recursive=True)
    leaks = [
        f"PTY master held by {pid}" for pid in _pty_holders([server, *descendants])
    ]
    for process in descendants:
        try:
            if process.name().lower().removesuffix(".exe") == BLOCKING_PROGRAM:
                leaks.append(f"blocked {BLOCKING_PROGRAM} survived as {process.pid}")
        except psutil.Error:
            continue
    return leaks


@pytest.mark.parametrize("browser_name", ["chromium", "webkit"])
def test_terminal_240_disconnect_with_blocked_stdin(
    terminal_server: Any, browser_name: str
) -> None:
    """RED: four blocked writers leaked every slot; fifth upgrade was HTTP 429."""
    playwright = _require("playwright.sync_api")
    url, _, server_pid = terminal_server
    with playwright.sync_playwright() as manager:
        browser = _launch(manager, browser_name)
        page = browser.new_page()
        page.goto(url)
        result = page.evaluate(BLOCKED_WRITERS_SCRIPT, BLOCKING_COMMAND)
        assert result is not None, "disconnected blocked writers leaked terminal slots"
        browser.close()
    # No session may outlive its client: its PTY and its foreground program
    # must both be gone.
    deadline = time.monotonic() + 10
    leaks = _leaks(server_pid)
    while leaks and time.monotonic() < deadline:
        time.sleep(0.1)
        leaks = _leaks(server_pid)
    assert not leaks, f"sessions outlived their clients: {leaks}"
