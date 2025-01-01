import shutil
import socket
import subprocess
from multiprocessing import Process
from pathlib import Path

from static_npm import Npm, Npx
from static_npm.paths import CACHE_DIR
from static_npm.running_process import RunningProcess

DEFAULT_PORT = 8081


def open_http_server(
    fastled_js: Path, port: int | None = None, open_browser=True
) -> subprocess.Popen | RunningProcess:
    """Start livereload server in the fastled_js directory using API"""
    cmd_list: list[str]
    print(f"\nStarting livereload server in {fastled_js}")

    cmd_list = ["live-server"]
    if port is not None:
        cmd_list.extend([f"--port={port}"])
    if not open_browser:
        cmd_list.append("--no-browser")

    live_server_where = shutil.which("live-server")
    if live_server_where is None:
        print("live-server not found. Installing it with the embedded npm...")
        # cmd_list = [sys.executable, "-m", "nodejs.npm", "install", "-g", "live-server"]
        # npm.call("install -g live-server --force --legacy-peer-deps")
        tool_dir = CACHE_DIR / "live-server"
        npm = Npm()
        npx = Npx()
        npm.run(["install", "live-server", "--prefix", tool_dir])
        # npm.run(["install", "live-server", "--prefix", str(tool_dir)])
        cmd_str = subprocess.list2cmdline(
            ["npx"] + ["--prefix", str(tool_dir)] + cmd_list
        )
        print(f"Running: {cmd_str}")
        proc = npx.run(["--prefix", str(tool_dir)] + cmd_list, cwd=fastled_js)
        return proc  # type ignore
    else:
        # npx.run("live-server", check=False)
        # subprocess.run(cmd_list)
        live_server_where = shutil.which("live-server")
        print(f"Using live-server from {live_server_where}")
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
