"""Docker management functionality for FastLED WASM compiler."""

import subprocess
import sys
import time
from typing import Optional

import docker  # type: ignore

TAG = "main"


class DockerManager:
    """Manages Docker operations for FastLED WASM compiler."""

    def __init__(self, container_name: str):
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

            # Wait for Docker to start up with increasing delays
            print("Waiting for Docker Desktop to start...")
            attempts = 0
            max_attempts = 20  # Increased max wait time
            while attempts < max_attempts:
                attempts += 1
                if self.is_running():
                    print("Docker started successfully.")
                    return True

                # Gradually increase wait time between checks
                wait_time = min(5, 1 + attempts * 0.5)
                print(
                    f"Docker not ready yet, waiting {wait_time:.1f}s... (attempt {attempts}/{max_attempts})"
                )
                time.sleep(wait_time)

            print("Failed to start Docker within the expected time.")
            print(
                "Please try starting Docker Desktop manually and run this command again."
            )
        except Exception as e:
            print(f"Error starting Docker: {str(e)}")
        return False

    def ensure_image_exists(self, force_update: bool = False) -> bool:
        """Check if local image exists, pull from remote if not or if update requested."""
        try:
            if force_update:
                print("Forcing image update...")
                # Remove both tagged versions of the image
                subprocess.run(
                    ["docker", "rmi", f"{self.container_name}:{TAG}"],
                    capture_output=True,
                    check=False,
                )
                subprocess.run(
                    ["docker", "rmi", f"niteris/fastled-wasm:{TAG}"],
                    capture_output=True,
                    check=False,
                )

            result = subprocess.run(
                ["docker", "image", "inspect", f"{self.container_name}:{TAG}"],
                capture_output=True,
                check=False,
            )
            if result.returncode != 0 or force_update:
                print("Local image not found. Pulling from niteris/fastled-wasm...")
                subprocess.run(
                    ["docker", "pull", f"niteris/fastled-wasm:{TAG}"], check=True
                )
                subprocess.run(
                    [
                        "docker",
                        "tag",
                        f"niteris/fastled-wasm:{TAG}",
                        f"{self.container_name}:{TAG}",
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
        """Run the Docker container with the specified volume.

        Args:
            volume_path: Path to the volume to mount
            base_name: Base name for the mounted volume
        """
        try:
            print("Creating new container...")
            docker_command = ["docker", "run"]

            if sys.stdout.isatty():
                docker_command.append("-it")
            docker_command.extend(
                [
                    "--name",
                    self.container_name,
                    "-v",
                    f"{volume_path}:/mapped/{base_name}",
                    f"{self.container_name}:{TAG}",
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
