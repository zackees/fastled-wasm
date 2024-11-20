"""
New abstraction for Docker management with improved Ctrl+C handling.
"""

import sys
from datetime import datetime, timezone

import docker
from docker.client import DockerClient
from docker.models.containers import Container
from docker.models.images import Image


# Docker uses datetimes in UTC but without the timezone info. If we pass in a tz
# then it will throw an exception.
def _utc_now_no_tz() -> datetime:
    now = datetime.now(timezone.utc)
    return now.replace(tzinfo=None)


class DockerManager:
    def __init__(self):
        self.client: DockerClient = docker.from_env()
        self.first_run = False

    def validate_or_download_image(self, image_name: str, tag: str = "latest") -> None:
        """
        Validate if the image exists, and if not, download it.
        """
        try:
            self.client.images.get(f"{image_name}:{tag}")
            print(f"Image {image_name}:{tag} is already available.")
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
        self, image_name: str, container_name: str, command: str | None = None
    ) -> Container:
        """
        Run a container from an image. If it already exists, start it.
        """
        try:
            container: Container = self.client.containers.get(container_name)
            if container.status == "running":
                print(f"Container {container_name} is already running.")
            if container.status == "paused":
                print(f"Resuming existing container {container_name}.")
                container.unpause()
            else:
                print(f"Starting existing container {container_name}.")
                self.first_run = True
                container.start()
        except docker.errors.NotFound:
            print(f"Creating and starting new container {container_name}.")
            container = self.client.containers.run(
                image_name, command, name=container_name, detach=True, tty=True
            )
        return container

    def attach_and_run(self, container: Container) -> None:
        """
        Attach to a running container and monitor its logs.
        """

        print(f"Attaching to container {container.name}...")

        first_run = self.first_run
        self.first_run = False
        from_date = _utc_now_no_tz() if not first_run else None
        to_date = _utc_now_no_tz()
        try:
            while True:
                for log in container.logs(
                    follow=False, since=from_date, until=to_date, stream=True
                ):
                    print(log.decode("utf-8"), end="")
                import time

                time.sleep(0.1)
                from_date = to_date
                to_date = _utc_now_no_tz()

        except KeyboardInterrupt:
            print("\nDetaching from container logs...")
            raise

    def suspend_container(self, container: Container) -> None:
        """
        Suspend (pause) the container.
        """
        try:
            container.pause()
            print(f"Container {container.name} has been suspended.")
        except Exception as e:
            print(f"Failed to suspend container {container.name}: {e}")

    def resume_container(self, container: Container) -> None:
        """
        Resume (unpause) the container.
        """
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


def handle_sigint(signal, frame):
    """
    Gracefully handle SIGINT (Ctrl+C).
    """
    print("\nInterrupt received. Exiting...")
    sys.exit(0)


def main():
    # Register SIGINT handler
    # signal.signal(signal.SIGINT, handle_sigint)

    docker_manager = DockerManager()

    # Parameters
    image_name = "python"
    tag = "3.10-slim"
    new_tag = "my-python"
    container_name = "my-python-container"
    command = "python -m http.server"

    try:
        # Step 1: Validate or download the image
        docker_manager.validate_or_download_image(image_name, tag)

        # Step 2: Tag the image
        docker_manager.tag_image(image_name, tag, new_tag)

        # Step 3: Run the container
        container = docker_manager.run_container(
            f"{image_name}:{new_tag}", container_name, command
        )

        # Step 4: Attach and monitor the container logs
        docker_manager.attach_and_run(container)
    except KeyboardInterrupt:
        print("\nStopping container...")
        container = docker_manager.get_container(container_name)
        # container.stop()
        # print("Container stopped.")
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
