import os
import subprocess
import webbrowser
from pathlib import Path
from shutil import which

PORT = 8081


def _open_browser_live_server(fastled_js: Path, port: int) -> None:
    """Start live-server in the fastled_js directory"""
    print(f"\nStarting live-server in {fastled_js}")
    print("\nStarting live-server...")
    try:
        subprocess.run(
            ["live-server", f"--port={port}"], check=True, shell=True, cwd=fastled_js
        )
    except subprocess.CalledProcessError as err:
        print(f"Error starting live-server: {err}")


def _open_browser_python(fastled_js: Path, port: int) -> None:
    """Start HTTP server in the fastled_js directory"""
    print(f"\nStarting HTTP server in {fastled_js}")
    print("\nStarting Python's built-in HTTP server...")
    webbrowser.open(f"http://localhost:{port}")
    subprocess.run(
        ["python", "-m", "http.server", f"{port}"],
        check=True,
        shell=True,
        cwd=fastled_js,
    )


def open_browser(fastled_js: Path, port: int | None = None) -> None:
    """Start HTTP server in the fastled_js directory"""
    if not os.path.exists(fastled_js):
        raise FileNotFoundError(f"Output directory {fastled_js} not found")
    port = port if port is not None else PORT
    # Check if live-server is available
    if which("live-server"):
        _open_browser_live_server(fastled_js, port)
    else:
        _open_browser_python(fastled_js, port)
