import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

from fled.build_mode import BuildMode
from fled.config import Config
from fled.docker_manager import DockerManager

CONTAINER_NAME = "fastled-wasm-compiler"
DOCKER = DockerManager(container_name=CONTAINER_NAME)
CONFIG: Config = Config()


@dataclass
class CompiledResult:
    """Dataclass to hold the result of the compilation."""

    success: bool
    fastled_js: str
    hash_value: str | None


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


def compile_local(
    directory: str,
    reuse: bool = False,
    force_update: bool = False,
    build_mode: BuildMode = BuildMode.QUICK,
) -> CompiledResult:
    """Compile the FastLED sketch using Docker. This is now deprecated since
    we use the web compiler instead with a localhost server instead.

    Args:
        directory: Path to the directory containing the FastLED sketch
        reuse: Whether to reuse an existing container
        force_update: Whether to force update even if container exists
        build_mode: Build mode to use:
            - DEBUG: Include debug info, minimal optimization
            - QUICK: Basic optimizations, faster compile time
            - RELEASE: Maximum optimization, slower compile time
    """
    absolute_directory = os.path.abspath(directory)
    volume_changed = CONFIG.last_volume_path != absolute_directory

    # Handle force update first
    if force_update:
        print("Update...")
        if DOCKER.container_exists():
            if not DOCKER.remove_container():
                print("Failed to remove existing container")
                return CompiledResult(success=False, fastled_js="", hash_value=None)
        # Remove the image to force a fresh download
        subprocess.run(["docker", "rmi", "fastled-wasm"], capture_output=True)
        print("All clean")

    # Update and save the current directory to settings
    CONFIG.last_volume_path = absolute_directory
    CONFIG.save()
    base_name = os.path.basename(absolute_directory)

    if not check_is_code_directory(Path(absolute_directory)):
        print(f"Directory '{absolute_directory}' does not contain a FastLED sketch.")
        return CompiledResult(success=False, fastled_js="", hash_value=None)

    if not DOCKER.is_running():
        if DOCKER.start():
            print("Docker is now running.")
        else:
            print("Docker could not be started. Exiting.")
            return CompiledResult(success=False, fastled_js="", hash_value=None)

    if not os.path.isdir(absolute_directory):
        print(f"ERROR: Directory '{absolute_directory}' does not exist.")
        return CompiledResult(success=False, fastled_js="", hash_value=None)

    # Ensure the image exists (pull if needed)
    if not DOCKER.ensure_image_exists():
        print("Failed to ensure Docker image exists..")
        return CompiledResult(success=False, fastled_js="", hash_value=None)

    volumes: dict[str, str] = {absolute_directory: f"/mapped/{base_name}"}

    cmd = ["python", "/js/run.py", "compile"]
    if build_mode == BuildMode.DEBUG:
        cmd.append("--debug")
    elif build_mode == BuildMode.RELEASE:
        cmd.append("--release")
    elif build_mode == BuildMode.QUICK:
        cmd.append("--quick")

    def _run_container() -> int:
        proc = DOCKER.run_container(cmd=cmd, volumes=volumes)
        proc.wait()
        return proc.returncode

    # Handle container reuse logic
    if DOCKER.container_exists():
        if volume_changed or not reuse:
            if not DOCKER.remove_container():
                print("Failed to remove existing container")
                return CompiledResult(success=False, fastled_js="", hash_value=None)

            return_code = _run_container()
        else:
            print("Reusing existing container...")
            docker_command = [
                "docker",
                "start",
                "-a",
                CONTAINER_NAME,
                build_mode.value,
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
        return_code = _run_container()

    if return_code != 0:
        print(f"Container execution failed with code {return_code}.")
        return CompiledResult(
            success=(return_code == 0), fastled_js="", hash_value=None
        )

    fastled_js = os.path.join(absolute_directory, "fastled_js")
    if not os.path.exists(fastled_js):
        print(f"ERROR: Output directory '{fastled_js}' not found.")
        return CompiledResult(success=False, fastled_js="", hash_value=None)
    print(f"Successfully compiled sketch in {fastled_js}")
    return CompiledResult(success=True, fastled_js=fastled_js, hash_value=None)
