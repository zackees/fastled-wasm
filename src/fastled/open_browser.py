import os
import shutil
import socket
import subprocess
import sys
from multiprocessing import Process
from pathlib import Path

DEFAULT_PORT = 8081


def _open_browser_python(fastled_js: Path) -> None:
    """Start livereload server in the fastled_js directory using API"""
    print(f"\nStarting livereload server in {fastled_js}")
    if shutil.which("live-server") is None:
        print("live-server not found. Installing it with the embedded npm...")
        subprocess.run(
            [sys.executable, "-m", "nodejs.npm", "install", "-g", "live-server"]
        )
    os.system(f"cd {fastled_js} && live-server")


def _find_open_port(start_port: int) -> int:
    """Find an open port starting from start_port."""
    port = start_port
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("localhost", port)) != 0:
                return port
            port += 1


def _run_server(fastled_js: Path) -> None:
    """Function to run in separate process that starts the livereload server"""
    sys.stderr = open(os.devnull, "w")  # Suppress stderr output
    _open_browser_python(fastled_js)
    try:
        # Keep the process running
        while True:
            pass
    except KeyboardInterrupt:
        print("\nShutting down livereload server...")


def open_browser_process(fastled_js: Path) -> Process:
    """Start livereload server in the fastled_js directory and return the process"""

    process = Process(
        target=_run_server,
        args=(fastled_js,),
        daemon=True,
    )
    process.start()
    return process
