import argparse
import importlib.resources as pkg_resources
from pathlib import Path

from fastled.server_fastapi_cli import run_fastapi_server_process


def get_asset_path(filename: str) -> Path | None:
    """Locate a file from the fastled.assets package resources."""
    try:
        resource = pkg_resources.files("fastled.assets").joinpath(filename)
        # Convert to Path for file-system access
        path = Path(str(resource))
        return path if path.exists() else None
    except (ModuleNotFoundError, AttributeError):
        return None


def run(
    path: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run the server, using package assets if explicit paths are not provided"""
    # Use package resources if no explicit path
    if certfile is None:
        certfile = get_asset_path("localhost.pem")
    if keyfile is None:
        keyfile = get_asset_path("localhost-key.pem")

    # _run_flask_server(path, port, certfile, keyfile)
    # run_fastapi_server_process(port=port, path=path, certfile=certfile, keyfile=keyfile)
    proc = run_fastapi_server_process(port=port, cwd=path)
    try:
        proc.join()
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()


def parse_args() -> argparse.Namespace:
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
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    fastled_js: Path = args.fastled_js
    port: int = args.port
    cert: Path | None = args.cert
    key: Path | None = args.key
    run(
        path=fastled_js,
        port=port,
        certfile=cert,
        keyfile=key,
    )


if __name__ == "__main__":
    main()
