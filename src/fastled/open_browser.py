import shutil
import socket
import subprocess
import sys
from multiprocessing import Process
from pathlib import Path

DEFAULT_PORT = 8081


def open_http_server(
    fastled_js: Path, port: int | None = None, open_browser=True
) -> subprocess.Popen:
    """Start livereload server in the fastled_js directory using API"""
    cmd_list: list[str]
    print(f"\nStarting livereload server in {fastled_js}")
    if shutil.which("live-server") is None:
        print("live-server not found. Installing it with the embedded npm...")
        cmd_list = [sys.executable, "-m", "npm", "install", "-g", "live-server"]
        subprocess.run(cmd_list)
    cmd_list = ["live-server"]
    if port is not None:
        cmd_list.extend([f"--port={port}"])
    if not open_browser:
        cmd_list.append("--no-browser")
    # subprocess.run(["cd", str(fastled_js), "&&"] + cmd_list)
    return subprocess.Popen(cmd_list, cwd=str(fastled_js), shell=True)


def _open_browser_python(fastled_js: Path) -> None:
    """Start livereload server in the fastled_js directory using API"""
    proc = open_http_server(fastled_js)
    proc.wait()


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

    try:
        # Keep the process running
        _open_browser_python(fastled_js)
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
