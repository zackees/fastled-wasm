import asyncio
import socket
import threading
from pathlib import Path

from livereload import Server

DEFAULT_PORT = 8081


def _open_browser_python(fastled_js: Path, port: int) -> None:
    """Start livereload server in the fastled_js directory and open browser"""
    print(f"\nStarting livereload server in {fastled_js} on port {port}")
    # Set up the event loop for this thread
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    server = Server()
    server.watch(str(fastled_js))

    while True:
        try:
            server.serve(
                port=port, root=str(fastled_js), debug=False, open_url_delay=0.5
            )
            break
        except OSError as e:
            print(f"Error starting server: {e}")
            if "Address already in use" in str(e):
                print(f"Port {port} is already in use. Trying next port.")
                port += 1
            else:
                raise


def _find_open_port(start_port: int) -> int:
    """Find an open port starting from start_port."""
    port = start_port
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            if sock.connect_ex(("localhost", port)) != 0:
                return port
            port += 1


def open_browser_thread(fastled_js: Path, port: int | None = None) -> threading.Thread:
    """Start livereload server in the fastled_js directory and return the started thread"""
    if port is None:
        port = DEFAULT_PORT

    port = _find_open_port(port)

    thread = threading.Thread(
        target=_open_browser_python, args=(fastled_js, port), daemon=True
    )
    thread.start()
    return thread
