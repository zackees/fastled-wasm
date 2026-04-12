import atexit
import random
import shutil
import subprocess
import sys
import time
import weakref
from multiprocessing import Process
from pathlib import Path

from fastled.interrupts import handle_keyboard_interrupt
from fastled.server_flask import run_flask_in_thread


def _find_tauri_viewer() -> Path | None:
    """Locate the fastled-viewer (Tauri) binary.

    Search order mirrors crates/fastled-cli/src/viewer.rs:
    1. Same directory as the running Python executable.
    2. target/debug/ and target/release/ relative to the project workspace.
    3. PATH lookup.
    """
    exe_name = "fastled-viewer.exe" if sys.platform == "win32" else "fastled-viewer"

    # 1. Sibling of the running interpreter / entry-point script.
    exe_path = Path(sys.executable).resolve()
    candidate = exe_path.parent / exe_name
    if candidate.is_file():
        return candidate

    # 2. Walk up to find a Cargo workspace root and check target dirs.
    search_start = Path(__file__).resolve().parent  # src/fastled/
    current = search_start
    for _ in range(10):
        cargo_toml = current / "Cargo.toml"
        if cargo_toml.is_file():
            for profile in ("debug", "release"):
                candidate = current / "target" / profile / exe_name
                if candidate.is_file():
                    return candidate
            # Also check platform-specific target dir (e.g. target/x86_64-pc-windows-msvc/...)
            target_dir = current / "target"
            if target_dir.is_dir():
                for arch_dir in target_dir.iterdir():
                    if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                        for profile in ("debug", "release"):
                            candidate = arch_dir / profile / exe_name
                            if candidate.is_file():
                                return candidate
            break
        parent = current.parent
        if parent == current:
            break
        current = parent

    # 3. PATH lookup.
    found = shutil.which(exe_name)
    if found:
        return Path(found)

    return None


def _launch_tauri_viewer(frontend_dir: Path) -> subprocess.Popen | None:
    """Try to launch the Tauri viewer. Returns the process or None on failure."""
    viewer = _find_tauri_viewer()
    if viewer is None:
        return None
    try:
        proc = subprocess.Popen(
            [str(viewer), "--frontend-dir", str(frontend_dir)],
        )
        return proc
    except KeyboardInterrupt as ki:
        handle_keyboard_interrupt(ki)
    except Exception:
        return None


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
            except KeyboardInterrupt as ki:
                handle_keyboard_interrupt(ki)
            except Exception:
                pass

    atexit.register(cleanup_if_alive)


def is_port_free(port: int) -> bool:
    """Check if a port is free"""
    import httpx

    try:
        # Try HTTPS first, then fall back to HTTP
        try:
            response = httpx.get(f"https://localhost:{port}", timeout=1, verify=False)
            response.raise_for_status()
            return False
        except (httpx.HTTPError, httpx.ConnectError):
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


def wait_for_server(port: int, timeout: int = 15, enable_https: bool = True) -> None:
    """Wait for the server to start."""
    import httpx
    from httpx import get

    future_time = time.time() + timeout
    protocol = "https" if enable_https else "http"
    hosts = ["localhost", "127.0.0.1"]
    while future_time > time.time():
        for host in hosts:
            try:
                url = f"{protocol}://{host}:{port}"
                verify = (
                    False if enable_https else True
                )  # Only disable SSL verification for HTTPS
                response = get(url, timeout=1, verify=verify)
                # Any HTTP response means the server is up, even if
                # it returns 500 (e.g., no index.html in served directory)
                if response.status_code < 600:
                    return
            except httpx.HTTPError:
                continue
        time.sleep(0.1)
    raise TimeoutError("Could not connect to server")


def spawn_http_server(
    fastled_js: Path,
    port: int | None = None,
    open_browser: bool = True,
    app: bool = False,  # Deprecated, ignored — Tauri viewer is always tried first
    enable_https: bool = True,
    sketch_dir: Path | None = None,
    fastled_path: Path | None = None,
) -> Process:
    del app  # Unused — kept for backward compatibility
    if port is not None and not is_port_free(port):
        raise ValueError(f"Port {port} was specified but in use")
    if port is None:
        offset = random.randint(0, 100)
        port = find_free_port(DEFAULT_PORT + offset)

    # Get SSL certificate paths from the fastled assets directory if HTTPS is enabled
    certfile: Path | None = None
    keyfile: Path | None = None

    if enable_https:
        import fastled

        assets_dir = Path(fastled.__file__).parent / "assets"
        certfile = assets_dir / "localhost.pem"
        keyfile = assets_dir / "localhost-key.pem"

    proc = Process(
        target=run_flask_in_thread,
        args=(
            port,
            fastled_js,
            certfile,
            keyfile,
            sketch_dir,
            fastled_path,
            None,
        ),
        daemon=True,
    )
    add_cleanup(proc)
    proc.start()

    wait_for_server(port, enable_https=enable_https)
    if open_browser:
        protocol = "https" if enable_https else "http"
        url = f"{protocol}://localhost:{port}"

        # Try Tauri viewer first (native webview), fall back to system browser.
        tauri_proc = _launch_tauri_viewer(fastled_js)
        if tauri_proc is not None:
            print("Opening FastLED sketch in Tauri viewer")
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
