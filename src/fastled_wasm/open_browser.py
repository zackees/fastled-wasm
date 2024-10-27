import os
import webbrowser
from pathlib import Path


def open_browser(fastled_js: Path) -> None:
    # Start HTTP server in the fastled_js directory
    if os.path.exists(fastled_js):
        print(f"\nStarting HTTP server in {fastled_js}")
        os.chdir(fastled_js)

        # Start Python's built-in HTTP server
        print("\nStarting HTTP server...")
        webbrowser.open("http://localhost:8000")
        os.system("python -m http.server")
    else:
        raise FileNotFoundError(f"Output directory {fastled_js} not found")
