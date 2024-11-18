import socket
import subprocess
from pathlib import Path

DEFAULT_PORT = 8081


def _open_browser_python(fastled_js: Path, port: int) -> subprocess.Popen:
    """Start livereload server in the fastled_js directory using CLI"""
    print(f"\nStarting livereload server in {fastled_js} on port {port}")

    # Construct command for livereload CLI
    cmd = [
        "livereload",
        str(fastled_js),  # directory to serve
        "--port",
        str(port),
        "-t",
        str(fastled_js / "index.html"),  # file to watch
        "-w",
        "0.1",  # delay
        "-o",
        "0.5",  # open browser delay
    ]

    # Start the process
    process = subprocess.Popen(
        cmd,
        # stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return process


def _find_open_port(start_port: int) -> int:
    """Find an open port starting from start_port."""
    port = start_port
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("localhost", port)) != 0:
                return port
            port += 1


def open_browser_thread(fastled_js: Path, port: int | None = None) -> subprocess.Popen:
    """Start livereload server in the fastled_js directory and return the started thread"""
    if port is None:
        port = DEFAULT_PORT

    port = _find_open_port(port)

    return _open_browser_python(fastled_js, port)
