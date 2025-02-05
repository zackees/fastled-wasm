import subprocess
import sys
import time
import webbrowser
from multiprocessing import Process
from pathlib import Path

DEFAULT_PORT = 8089  # different than live version.

PYTHON_EXE = sys.executable


def open_http_server_subprocess(
    fastled_js: Path, port: int, open_browser: bool
) -> None:
    """Start livereload server in the fastled_js directory and return the process"""
    import shutil

    try:
        if shutil.which("live-server") is not None:
            cmd = [
                "live-server",
                f"--port={port}",
                "--host=localhost",
                ".",
            ]
            if not open_browser:
                cmd.append("--no-browser")
            subprocess.run(cmd, shell=True, cwd=fastled_js)
            return

        cmd = [
            PYTHON_EXE,
            "-m",
            "fastled.open_browser2",
            str(fastled_js),
            "--port",
            str(port),
        ]
        # return subprocess.Popen(cmd)  # type ignore
        # pipe stderr and stdout to null
        subprocess.run(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )  # type ignore
    except KeyboardInterrupt:
        print("Exiting from server...")
        sys.exit(0)


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
    for port in range(start_port, start_port + 100):
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
            response = get(f"http://localhost:{port}", timeout=1)
            if response.status_code == 200:
                return
        except Exception:
            continue
    raise TimeoutError("Could not connect to server")


def _background_npm_install_live_server() -> None:
    import shutil
    import time

    if shutil.which("npm") is None:
        return

    if shutil.which("live-server") is not None:
        return

    time.sleep(3)
    subprocess.run(
        ["npm", "install", "-g", "live-server"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def open_browser_process(
    fastled_js: Path, port: int | None = None, open_browser: bool = True
) -> Process:
    import shutil

    """Start livereload server in the fastled_js directory and return the process"""
    if port is not None:
        if not is_port_free(port):
            raise ValueError(f"Port {port} was specified but in use")
    else:
        port = find_free_port(DEFAULT_PORT)
    out: Process = Process(
        target=open_http_server_subprocess,
        args=(fastled_js, port, False),
        daemon=True,
    )
    out.start()
    wait_for_server(port)
    if open_browser:
        print(f"Opening browser to http://localhost:{port}")
        webbrowser.open(url=f"http://localhost:{port}", new=1, autoraise=True)

    # start a deamon thread to install live-server
    if shutil.which("live-server") is None:
        import threading

        t = threading.Thread(target=_background_npm_install_live_server)
        t.daemon = True
        t.start()
    return out


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js",
        type=Path,
        help="Path to the fastled_js directory",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help=f"Port to run the server on (default: {DEFAULT_PORT})",
    )
    args = parser.parse_args()

    proc = open_browser_process(args.fastled_js, args.port, open_browser=True)
    proc.join()
