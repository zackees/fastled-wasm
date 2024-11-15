"""Docker management functionality for FastLED WASM compiler."""

import subprocess
import sys
import time
from pathlib import Path

import docker  # type: ignore

TAG = "main"


def _win32_docker_location() -> str | None:
    home_dir = Path.home()
    out = [
        "C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe",
        f"{home_dir}\\AppData\\Local\\Docker\\Docker Desktop.exe",
    ]
    for loc in out:
        if Path(loc).exists():
            return loc
    return None


class DockerManager:
    """Manages Docker operations for FastLED WASM compiler."""

    def __init__(self, container_name: str):
        self.container_name = container_name

    @staticmethod
    def is_docker_installed() -> bool:
        """Check if Docker is installed on the system."""
        try:
            subprocess.run(["docker", "--version"], capture_output=True, check=True)
            print("Docker is installed.")
            return True
        except subprocess.CalledProcessError as e:
            print(f"Docker command failed: {str(e)}")
            return False
        except FileNotFoundError:
            print("Docker is not installed.")
            return False

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
                docker_path = _win32_docker_location()
                if not docker_path:
                    print("Docker Desktop not found.")
                    return False
                subprocess.run(["start", "", docker_path], shell=True)
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

    def ensure_linux_containers(self) -> bool:
        """Ensure Docker is using Linux containers on Windows."""
        if sys.platform == "win32":
            try:
                # Check if we're already in Linux container mode
                result = subprocess.run(
                    ["docker", "info"], capture_output=True, text=True, check=True
                )
                if "linux" in result.stdout.lower():
                    return True

                print("Switching to Linux containers...")
                subprocess.run(
                    ["cmd", "/c", "docker context ls"], check=True, capture_output=True
                )
                subprocess.run(
                    ["cmd", "/c", "docker context use default"],
                    check=True,
                    capture_output=True,
                )
                return True
            except subprocess.CalledProcessError as e:
                print(f"Failed to switch to Linux containers: {e}")
                print(f"stdout: {e.stdout}")
                print(f"stderr: {e.stderr}")
                return False
        return True  # Non-Windows platforms don't need this

    def ensure_image_exists(self, force_update: bool = False) -> bool:
        """Check if local image exists, pull from remote if not or if update requested."""
        try:
            if not self.ensure_linux_containers():
                return False

            image_name = f"{self.container_name}:{TAG}"
            remote_image = f"niteris/fastled-wasm:{TAG}"

            if force_update:
                print("Forcing image update...")
                # Remove both tagged versions of the image
                subprocess.run(
                    ["docker", "rmi", image_name],
                    check=False,
                )
                subprocess.run(
                    ["docker", "rmi", remote_image],
                    check=False,
                )

            # First check if we have the local tagged image
            result: subprocess.CompletedProcess = subprocess.run(
                ["docker", "image", "inspect", image_name],
                capture_output=True,
                text=True,
                check=False,
            )

            if result.returncode != 0 or force_update:
                print(f"Local image not found. Pulling from {remote_image}...")
                pull_result = subprocess.run(
                    ["docker", "pull", remote_image],
                    text=True,
                    check=False,
                )
                if pull_result.returncode != 0:
                    print("Failed to pull image.")
                    return False

                tag_result = subprocess.run(
                    [
                        "docker",
                        "tag",
                        remote_image,
                        image_name,
                    ],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if tag_result.returncode != 0:
                    print(f"Failed to tag image: {tag_result.stderr}")
                    return False

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

    def full_container_name(self) -> str:
        """Get the name of the container."""
        return f"{self.container_name}:{TAG}"

    def run_container(
        self,
        cmd: list[str],
        volumes: dict[str, str] | None = None,
        ports: dict[int, int] | None = None,
    ) -> subprocess.Popen:
        """Run the Docker container with the specified volume.

        Args:
            volume_path: Path to the volume to mount
            base_name: Base name for the mounted volume
            build_mode: Build mode (DEBUG, QUICK, or RELEASE)
        """
        volumes = volumes or {}
        ports = ports or {}

        print("Creating new container...")
        docker_command = ["docker", "run"]

        if sys.stdout.isatty():
            docker_command.append("-it")
        # Attach volumes if specified
        docker_command += [
            "--name",
            self.container_name,
        ]
        if ports:
            for host_port, container_port in ports.items():
                docker_command.extend(["-p", f"{host_port}:{container_port}"])
        if volumes:
            for host_path, container_path in volumes.items():
                docker_command.extend(["-v", f"{host_path}:{container_path}"])

        docker_command.extend(
            [
                f"{self.container_name}:{TAG}",
            ]
        )
        docker_command.extend(cmd)

        print(f"Running command: {' '.join(docker_command)}")
        process = subprocess.Popen(
            docker_command,
            text=True,
        )

        return process
