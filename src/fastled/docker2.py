"""
New abstraction for Docker management with improved Ctrl+C handling.
"""

import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

import docker
from docker.client import DockerClient
from docker.models.containers import Container
from docker.models.images import Image
from filelock import FileLock


# Docker uses datetimes in UTC but without the timezone info. If we pass in a tz
# then it will throw an exception.
def _utc_now_no_tz() -> datetime:
    now = datetime.now(timezone.utc)
    return now.replace(tzinfo=None)


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


_HERE = Path(__file__).parent
_FILE_LOCK = FileLock(str(_HERE / "fled.lock"))


class RunningContainer:
    def __init__(self, container, first_run=False):
        self.container = container
        self.first_run = first_run
        self.running = True
        self.thread = threading.Thread(target=self._log_monitor)
        self.thread.daemon = True
        self.thread.start()

    def _log_monitor(self):
        from_date = _utc_now_no_tz() if not self.first_run else None
        to_date = _utc_now_no_tz()

        while self.running:
            try:
                for log in self.container.logs(
                    follow=False, since=from_date, until=to_date, stream=True
                ):
                    print(log.decode("utf-8"), end="")
                time.sleep(0.1)
                from_date = to_date
                to_date = _utc_now_no_tz()
            except Exception as e:
                print(f"Error monitoring logs: {e}")
                break

    def stop(self) -> None:
        """Stop monitoring the container logs"""
        self.running = False
        self.thread.join()


