import argparse
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

        # Must be a full path or flask will fail to find the file.
        fastled_js = fastled_js.resolve()

        @app.route("/")
        def serve_index():
            return send_from_directory(fastled_js, "index.html")

        @app.route("/<path:path>")
        def serve_files(path):
            response = send_from_directory(fastled_js, path)
            # Some servers don't set the Content-Type header for a bunch of files.
            pathending = path.split(".")[-1]

            mapped_value = MAPPING.get(pathending)
            if mapped_value:
                response.headers["Content-Type"] = mapped_value

            # now also add headers to force no caching
            response.headers["Cache-Control"] = "no-cache, no-store, must-revalidate"
            response.headers["Pragma"] = "no-cache"
            response.headers["Expires"] = "0"
            return response

        # If SSL cert and key provided, use Flask's built-in HTTPS support
        if certfile and keyfile:
            app.run(
                host="127.0.0.1",
                port=port,
                ssl_context=(str(certfile), str(keyfile)),
                debug=True,
            )
            return

        # Otherwise, fallback to livereload
        server = Server(app.wsgi_app)
        server.watch(str(fastled_js / "index.html"))
        server.serve(port=port, debug=True)
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
    except Exception as e:
        print(f"Failed to run server: {e}")
        import _thread

        _thread.interrupt_main()


def run(
    path: Path, port: int, certfile: Path | None = None, keyfile: Path | None = None
) -> None:
    """Run the server, optionally over HTTPS"""
    _run_flask_server(path, port, certfile, keyfile)


def parse_args() -> argparse.Namespace:
    """Parse command line args"""
    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
    )
    parser.add_argument(
        "--port", "-p", type=int, default=5500, help="Port to run the server on"
    )
    parser.add_argument(
        "--cert", type=Path, help="Path to SSL certificate (PEM format)"
    )
    parser.add_argument("--key", type=Path, help="Path to SSL private key (PEM format)")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    run(args.fastled_js, args.port, args.cert, args.key)


if __name__ == "__main__":
    main()
