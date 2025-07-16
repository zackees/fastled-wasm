import atexit
import random
import sys
import time
import weakref
from multiprocessing import Process
from pathlib import Path

from fastled.playwright.playwright_browser import open_with_playwright
from fastled.server_flask import run_flask_in_thread

# Global reference to keep Playwright browser alive
_playwright_browser_proxy = None


def cleanup_playwright_browser() -> None:
    """Clean up the Playwright browser on exit."""
    try:
        global _playwright_browser_proxy
        if _playwright_browser_proxy:
            _playwright_browser_proxy.close()
            _playwright_browser_proxy = None
    except Exception:
        pass


# Register cleanup function
atexit.register(cleanup_playwright_browser)

DEFAULT_PORT = 8089  # different than live version.
PYTHON_EXE = sys.executable

# Use a weak reference set to track processes without preventing garbage collection
_WEAK_CLEANUP_SET = weakref.WeakSet()


def add_cleanup(proc: Process) -> None:
    """Add a process to the cleanup list using weak references"""
    _WEAK_CLEANUP_SET.add(proc)

    # Register a cleanup function that checks if the process is still alive
    def cleanup_if_alive():
        if proc.is_alive():
            try:
                proc.terminate()
                proc.join(timeout=1.0)
                if proc.is_alive():
                    proc.kill()
            except Exception:
                pass

    atexit.register(cleanup_if_alive)


def is_port_free(port: int) -> bool:
    """Check if a port is free"""
    import httpx

    try:
        response = httpx.get(f"http://localhost:{port}", timeout=1)
        response.raise_for_status()
        return False
    except (httpx.HTTPError, httpx.ConnectError):
        return True


def find_free_port(start_port: int) -> int:
    """Find a free port starting at start_port"""
    for port in range(start_port, start_port + 100, 2):
        if is_port_free(port):
            print(f"Found free port: {port}")
            return port
        else:
            print(f"Port {port} is in use, finding next")
    raise ValueError("Could not find a free port")


def wait_for_server(port: int, timeout: int = 10) -> None:
    """Wait for the server to start."""
    from httpx import get

    future_time = time.time() + timeout
    while future_time > time.time():
        try:
            url = f"http://localhost:{port}"
            # print(f"Waiting for server to start at {url}")
            response = get(url, timeout=1)
            if response.status_code == 200:
                return
        except Exception:
            continue
    raise TimeoutError("Could not connect to server")


def spawn_http_server(
    fastled_js: Path,
    compile_server_port: int,
    port: int | None = None,
    open_browser: bool = True,
    app: bool = False,
) -> Process:

    if port is not None and not is_port_free(port):
        raise ValueError(f"Port {port} was specified but in use")
    if port is None:
        offset = random.randint(0, 100)
        port = find_free_port(DEFAULT_PORT + offset)

    # port: int,
    # cwd: Path,
    # compile_server_port: int,
    # certfile: Path | None = None,
    # keyfile: Path | None = None,

    proc = Process(
        target=run_flask_in_thread,
        args=(port, fastled_js, compile_server_port),
        daemon=True,
    )
    add_cleanup(proc)
    proc.start()

    # Add to cleanup set with weak reference
    add_cleanup(proc)

    wait_for_server(port)
    if open_browser:
        url = f"http://localhost:{port}"
        should_use_playwright = app

        if should_use_playwright:
            if app:
                # For --app mode, try to install browsers if needed
                from fastled.playwright.playwright_browser import (
                    install_playwright_browsers,
                )

                install_playwright_browsers()

            print(f"Opening FastLED sketch in Playwright browser: {url}")
            print(
                "Auto-resize enabled: Browser window will automatically adjust to content size"
            )
            print(
                "🔧 C++ DevTools Support extension will be loaded for DWARF debugging"
            )
            global _playwright_browser_proxy
            _playwright_browser_proxy = open_with_playwright(
                url, enable_extensions=True
            )
        else:
            print(f"Opening browser to {url}")
            import webbrowser

            webbrowser.open(
                url=url,
                new=1,
                autoraise=True,
            )
    return proc


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help=f"Port to run the server on (default: {DEFAULT_PORT})",
    )
    args = parser.parse_args()

    proc = spawn_http_server(args.fastled_js, args.port, open_browser=True)
    proc.join()
