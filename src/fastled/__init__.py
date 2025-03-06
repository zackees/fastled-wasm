"""FastLED Wasm Compiler package."""

# context
import shutil
import subprocess
import tempfile
from contextlib import contextmanager
from multiprocessing import Process
from pathlib import Path
from typing import Generator

import httpx

from .compile_server import CompileServer
from .live_client import LiveClient
from .site.build import build
from .types import BuildMode, CompileResult, CompileServerError

# IMPORTANT! There's a bug in github which will REJECT any version update
# that has any other change in the repo. Please bump the version as the
# ONLY change in a commit, or else the pypi update and the release will fail.
__version__ = "1.2.41"

DOCKER_FILE = (
    "https://raw.githubusercontent.com/zackees/fastled-wasm/refs/heads/main/Dockerfile"
)


class Api:
    @staticmethod
    def get_examples(host: str | CompileServer | None = None) -> list[str]:
        from fastled.project_init import get_examples

        if isinstance(host, CompileServer):
            host = host.url()

        return get_examples(host=host)

    @staticmethod
    def project_init(
        example=None, outputdir=None, host: str | CompileServer | None = None
    ) -> Path:
        from fastled.project_init import project_init

        if isinstance(host, CompileServer):
            host = host.url()
        return project_init(example, outputdir, host)

    @staticmethod
    def web_compile(
        directory: Path | str,
        host: str | CompileServer | None = None,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,  # When true then profile information will be enabled and included in the zip.
    ) -> CompileResult:
        from fastled.web_compile import web_compile

        if isinstance(host, CompileServer):
            host = host.url()
        if isinstance(directory, str):
            directory = Path(directory)
        out: CompileResult = web_compile(
            directory, host, build_mode=build_mode, profile=profile
        )
        return out

    @staticmethod
    def live_client(
        sketch_directory: Path,
        host: str | CompileServer | None = None,
        auto_updates: bool = True,
        auto_start=True,
        open_web_browser=True,
        keep_running=True,
        build_mode=BuildMode.QUICK,
        profile=False,
    ) -> LiveClient:
        return LiveClient(
            sketch_directory=sketch_directory,
            host=host,
            auto_updates=auto_updates,
            auto_start=auto_start,
            open_web_browser=open_web_browser,
            keep_running=keep_running,
            build_mode=build_mode,
            profile=profile,
        )

    @staticmethod
    def spawn_server(
        interactive=False,
        auto_updates=None,
        auto_start=True,
        mapped_dir: Path | None = None,  # Sketch directory.
        container_name: str | None = None,  # Specific docker container name.
        remove_previous: bool = False,
    ) -> CompileServer:
        """Uses docker to spawn a compile server from the given name."""
        from fastled.compile_server import CompileServer

        out = CompileServer(
            container_name=container_name,
            interactive=interactive,
            auto_updates=auto_updates,
            mapped_dir=mapped_dir,
            auto_start=auto_start,
            remove_previous=remove_previous,
        )
        return out

    @staticmethod
    @contextmanager
    def server(
        interactive=False,
        auto_updates=None,
        auto_start=True,
        mapped_dir: Path | None = None,  # Sketch directory.
        container_name: str | None = None,  # Specific docker container name.
        remove_previous=False,
    ) -> Generator[CompileServer, None, None]:
        server = Api.spawn_server(
            interactive=interactive,
            auto_updates=auto_updates,
            auto_start=auto_start,
            mapped_dir=mapped_dir,
            container_name=container_name,
            remove_previous=remove_previous,
        )
        try:
            yield server
        finally:
            server.stop()


