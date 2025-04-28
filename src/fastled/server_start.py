import argparse
import importlib.resources as pkg_resources
from dataclasses import dataclass
from multiprocessing import Process
from pathlib import Path

from fastled.server_fastapi_cli import run_fastapi_server_process
from fastled.server_flask import run_flask_server_process


def run_server_process(
    port: int, cwd: Path, certfile: Path | None = None, keyfile: Path | None = None
) -> Process:
    """Run the server in a separate process."""
    if True:
        # Use Flask server
        process = run_flask_server_process(
            port=port,
            cwd=cwd,
            certfile=certfile,
            keyfile=keyfile,
        )
    else:
        # Use FastAPI server
        process = run_fastapi_server_process(
            port=port,
            cwd=cwd,
            certfile=certfile,
            keyfile=keyfile,
        )
    return process


def get_asset_path(filename: str) -> Path | None:
    """Locate a file from the fastled.assets package resources."""
    try:
        resource = pkg_resources.files("fastled.assets").joinpath(filename)
        # Convert to Path for file-system access
        path = Path(str(resource))
        return path if path.exists() else None
    except (ModuleNotFoundError, AttributeError):
        return None


def start_process(
    path: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> Process:
    """Run the server, using package assets if explicit paths are not provided"""
    # Use package resources if no explicit path
    if certfile is None:
        certfile = get_asset_path("localhost.pem")
    if keyfile is None:
        keyfile = get_asset_path("localhost-key.pem")

    # _run_flask_server(path, port, certfile, keyfile)
    # run_fastapi_server_process(port=port, path=path, certfile=certfile, keyfile=keyfile)
    proc = run_server_process(port=port, cwd=path)
    # try:
    #     proc.join()
    # except KeyboardInterrupt:
    #     import _thread

    #     _thread.interrupt_main()
    return proc


@dataclass
class Args:
    fastled_js: Path
    port: int
    cert: Path | None
    key: Path | None


def parse_args() -> Args:
    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
    )
    parser.add_argument(
        "--port",
        "-p",
        type=int,
        default=5500,
        help="Port to run the server on (default: 5500)",
    )
    parser.add_argument(
        "--cert", type=Path, help="(Optional) Path to SSL certificate (PEM format)"
    )
    parser.add_argument(
        "--key", type=Path, help="(Optional) Path to SSL private key (PEM format)"
    )
    args = parser.parse_args()
    out: Args = Args(
        fastled_js=args.fastled_js,
        port=args.port,
        cert=args.cert,
        key=args.key,
    )
    if args.fastled_js is None:
        raise ValueError("fastled_js directory is required")
    return out


def main() -> None:
    args: Args = parse_args()
    fastled_js: Path = args.fastled_js
    port: int = args.port
    cert: Path | None = args.cert
    key: Path | None = args.key
    proc = start_process(
        path=fastled_js,
        port=port,
        certfile=cert,
        keyfile=key,
    )
    try:
        proc.join()
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
        pass


if __name__ == "__main__":
    main()
