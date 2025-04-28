import argparse
import importlib.resources as pkg_resources
from pathlib import Path

from livereload import Server

MAPPING = {
    "js": "application/javascript",
    "css": "text/css",
    "wasm": "application/wasm",
    "json": "application/json",
    "png": "image/png",
    "jpg": "image/jpeg",
    "jpeg": "image/jpeg",
    "gif": "image/gif",
    "svg": "image/svg+xml",
    "ico": "image/x-icon",
    "html": "text/html",
}


def get_asset_path(filename: str) -> Path | None:
    """Locate a file from the fastled.assets package resources."""
    try:
        resource = pkg_resources.files("fastled.assets").joinpath(filename)
        # Convert to Path for file-system access
        path = Path(str(resource))
        return path if path.exists() else None
    except (ModuleNotFoundError, AttributeError):
        return None


def _run_flask_server(
    fastled_js: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run Flask server with live reload or HTTPS depending on args"""
    try:
        from flask import Flask, send_from_directory

        app = Flask(__name__)
        base = fastled_js.resolve()

        @app.route("/")
        def serve_index():
            return send_from_directory(base, "index.html")

        @app.route("/<path:path>")
        def serve_files(path: str):
            response = send_from_directory(base, path)
            ext = path.rsplit(".", 1)[-1].lower()
            if ext in MAPPING:
                response.headers["Content-Type"] = MAPPING[ext]
            # disable caching
            response.headers["Cache-Control"] = "no-cache, no-store, must-revalidate"
            response.headers["Pragma"] = "no-cache"
            response.headers["Expires"] = "0"
            return response

        # HTTPS if both cert and key are available
        if certfile and keyfile:
            app.run(
                host="127.0.0.1",
                port=port,
                ssl_context=(str(certfile), str(keyfile)),
                debug=True,
            )
            return

        # fallback: live-reload server
        server = Server(app.wsgi_app)
        server.watch(str(base / "index.html"))
        server.serve(port=port, debug=True)

    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
    except Exception as e:
        print(f"Failed to run server: {e}")
        import _thread

        _thread.interrupt_main()


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

    _run_flask_server(path, port, certfile, keyfile)


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
    run(args.fastled_js, args.port, args.cert, args.key)


if __name__ == "__main__":
    main()
