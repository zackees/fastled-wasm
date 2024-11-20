from fastled.docker_manager import DockerManager


class DockerContainer:
    def __init__(
        self, image_name: str, image_tag: str, container_name: str, update: bool
    ) -> None:
        self.image_name = image_name
        self.image_tag = image_tag
        self.container_name = container_name
        self.docker = DockerManager()
