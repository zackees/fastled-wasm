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
    try:
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
        # pipe stderr and stdout to null
        subprocess.run(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )  # type ignore
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()


def is_port_free(port: int) -> bool:
    """Check if a port is free"""
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        return s.connect_ex(("localhost", port)) != 0


def find_free_port(start_port: int) -> int:
    """Find a free port starting at start_port"""
    for port in range(start_port, start_port + 100):
        if is_port_free(port):
            print(f"Found free port: {port}")
            return port
        else:
            print(f"Port {port} is in use, finding next")
    raise ValueError("Could not find a free port")


def open_browser_process(
    fastled_js: Path, port: int | None = None, open_browser: bool = True
) -> Process:
    """Start livereload server in the fastled_js directory and return the process"""
    if port is not None:
        if not is_port_free(port):
            raise ValueError(f"Port {port} was specified but in use in use")
    port = port or find_free_port(DEFAULT_PORT)
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
