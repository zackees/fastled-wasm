"""FastLED Wasm Compiler package."""

from contextlib import contextmanager
from multiprocessing import Process
from pathlib import Path
from typing import Generator

from .compile_server import CompileServer
from .live_client import LiveClient
from .site.build import build
from .types import BuildMode, CompileResult, CompileServerError

# IMPORTANT! There's a bug in github which will REJECT any version update
# that has any other change in the repo. Please bump the version as the
# ONLY change in a commit, or else the pypi update and the release will fail.
__version__ = "1.2.56"

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
    "CompileServer",
    "CompileResult",
    "CompileServerError",
    "BuildMode",
]
