"""Docker management functionality for FastLED WASM compiler."""

import subprocess
import sys
import time
from typing import Optional

import docker  # type: ignore


class DockerManager:
    """Manages Docker operations for FastLED WASM compiler."""

    def __init__(self, container_name: str = "fastled-wasm-compiler"):
        self.container_name = container_name

    def is_running(self) -> bool:
        """Check if Docker is running by pinging the Docker daemon."""
        try:
            client = docker.from_env()
            client.ping()
            print("Docker is running.")
            return True
        except docker.errors.DockerException as e:
            print(f"Docker is not running: {str(e)}")
            return False

    def start(self) -> bool:
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
                if self.is_running():
                    print("Docker started successfully.")
                    return True
                time.sleep(3)

            print("Failed to start Docker within the expected time.")
        except Exception as e:
            print(f"Error starting Docker: {str(e)}")
        return False

    def ensure_image_exists(self) -> bool:
        """Check if local image exists, pull from remote if not."""
        try:
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

    def container_exists(self) -> bool:
        """Check if a container with the given name exists."""
        try:
            result = subprocess.run(
                ["docker", "container", "inspect", self.container_name],
                capture_output=True,
                check=False,
            )
            return result.returncode == 0
        except subprocess.CalledProcessError:
            return False

    def remove_container(self) -> bool:
        """Remove a container if it exists."""
        try:
            subprocess.run(
                ["docker", "rm", "-f", self.container_name],
                capture_output=True,
                check=True,
            )
            return True
        except subprocess.CalledProcessError:
            return False

    def run_container(self, volume_path: str, base_name: str) -> Optional[int]:
        """Run the Docker container with the specified volume."""
        try:
            container_exists_flag = self.container_exists()

            if container_exists_flag:
                print("Reusing existing container...")
                docker_command = [
                    "docker",
                    "start",
                    "-a",
                    self.container_name,
                ]
            else:
                print("Creating new container...")
                docker_command = ["docker", "run"]

                if sys.stdout.isatty():
                    docker_command.append("-it")
                docker_command.extend(
                    [
                        "--name",
                        self.container_name,
                        "--platform",
                        "linux/amd64",
                        "-v",
                        f"{volume_path}:/mapped/{base_name}",
                        "fastled-wasm-compiler:latest",
                    ]
                )

            print(f"Running command: {' '.join(docker_command)}")
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
            return process.returncode

        except subprocess.CalledProcessError as e:
            print(f"Failed to run Docker container: {e}")
            return 1
        except KeyboardInterrupt:
            print("\nOperation cancelled by user.")
            return 1
