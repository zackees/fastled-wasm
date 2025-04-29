import argparse
from multiprocessing import Process
from pathlib import Path

from livereload import Server


def _run_flask_server(
    fastled_js: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run Flask server with live reload in a subprocess

    Args:
        fastled_js: Path to the fastled_js directory
        port: Port to run the server on
        certfile: Path to the SSL certificate file
        keyfile: Path to the SSL key file
    """
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
            if path.endswith(".js"):
                response.headers["Content-Type"] = "application/javascript"
            if path.endswith(".css"):
                response.headers["Content-Type"] = "text/css"
            if path.endswith(".wasm"):
                response.headers["Content-Type"] = "application/wasm"
            if path.endswith(".json"):
                response.headers["Content-Type"] = "application/json"
            if path.endswith(".png"):
                response.headers["Content-Type"] = "image/png"
            if path.endswith(".jpg"):
                response.headers["Content-Type"] = "image/jpeg"
            if path.endswith(".jpeg"):
                response.headers["Content-Type"] = "image/jpeg"
            if path.endswith(".gif"):
                response.headers["Content-Type"] = "image/gif"
            if path.endswith(".svg"):
                response.headers["Content-Type"] = "image/svg+xml"
            if path.endswith(".ico"):
                response.headers["Content-Type"] = "image/x-icon"
            if path.endswith(".html"):
                response.headers["Content-Type"] = "text/html"

            # now also add headers to force no caching
            response.headers["Cache-Control"] = "no-cache, no-store, must-revalidate"
            response.headers["Pragma"] = "no-cache"
            response.headers["Expires"] = "0"
            return response

        server = Server(app.wsgi_app)
        # Watch index.html for changes
        server.watch(str(fastled_js / "index.html"))
        # server.watch(str(fastled_js / "index.js"))
        # server.watch(str(fastled_js / "index.css"))
        # Start the server
        server.serve(port=port, debug=True)
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
    except Exception as e:
        print(f"Failed to run Flask server: {e}")
        import _thread

        _thread.interrupt_main()


def run(
    port: int, cwd: Path, certfile: Path | None = None, keyfile: Path | None = None
) -> None:
    """Run the Flask server."""
    try:
        _run_flask_server(cwd, port, certfile, keyfile)
        import warnings

        warnings.warn("Flask server has stopped")
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
        pass


def parse_args() -> argparse.Namespace:
    """Parse the command line arguments."""
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
        required=True,
        help="Port to run the server on (default: %(default)s)",
    )
    parser.add_argument(
        "--certfile",
        type=Path,
        help="Path to the SSL certificate file for HTTPS",
    )
    parser.add_argument(
        "--keyfile",
        type=Path,
        help="Path to the SSL key file for HTTPS",
    )
    return parser.parse_args()


def run_flask_server_process(
    port: int,
    cwd: Path | None = None,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> Process:
    """Run the Flask server in a separate process."""
    cwd = cwd or Path(".")
    process = Process(
        target=run,
        args=(port, cwd, certfile, keyfile),
    )
    process.start()
    return process


def main() -> None:
    """Main function."""
    args = parse_args()
    run(args.port, args.fastled_js, args.certfile, args.keyfile)


if __name__ == "__main__":
    main()
