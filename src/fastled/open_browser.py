import atexit
import subprocess
import time
import weakref
from pathlib import Path

from fastled._native import find_fastled_viewer
from fastled._rust_cli import find_rust_fastled_cli
from fastled.interrupts import handle_keyboard_interrupt

# Use a weak reference set to track processes without preventing garbage collection
_WEAK_CLEANUP_SET: weakref.WeakSet = weakref.WeakSet()


def add_cleanup(proc: subprocess.Popen) -> None:
    """Add a process to the cleanup list using weak references"""
    _WEAK_CLEANUP_SET.add(proc)

    def cleanup_if_alive():
        if proc.poll() is None:
            try:
                proc.terminate()
                proc.wait(timeout=1.0)
                if proc.poll() is None:
                    proc.kill()
            except KeyboardInterrupt as ki:
                handle_keyboard_interrupt(ki)
            except Exception:
                pass

    atexit.register(cleanup_if_alive)


def spawn_http_server(
    fastled_js: Path,
    port: int | None = None,
    open_browser: bool = True,
    app: bool = False,
    enable_https: bool = True,
    sketch_dir: Path | None = None,
    fastled_path: Path | None = None,
) -> subprocess.Popen:
    """Spawn the Rust CLI HTTP server as a subprocess.

    The Rust binary's ``--serve-dir`` flag starts a native HTTP server and
    auto-launches the Tauri viewer when the ``fastled-viewer`` binary is
    available, so no Python-side viewer spawn is required.
    """
    del app, enable_https, sketch_dir, fastled_path  # handled by Rust

    cli = find_rust_fastled_cli()
    if cli is None:
        raise RuntimeError("Could not find the fastled CLI binary")

    cmd = [str(cli), "--serve-dir", str(fastled_js)]

    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    add_cleanup(proc)

    # Parse the port from server output (e.g. "Serving <dir> at http://127.0.0.1:12345")
    import re

    actual_port = port
    if proc.stdout:
        for _ in range(50):  # read up to 50 lines looking for the URL
            line = proc.stdout.readline().decode("utf-8", errors="replace")
            if not line:
                break
            m = re.search(r"http://[\d.]+:(\d+)", line)
            if m:
                actual_port = int(m.group(1))
                break

    if actual_port is None:
        # The Rust CLI prints its listening URL once the socket is bound, so the
        # URL parse above already gates "server is up". Only the fallback path
        # (URL line not seen yet) needs a small grace period.
        time.sleep(1.0)

    if open_browser:
        if actual_port:
            url = f"http://localhost:{actual_port}"
        else:
            url = "http://localhost:8089"

        # The Rust CLI auto-launches the viewer; only fall back to the system
        # browser when no viewer binary is present.
        if find_fastled_viewer() is None:
            print(f"Opening browser to {url}")
            import webbrowser

            webbrowser.open(url=url, new=1, autoraise=True)
        else:
            print("FastLED viewer will be launched by the Rust CLI")

    return proc
