import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager

CONTAINER_NAME = "fastled-wasm-compiler"
DOCKER = DockerManager(container_name=CONTAINER_NAME)
CONFIG: Config = Config()


@dataclass
class CompiledResult:
    """Dataclass to hold the result of the compilation."""

    return_code: int
    fastled_js: str


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


def compile(directory: str, reuse: bool = False) -> CompiledResult:
    """Compile the FastLED sketch using Docker."""
    absolute_directory = os.path.abspath(directory)
    volume_changed = CONFIG.last_volume_path != absolute_directory

    # Update and save the current directory to settings
    CONFIG.last_volume_path = absolute_directory
    CONFIG.save()
    base_name = os.path.basename(absolute_directory)

    if not check_is_code_directory(Path(absolute_directory)):
        print(f"Directory '{absolute_directory}' does not contain a FastLED sketch.")
        return CompiledResult(return_code=1, fastled_js="")

    if not DOCKER.is_running():
        if DOCKER.start():
            print("Docker is now running.")
        else:
            print("Docker could not be started. Exiting.")
            return CompiledResult(return_code=1, fastled_js="")

    if not os.path.isdir(absolute_directory):
        print(f"ERROR: Directory '{absolute_directory}' does not exist.")
        return CompiledResult(return_code=1, fastled_js="")

    # Ensure the image exists (pull if needed)
    if not DOCKER.ensure_image_exists():
        print("Failed to ensure Docker image exists. Exiting.")
        return CompiledResult(return_code=1, fastled_js="")

    # Handle container reuse logic
    if DOCKER.container_exists():
        if volume_changed or not reuse:
            if not DOCKER.remove_container():
                print("Failed to remove existing container")
                return CompiledResult(return_code=1, fastled_js="")
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
        return CompiledResult(
            return_code=return_code if return_code is not None else 1, fastled_js=""
        )

    fastled_js = os.path.join(absolute_directory, "fastled_js")
    if not os.path.exists(fastled_js):
        print(f"ERROR: Output directory '{fastled_js}' not found.")
        return CompiledResult(return_code=1, fastled_js="")
    print(f"Successfully compiled sketch in {fastled_js}")
    return CompiledResult(return_code=0, fastled_js=fastled_js)