class Docker:
    @staticmethod
    def is_installed() -> bool:
        from fastled.docker_manager import DockerManager

        return DockerManager.is_docker_installed()

    @staticmethod
    def is_running() -> bool:
        from fastled.docker_manager import DockerManager

        return DockerManager.is_running()

    @staticmethod
    def is_container_running(container_name: str | None = None) -> bool:
        # from fastled.docker import is_container_running
        from fastled.docker_manager import DockerManager
        from fastled.settings import CONTAINER_NAME

        docker_mgr = DockerManager()
        container_name = container_name or CONTAINER_NAME
        return docker_mgr.is_container_running(container_name)

    @staticmethod
    def purge() -> None:
        from fastled.docker_manager import DockerManager
        from fastled.settings import IMAGE_NAME

        docker_mgr = DockerManager()
        docker_mgr.purge(image_name=IMAGE_NAME)

    @staticmethod
    def spawn_server_from_github(
        url: str = "https://github.com/fastled/fastled",
        output_dir: Path | str = Path(".cache/fastled"),
    ) -> CompileServer:
        """Build the FastLED WASM compiler Docker image from a GitHub repository.

        Args:
            url: GitHub repository URL (default: https://github.com/fastled/fastled)
            output_dir: Directory to clone the repo into (default: .cache/fastled)

        Returns:
            Container name.
        """

        from fastled.docker_manager import DockerManager
        from fastled.settings import CONTAINER_NAME, IMAGE_NAME

        if isinstance(output_dir, str):
            output_dir = Path(output_dir)

        # Create output directory if it doesn't exist
        output_dir.mkdir(parents=True, exist_ok=True)

        git_dir = output_dir / ".git"
        library_properties = output_dir / "library.properties"

        git_dir_exists = git_dir.exists()
        library_properties_exists = library_properties.exists()
        library_properties_text = (
            library_properties.read_text().strip() if library_properties_exists else ""
        )

        already_exists = (
            git_dir_exists
            and library_properties_exists
            and "FastLED" in library_properties_text
        )
        if git_dir_exists and not already_exists:
            if ".cache/fastled" in str(output_dir.as_posix()):
                shutil.rmtree(output_dir)
                already_exists = False
            else:
                raise ValueError(
                    f"Output directory {output_dir} already exists but does not appear to be a FastLED repository."
                )

        # Clone or update the repository
        if already_exists:
            print(f"Updating existing repository in {output_dir}")
            # Reset local changes and move HEAD back to handle force pushes
            subprocess.run(
                ["git", "reset", "--hard", "HEAD~10"],
                cwd=output_dir,
                check=False,
                capture_output=True,  # Suppress output of reset
            )
            subprocess.run(
                ["git", "pull", "origin", "master"], cwd=output_dir, check=True
            )
        else:
            print(f"Cloning {url} into {output_dir}")
            subprocess.run(
                ["git", "clone", "--depth", "1", url, str(output_dir)], check=True
            )

        with tempfile.TemporaryDirectory() as tempdir:
            dockerfiles_dst = Path(tempdir) / "Dockerfile"
            # download the file and write it to dockerfiles_dst path
            with open(dockerfiles_dst, "wb") as f:
                with httpx.stream("GET", DOCKER_FILE) as response:
                    for chunk in response.iter_bytes():
                        f.write(chunk)

            if not dockerfiles_dst.exists():
                raise FileNotFoundError(
                    f"Dockerfile not found at {dockerfiles_dst}. "
                    "This may not be a valid FastLED repository."
                )

            docker_mgr = DockerManager()

            platform_tag = ""
            # if "arm" in docker_mgr.architecture():
            if (
                "arm"
                in subprocess.run(["uname", "-m"], capture_output=True).stdout.decode()
            ):
                platform_tag = "-arm64"

            # Build the image
            docker_mgr.build_image(
                image_name=IMAGE_NAME,
                tag="main",
                dockerfile_path=dockerfiles_dst,
                build_context=output_dir,
                build_args={"NO_PREWARM": "1"},
                platform_tag=platform_tag,
            )

        # # Run the container and return it
        # container = docker_mgr.run_container_detached(
        #     image_name=IMAGE_NAME,
        #     tag="main",
        #     container_name=CONTAINER_NAME,
        #     command=None,  # Use default command from Dockerfile
        #     volumes=None,  # No volumes needed for build
        #     ports=None,  # No ports needed for build
        #     remove_previous=True,  # Remove any existing container
        # )
        # name = container.name
        # container.stop()

        out: CompileServer = CompileServer(
            container_name=CONTAINER_NAME,
            interactive=False,
            auto_updates=False,
            mapped_dir=None,
            auto_start=True,
            remove_previous=True,
        )

        return out

    @staticmethod
    def spawn_server_from_fastled_repo(
        project_root: Path | str = Path("."),
        interactive: bool = False,
        sketch_folder: Path | None = None,
    ) -> CompileServer:
        """Build the FastLED WASM compiler Docker image, which will be tagged as "main".

        Args:
            project_root: Path to the FastLED project root directory
            platform_tag: Optional platform tag (e.g. "-arm64" for ARM builds)

        Returns:
            The string name of the docker container.
        """
        from fastled.docker_manager import DockerManager
        from fastled.settings import CONTAINER_NAME, IMAGE_NAME

        project_root = Path(project_root)
        if interactive:
            if sketch_folder is None:
                sketch_folder = project_root / "examples" / "wasm"

        if isinstance(project_root, str):
            project_root = Path(project_root)

        if DockerManager.is_docker_installed() is False:
            raise Exception("Docker is not installed.")

        docker_mgr = DockerManager()
        if DockerManager.is_running() is False:
            docker_mgr.start()

        platform_tag = ""
        # if "arm" in docker_mgr.architecture():
        if (
            "arm"
            in subprocess.run(["uname", "-m"], capture_output=True).stdout.decode()
        ):
            platform_tag = "-arm64"

        # if image exists, remove it
        docker_mgr.purge(image_name=IMAGE_NAME)

        with tempfile.TemporaryDirectory() as tempdir:
            dockerfile_dst = Path(tempdir) / "Dockerfile"

            # download the file and write it to dockerfiles_dst path
            with open(dockerfile_dst, "wb") as f:
                with httpx.stream("GET", DOCKER_FILE) as response:
                    for chunk in response.iter_bytes():
                        f.write(chunk)

            # Build the image
            docker_mgr.build_image(
                image_name=IMAGE_NAME,
                tag="main",
                dockerfile_path=dockerfile_dst,
                build_context=project_root,
                build_args={"NO_PREWARM": "1"},
                platform_tag=platform_tag,
            )

        out: CompileServer = CompileServer(
            container_name=CONTAINER_NAME,
            interactive=interactive,
            auto_updates=False,
            mapped_dir=sketch_folder,
            auto_start=True,
            remove_previous=True,
        )

        return out


