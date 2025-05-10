import subprocess
import sys
import time
from multiprocessing import Process
from pathlib import Path

from fastled.keyz import get_ssl_config

DEFAULT_PORT = 8089  # different than live version.
PYTHON_EXE = sys.executable


# print(f"SSL Config: {SSL_CONFIG.certfile}, {SSL_CONFIG.keyfile}")


def _open_http_server_subprocess(
    fastled_js: Path,
    port: int,
) -> None:
    print("\n################################################################")
    print(f"# Opening browser to {fastled_js} on port {port}")
    print("################################################################\n")
    ssl = get_ssl_config()
    try:
        # Fallback to our Python server
        cmd = [
            PYTHON_EXE,
            "-m",
            "fastled.server_start",
            str(fastled_js),
            "--port",
            str(port),
        ]
        # Pass SSL flags if available
        if ssl:
            raise NotImplementedError("SSL is not implemented yet")
        print(f"Running server on port {port}.")
        print(f"Command: {subprocess.list2cmdline(cmd)}")
        # Suppress output
        subprocess.run(
            cmd,
            stdout=subprocess.DEVNULL,
            # stderr=subprocess.DEVNULL,
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
    for port in range(start_port, start_port + 100, 2):
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
            url = f"http://localhost:{port}"
            # print(f"Waiting for server to start at {url}")
            response = get(url, timeout=1)
            if response.status_code == 200:
                return
        except Exception:
            continue
    raise TimeoutError("Could not connect to server")


def open_browser_process(
    fastled_js: Path,
    port: int | None = None,
    open_browser: bool = True,
) -> Process:

    if port is not None and not is_port_free(port):
        raise ValueError(f"Port {port} was specified but in use")
    if port is None:
        port = find_free_port(DEFAULT_PORT)

    proc = Process(
        target=_open_http_server_subprocess,
        args=(fastled_js, port),
        daemon=True,
    )
    proc.start()
    wait_for_server(port)
    if open_browser:
        print(f"Opening browser to http://localhost:{port}")
        import webbrowser

        webbrowser.open(
            url=f"http://localhost:{port}",
            new=1,
            autoraise=True,
        )
    return proc


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
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
