"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import sys
import webbrowser
from pathlib import Path

from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager

CONTAINER_NAME = "fastled-wasm-compiler"
DOCKER = DockerManager(container_name=CONTAINER_NAME)
CONFIG: Config = Config()


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description="FastLED WASM Compiler")
    parser.add_argument(
        "directory",
        type=str,
        nargs="?",
        default=os.getcwd(),
        help="Directory containing the FastLED sketch to compile",
    )
    parser.add_argument(
        "--open",
        action="store_true",
        help="Open the compiled sketch in a web browser",
    )
    return parser.parse_args()


def check_is_code_directory(directory: Path) -> bool:
    """Check if the current directory is a code directory."""
    platformio_exists = (directory / "platformio.ini").exists()
    if platformio_exists:
        return True
    src_dir = directory / "src"
    if src_dir.exists():
        return check_is_code_directory(src_dir)
    ino_file = list(directory.glob("*.ino"))
    if ino_file:
        return True
    cpp_files = list(directory.glob("*.cpp"))
    if cpp_files:
        return True
    return False


def main() -> int:
    args = parse_args()
    open_browser = args.open
    directory = args.directory
    absolute_directory = os.path.abspath(directory)

    volume_changed = CONFIG.last_volume_path != absolute_directory

    # Update and save the current directory to settings
    CONFIG.last_volume_path = absolute_directory
    CONFIG.save()
    base_name = os.path.basename(absolute_directory)

    if not check_is_code_directory(Path(absolute_directory)):
        print(f"Directory '{absolute_directory}' does not contain a FastLED sketch.")
        return 1

    if not DOCKER.is_running():
        if DOCKER.start():
            print("Docker is now running.")
        else:
            print("Docker could not be started. Exiting.")
            return 1

    if not os.path.isdir(absolute_directory):
        print(f"ERROR: Directory '{absolute_directory}' does not exist.")
        return 1

    # Ensure the image exists (pull if needed)
    if not DOCKER.ensure_image_exists():
        print("Failed to ensure Docker image exists. Exiting.")
        return 1

    # Check if we need to recreate the container due to volume path change
    if DOCKER.container_exists() and volume_changed:
        print("Volume path changed, removing existing container...")
        if not DOCKER.remove_container():
            print("Failed to remove existing container")
            return 1

    # Run the container
    return_code = DOCKER.run_container(absolute_directory, base_name)
    if return_code != 0:
        print(f"Container execution failed with code {return_code}.")
        return return_code if return_code is not None else 1

    if open_browser:
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
            print(f"\nOutput directory {output_dir} not found")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
