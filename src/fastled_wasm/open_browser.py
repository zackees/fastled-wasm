import os
import webbrowser
from pathlib import Path


def open_browser(absolute_directory: Path) -> None:
    # Start HTTP server in the fastled_js directory
    output_dir = os.path.join(absolute_directory, "fastled_js")
    if os.path.exists(output_dir):
        print(f"\nStarting HTTP server in {output_dir}")
        os.chdir(output_dir)

        # Start Python's built-in HTTP server
        print("\nStarting HTTP server...")
        webbrowser.open("http://localhost:8000")
        os.system("python -m http.server")
    else:
        raise FileNotFoundError(f"Output directory {output_dir} not found")
