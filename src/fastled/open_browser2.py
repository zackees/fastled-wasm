import argparse
import socket
import time
from pathlib import Path
from threading import Thread


def _run_flask_server(fastled_js: Path, port: int) -> None:
    """Run Flask server with live reload in a subprocess"""
    try:
        from flask import Flask, send_from_directory

        app = Flask(__name__)

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
            return response

        app.run(port=port, debug=True)
    except Exception as e:
        print(f"Failed to run Flask server: {e}")
        import _thread

        _thread.interrupt_main()


def wait_for_server(port: int, timeout: int = 10) -> None:
    """Wait for the server to start."""
    future_time = time.time() + timeout
    while future_time > time.time():
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("localhost", port)) == 0:
                return
    raise TimeoutError("Could not connect to server")


def wait_for_server_then_launch_browser(port: int) -> None:
    """Wait for the server to start, then launch the browser."""
    wait_for_server(port)
    import webbrowser

    webbrowser.open(f"http://localhost:{port}")


def run(path: Path, port: int, open_browser: bool) -> None:
    """Run the Flask server."""
    if open_browser:
        browser_thread = Thread(
            target=wait_for_server_then_launch_browser, args=(port,), daemon=True
        )
        browser_thread.start()
    _run_flask_server(path, port)


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
        "--no-browser",
        action="store_true",
        help="Do not open the browser",
    )
    return parser.parse_args()


def main() -> None:
    """Main function."""
    args = parse_args()
    open_browser = not args.no_browser
    run(args.fastled_js, args.port, open_browser)


if __name__ == "__main__":
    main()
