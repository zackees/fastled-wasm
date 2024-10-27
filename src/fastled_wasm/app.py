"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager
from fastled_wasm.open_browser import open_browser

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
        "--no-open",
        action="store_true",
        help="Just compile, skip the step where the browser is opened.",
    )
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="Reuse the existing container if it exists.",
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
    open_browser_after_compile = args.no_open is False
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

    # Handle container reuse logic
    if DOCKER.container_exists():
        if volume_changed or not args.reuse:
            if not DOCKER.remove_container():
                print("Failed to remove existing container")
                return 1
            return_code = DOCKER.run_container(absolute_directory, base_name)
        else:
            print("Reusing existing container...")
            docker_command = [
                "docker",
                "start",
                "-a",
                CONTAINER_NAME,
            ]
            process = subprocess.Popen(
                docker_command,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            assert process.stdout
            for line in process.stdout:
                print(line, end="")
            process.wait()
            return_code = process.returncode
    else:
        return_code = DOCKER.run_container(absolute_directory, base_name)
    if return_code != 0:
        print(f"Container execution failed with code {return_code}.")
        return return_code if return_code is not None else 1

    fastled_js = os.path.join(absolute_directory, "fastled_js")
    if not os.path.exists(fastled_js):
        print(f"ERROR: Output directory '{fastled_js}' not found.")
        return 1
    print(f"Successfully compiled sketch in {fastled_js}")

    if open_browser_after_compile:
        open_browser(Path(fastled_js))
    else:
        print(
            "If you want to open the compiled sketch in a web browser, run this command with --open flag."
        )
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