class DockerManager2:
    def __init__(self) -> None:
        self.client: DockerClient = docker.from_env()
        self.first_run = False

    def get_lock(self) -> FileLock:
        """Get the file lock for this DockerManager instance."""
        return _FILE_LOCK

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

    @staticmethod
    def is_running() -> bool:
        """Check if Docker is running by pinging the Docker daemon."""
        if not DockerManager2.is_docker_installed():
            return False
        try:
            # self.client.ping()
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
        except KeyboardInterrupt:
            print("Aborted by user.")
            raise
        except Exception as e:
            print(f"Error starting Docker: {str(e)}")
        return False

    def validate_or_download_image(
        self, image_name: str, tag: str = "latest", upgrade: bool = False
    ) -> None:
        """
        Validate if the image exists, and if not, download it.
        If upgrade is True, will pull the latest version even if image exists locally.
        """
        try:
            local_image = self.client.images.get(f"{image_name}:{tag}")
            print(f"Image {image_name}:{tag} is already available.")

            # Quick check for latest version
            try:
                remote_image = self.client.images.get_registry_data(
                    f"{image_name}:{tag}"
                )
                is_latest = local_image.id.startswith(remote_image.id)
                print(f"Local version is{' ' if is_latest else ' not '}up to date")

                if upgrade and not is_latest:
                    print(f"Pulling newer version of {image_name}:{tag}...")
                    _ = self.client.images.pull(image_name, tag=tag)
                    print(f"Updated to newer version of {image_name}:{tag}")
            except Exception as e:
                print(f"Failed to check remote version: {e}")

        except docker.errors.ImageNotFound:
            print(f"Image {image_name}:{tag} not found. Downloading...")
            self.client.images.pull(image_name, tag=tag)
            print(f"Image {image_name}:{tag} downloaded successfully.")

    def tag_image(self, image_name: str, old_tag: str, new_tag: str) -> None:
        """
        Tag an image with a new tag.
        """
        image: Image = self.client.images.get(f"{image_name}:{old_tag}")
        image.tag(image_name, new_tag)
        print(f"Image {image_name}:{old_tag} tagged as {new_tag}.")

    def run_container(
        self,
        image_name: str,
        tag: str,
        container_name: str,
        command: str | None = None,
        volumes: dict[str, dict[str, str]] | None = None,
        ports: dict[int, int] | None = None,
    ) -> Container:
        """
        Run a container from an image. If it already exists, start it.

        Args:
            volumes: Dict mapping host paths to dicts with 'bind' and 'mode' keys
                    Example: {'/host/path': {'bind': '/container/path', 'mode': 'rw'}}
            ports: Dict mapping host ports to container ports
                    Example: {8080: 80} maps host port 8080 to container port 80
        """
        image_name = f"{image_name}:{tag}"
        try:
            container: Container = self.client.containers.get(container_name)
            if container.status == "running":
                print(f"Container {container_name} is already running.")
            elif container.status == "exited":
                print(f"Starting existing container {container_name}.")
                container.start()
            elif container.status == "restarting":
                print(f"Waiting for container {container_name} to restart...")
                timeout = 10
                container.wait(timeout=10)
                if container.status == "running":
                    print(f"Container {container_name} has restarted.")
                else:
                    print(
                        f"Container {container_name} did not restart within {timeout} seconds."
                    )
                    container.stop()
                    print(f"Container {container_name} has been stopped.")
                    container.start()
            elif container.status == "paused":
                print(f"Resuming existing container {container_name}.")
                container.unpause()
            else:
                print(f"Unknown container status: {container.status}")
                print(f"Starting existing container {container_name}.")
                self.first_run = True
                container.start()
        except docker.errors.NotFound:
            print(f"Creating and starting new container {container_name}.")
            container = self.client.containers.run(
                image_name,
                command,
                name=container_name,
                detach=True,
                tty=True,
                volumes=volumes,
                ports=ports,
            )
        return container

    def attach_and_run(self, container: Container | str) -> RunningContainer:
        """
        Attach to a running container and monitor its logs in a background thread.
        Returns a RunningContainer object that can be used to stop monitoring.
        """
        if isinstance(container, str):
            container = self.get_container(container)

        print(f"Attaching to container {container.name}...")

        first_run = self.first_run
        self.first_run = False

        return RunningContainer(container, first_run)

    def suspend_container(self, container: Container | str) -> None:
        """
        Suspend (pause) the container.
        """
        if isinstance(container, str):
            container = self.get_container(container)
        try:
            container.pause()
            print(f"Container {container.name} has been suspended.")
        except Exception as e:
            print(f"Failed to suspend container {container.name}: {e}")

    def resume_container(self, container: Container | str) -> None:
        """
        Resume (unpause) the container.
        """
        if isinstance(container, str):
            container = self.get_container(container)
        try:
            container.unpause()
            print(f"Container {container.name} has been resumed.")
        except Exception as e:
            print(f"Failed to resume container {container.name}: {e}")

    def get_container(self, container_name: str) -> Container:
        """
        Get a container by name.
        """
        try:
            return self.client.containers.get(container_name)
        except docker.errors.NotFound:
            print(f"Container {container_name} not found.")
            raise

    def is_container_running(self, container_name: str) -> bool:
        """
        Check if a container is running.
        """
        try:
            container = self.client.containers.get(container_name)
            return container.status == "running"
        except docker.errors.NotFound:
            print(f"Container {container_name} not found.")
            return False


def main():
    # Register SIGINT handler
    # signal.signal(signal.SIGINT, handle_sigint)

    docker_manager = DockerManager2()

    # Parameters
    image_name = "python"
    tag = "3.10-slim"
    # new_tag = "my-python"
    container_name = "my-python-container"
    command = "python -m http.server"

    try:
        # Step 1: Validate or download the image
        docker_manager.validate_or_download_image(image_name, tag)

        # Step 2: Tag the image
        # docker_manager.tag_image(image_name, tag, new_tag)

        # Step 3: Run the container
        container = docker_manager.run_container(
            image_name, tag, container_name, command
        )

        # Step 4: Attach and monitor the container logs
        running_container = docker_manager.attach_and_run(container)

        # Wait for keyboard interrupt
        while True:
            time.sleep(0.1)

    except KeyboardInterrupt:
        print("\nStopping container...")
        running_container.stop()
        container = docker_manager.get_container(container_name)
        docker_manager.suspend_container(container)

    try:
        # Suspend and resume the container
        container = docker_manager.get_container(container_name)
        docker_manager.suspend_container(container)

        input("Press Enter to resume the container...")

        docker_manager.resume_container(container)
    except Exception as e:
        print(f"An error occurred: {e}")


if __name__ == "__main__":
    main()
