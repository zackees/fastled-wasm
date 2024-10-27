"""
Uses the latest wasm compiler image to compile the FastLED sketch.

Probably an unfortunate name.

Push instructions:
  1. docker login
  2. ./wasm (builds the image and then runs a container)
    a. This will create an image tagged by fastled-wasm-compiler
  3. docker tag fastled-wasm-compiler:latest niteris/fastled-wasm:latest
  4. docker push niteris/fastled-wasm:latest
"""

import argparse
import os
import subprocess
import sys
import time
import webbrowser
from pathlib import Path

import docker  # type: ignore

from fastled_wasm.config import Config

config: Config = Config()


def is_docker_running():
    """Check if Docker is running by pinging the Docker daemon."""
    try:
        client = docker.from_env()
        client.ping()  # Ping the Docker daemon to verify connectivity
        print("Docker is running.")
        return True
    except docker.errors.DockerException as e:
        print(f"Docker is not running: {str(e)}")
        return False


def start_docker():
    """Attempt to start Docker Desktop (or the Docker daemon) automatically."""
    print("Attempting to start Docker...")
    try:
        if sys.platform == "win32":
            subprocess.run(["start", "Docker Desktop"], shell=True)
        elif sys.platform == "darwin":
            subprocess.run(["open", "/Applications/Docker.app"])
        elif sys.platform.startswith("linux"):
            subprocess.run(["sudo", "systemctl", "start", "docker"])
        else:
            print("Unknown platform. Cannot auto-launch Docker.")
            return False

        # Wait for Docker to start up
        print("Waiting for Docker to start...")
        for _ in range(10):
            if is_docker_running():
                print("Docker started successfully.")
                return True
            time.sleep(3)

        print("Failed to start Docker within the expected time.")
    except Exception as e:
        print(f"Error starting Docker: {str(e)}")
    return False


def ensure_image_exists():
    """Check if local image exists, pull from remote if not."""
    try:
        # Check if local image exists
        result = subprocess.run(
            ["docker", "image", "inspect", "fastled-wasm-compiler:latest"],
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            print("Local image not found. Pulling from niteris/fastled-wasm...")
            subprocess.run(
                ["docker", "pull", "niteris/fastled-wasm:latest"], check=True
            )
            subprocess.run(
                [
                    "docker",
                    "tag",
                    "niteris/fastled-wasm:latest",
                    "fastled-wasm-compiler:latest",
                ],
                check=True,
            )
            print("Successfully pulled and tagged remote image.")
        return True
    except subprocess.CalledProcessError as e:
        print(f"Failed to ensure image exists: {e}")
        return False


def container_exists(container_name):
    """Check if a container with the given name exists."""
    try:
        result = subprocess.run(
            ["docker", "container", "inspect", container_name],
            capture_output=True,
            check=False,
        )
        return result.returncode == 0
    except subprocess.CalledProcessError:
        return False


def remove_container(container_name):
    """Remove a container if it exists."""
    try:
        subprocess.run(
            ["docker", "rm", "-f", container_name],
            capture_output=True,
            check=True,
        )
        return True
    except subprocess.CalledProcessError:
        return False


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
    return False


def main() -> int:
    args = parse_args()
    directory = args.directory
    absolute_directory = os.path.abspath(directory)

    # Update and save the current directory to settings
    config.last_volume_path = absolute_directory
    config.save()
    base_name = os.path.basename(absolute_directory)

    if not check_is_code_directory(Path(absolute_directory)):
        print(f"Directory '{absolute_directory}' does not contain a FastLED sketch.")
        return 1

    if not is_docker_running():
        if start_docker():
            print("Docker is now running.")
        else:
            print("Docker could not be started. Exiting.")
            return 1

    if not os.path.isdir(absolute_directory):
        print(f"ERROR: Directory '{absolute_directory}' does not exist.")
        return 1

    container_name = "fastled-wasm-compiler"

    # Ensure the image exists (pull if needed)
    if not ensure_image_exists():
        print("Failed to ensure Docker image exists. Exiting.")
        return 1

    # Check if we need to recreate the container due to volume path change
    previous_path = config.last_volume_path
    container_exists_flag = container_exists(container_name)

    if container_exists_flag and previous_path != absolute_directory:
        print("Volume path changed, removing existing container...")
        if not remove_container(container_name):
            print("Failed to remove existing container")
            return 1
        container_exists_flag = False

    # Launch or start the Docker container if Docker is running
    try:
        if container_exists_flag and previous_path == absolute_directory:
            print("Reusing existing container...")
            # Start existing container
            docker_command = [
                "docker",
                "start",
                "-a",  # Attach to container's output
                container_name,
            ]
        else:
            print("Creating new container...")
            # Create new container
            docker_command = [
                "docker",
                "run",
            ]

        if not container_exists_flag:
            # Only add these flags for 'docker run'
            if sys.stdout.isatty():
                docker_command.append("-it")
            docker_command.extend(
                [
                    "--name",
                    container_name,
                    "--platform",
                    "linux/amd64",
                    "-v",
                    f"{absolute_directory}:/mapped/{base_name}",
                    "fastled-wasm-compiler:latest",
                ]
            )

        print(f"Running command: {' '.join(docker_command)}")
        process = subprocess.Popen(
            docker_command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
        )

        assert process.stdout

        # Stream the output
        for line in process.stdout:
            print(line, end="")

        # Wait for the process to complete
        process.wait()

        print("\nContainer execution completed.")

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

    except subprocess.CalledProcessError as e:
        print(f"Failed to run Docker container: {e}")
    except KeyboardInterrupt:
        print("\nOperation cancelled by user.")

    return 0


if __name__ == "__main__":
    sys.exit(main())
