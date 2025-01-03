import subprocess
import sys
from multiprocessing import Process
from pathlib import Path

DEFAULT_PORT = 8081

PYTHON_EXE = sys.executable


def open_http_server_subprocess(
    fastled_js: Path, port: int, open_browser: bool
) -> None:
    """Start livereload server in the fastled_js directory and return the process"""

    cmd = [
        PYTHON_EXE,
        "-m",
        "fastled.open_browser2",
        str(fastled_js),
        "--port",
        str(port),
    ]
    if not open_browser:
        cmd.append("--no-browser")
    # return subprocess.Popen(cmd)  # type ignore
    subprocess.run(cmd)  # type ignore


def open_browser_process(
    fastled_js: Path, port: int = DEFAULT_PORT, open_browser: bool = True
) -> Process:
    """Start livereload server in the fastled_js directory and return the process"""
    out: Process = Process(
        target=open_http_server_subprocess,
        args=(fastled_js, port, open_browser),
        daemon=True,
    )
    out.start()
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
