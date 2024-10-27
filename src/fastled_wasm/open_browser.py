import asyncio
import threading
import webbrowser
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

    # Open browser after a short delay
    def open_browser():
        webbrowser.open(f"http://localhost:{port}")

    # server.application.on_after_start = open_browser
    open_browser()

    try:
        server.serve(port=port, root=str(fastled_js), debug=True)
    except OSError as e:
        print(f"Error starting server: {e}")
        if "Address already in use" in str(e):
            print(f"Port {port} is already in use. Try a different port.")
        raise


def open_browser_thread(fastled_js: Path, port: int | None = None) -> threading.Thread:
    """Start HTTP server in the fastled_js directory and return the started thread"""
    if port is None:
        port = DEFAULT_PORT

    thread = threading.Thread(
        target=_open_browser_python, args=(fastled_js, port), daemon=True
    )
    thread.start()
    return thread
