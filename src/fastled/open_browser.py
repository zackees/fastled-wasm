import os
import socket
import sys
from multiprocessing import Process
from pathlib import Path

from livereload import Server

DEFAULT_PORT = 8081


def _open_browser_python(fastled_js: Path, port: int) -> Server:
    """Start livereload server in the fastled_js directory using API"""
    print(f"\nStarting livereload server in {fastled_js} on port {port}")

    # server = Server()
    # server.watch(str(fastled_js / "index.html"), delay=0.1)
    # server.setHeader("Cache-Control", "no-cache")
    # server.serve(root=str(fastled_js), port=port, open_url_delay=0.5)
    # return server
    os.system(f"cd {fastled_js} && live-server")


def _find_open_port(start_port: int) -> int:
    """Find an open port starting from start_port."""
    port = start_port
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("localhost", port)) != 0:
                return port
            port += 1


def _run_server(fastled_js: Path, port: int) -> None:
    """Function to run in separate process that starts the livereload server"""
    sys.stderr = open(os.devnull, "w")  # Suppress stderr output
    _open_browser_python(fastled_js, port)
    try:
        # Keep the process running
        while True:
            pass
    except KeyboardInterrupt:
        print("\nShutting down livereload server...")


def open_browser_process(fastled_js: Path, port: int | None = None) -> Process:
    """Start livereload server in the fastled_js directory and return the process"""
    if port is None:
        port = DEFAULT_PORT

    port = _find_open_port(port)

    process = Process(
        target=_run_server,
        args=(fastled_js, port),
        daemon=True,
    )
    process.start()
    return process
