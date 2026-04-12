import atexit
import shutil
import subprocess
import sys
import time
import weakref
from pathlib import Path

from fastled.interrupts import handle_keyboard_interrupt


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


def _find_fastled_cli() -> Path | None:
    """Locate the fastled CLI binary (Rust)."""
    exe_name = "fastled.exe" if sys.platform == "win32" else "fastled"

    # 1. Sibling of the running interpreter.
    exe_path = Path(sys.executable).resolve()
    candidate = exe_path.parent / exe_name
    if candidate.is_file():
        return candidate

    # 2. Walk up to find a Cargo workspace root and check target dirs.
    search_start = Path(__file__).resolve().parent
    current = search_start
    for _ in range(10):
        cargo_toml = current / "Cargo.toml"
        if cargo_toml.is_file():
            for profile in ("debug", "release"):
                candidate = current / "target" / profile / exe_name
                if candidate.is_file():
                    return candidate
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


def wait_for_server(port: int, timeout: int = 15) -> None:
    """Wait for the HTTP server to start."""
    import httpx

    future_time = time.time() + timeout
    hosts = ["localhost", "127.0.0.1"]
    while future_time > time.time():
        for host in hosts:
            try:
                url = f"http://{host}:{port}"
                response = httpx.get(url, timeout=1)
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
    app: bool = False,
    enable_https: bool = True,
    sketch_dir: Path | None = None,
    fastled_path: Path | None = None,
) -> subprocess.Popen:
    """Spawn the Rust CLI HTTP server as a subprocess.

    This replaces the old Flask-based server.  The Rust binary's
    ``--serve-dir`` flag starts a native HTTP server.
    """
    del app, enable_https, sketch_dir, fastled_path  # handled by Rust

    cli = _find_fastled_cli()
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
        # Fallback: wait a bit and hope the server is up
        time.sleep(1.0)
    else:
        wait_for_server(actual_port, timeout=15)

    if open_browser:
        if actual_port:
            url = f"http://localhost:{actual_port}"
        else:
            url = "http://localhost:8089"
        tauri_proc = _launch_tauri_viewer(fastled_js)
        if tauri_proc is not None:
            print("Opening FastLED sketch in Tauri viewer")
        else:
            print(f"Opening browser to {url}")
            import webbrowser

            webbrowser.open(url=url, new=1, autoraise=True)

    return proc