class Tools:
    @staticmethod
    def string_diff(needle: str, haystack: list[str]) -> list[tuple[float, str]]:
        """Returns a sorted list with the top matches at the beginning."""
        from fastled.string_diff import string_diff

        return string_diff(needle, haystack)


class Test:
    __test__ = False  # This prevents unittest from recognizing it as a test class.

    @staticmethod
    def can_run_local_docker_tests() -> bool:
        from fastled.test.can_run_local_docker_tests import can_run_local_docker_tests

        return can_run_local_docker_tests()

    @staticmethod
    def test_examples(
        examples: list[str] | None = None, host: str | CompileServer | None = None
    ) -> dict[str, Exception]:
        from fastled.test.examples import test_examples

        if isinstance(host, CompileServer):
            host = host.url()

        return test_examples(examples=examples, host=host)

    @staticmethod
    def build_site(outputdir: Path, fast: bool | None = None, check: bool = True):
        """Builds the FastLED compiler site."""
        build(outputdir=outputdir, fast=fast, check=check)

    @staticmethod
    def spawn_http_server(
        directory: Path | str = Path("."),
        port: int | None = None,
        open_browser: bool = True,
    ) -> Process:
        from fastled.open_browser import open_browser_process

        if isinstance(directory, str):
            directory = Path(directory)
        proc: Process = open_browser_process(
            directory, port=port, open_browser=open_browser
        )
        return proc


__all__ = [
    "Api",
    "Test",
    "Build",
    "CompileServer",
    "CompileResult",
    "CompileServerError",
    "BuildMode",
]
