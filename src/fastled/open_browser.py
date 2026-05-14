"""Compatibility process launcher for the native Rust HTTP server.

The Rust CLI owns server startup and viewer launch. Python keeps this module as
an API-compatible subprocess wrapper for existing callers.
"""

import atexit
import subprocess
import time
import weakref
from pathlib import Path

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
    enable_https: bool = True,
    sketch_dir: Path | None = None,
    fastled_path: Path | None = None,
) -> subprocess.Popen:
    """Spawn the Rust CLI HTTP server as a subprocess.

    The Rust binary's ``--serve-dir`` flag starts a native HTTP server and
    auto-launches the Tauri viewer, so no Python-side viewer spawn is required.
    """
    del open_browser, enable_https, sketch_dir, fastled_path  # handled by Rust

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
        # URL parse above already gates "server is up". Only the unknown-port path
        # (URL line not seen yet) needs a small grace period.
        time.sleep(1.0)

    print("FastLED viewer will be launched by the Rust CLI")

    return proc
