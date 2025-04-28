import argparse
import importlib.resources as pkg_resources
from pathlib import Path

from .server_fastapi_cli import run_fastapi_server_proces


def get_asset_path(filename: str) -> Path | None:
    """Locate a file from the fastled.assets package resources."""
    try:
        resource = pkg_resources.files("fastled.assets").joinpath(filename)
        # Convert to Path for file-system access
        path = Path(str(resource))
        return path if path.exists() else None
    except (ModuleNotFoundError, AttributeError):
        return None


def _open_browser(url: str) -> None:
    # import webview

    # print("\n##################################################")
    # print(f"# Opening browser to {url}")
    # print("##################################################\n")

    # webview.create_window("FastLED", url)
    # webview.start()
    import webbrowser

    webbrowser.open(url, new=1, autoraise=True)
    while True:
        import time

        time.sleep(1)


def run(
    path: Path,
    port: int,
    open_browser: bool,
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
    # run_fastapi_server_proces(port=port, path=path, certfile=certfile, keyfile=keyfile)
    proc = run_fastapi_server_proces(port=port, cwd=path)
    if open_browser:
        _open_browser(f"http://localhost:{port}/")
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
    parser.add_argument(
        "--open-browser",
        action="store_true",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    open_browser: bool = args.open_browser
    fastled_js: Path = args.fastled_js
    port: int = args.port
    cert: Path | None = args.cert
    key: Path | None = args.key
    run(
        path=fastled_js,
        port=port,
        open_browser=open_browser,
        certfile=cert,
        keyfile=key,
    )


if __name__ == "__main__":
    main()
