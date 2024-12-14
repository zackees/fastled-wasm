"""FastLED Wasm Compiler package."""

# context
from contextlib import contextmanager
from pathlib import Path
from typing import Generator

from .compile_server import CompileServer
from .live_client import LiveClient
from .types import BuildMode, CompileResult, CompileServerError

__version__ = "1.1.61"


class Api:
    @staticmethod
    def get_examples():
        from fastled.project_init import get_examples

        return get_examples()

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
        auto_start=True,
        open_web_browser=True,
        keep_running=True,
        build_mode=BuildMode.QUICK,
        profile=False,
    ) -> LiveClient:
        return LiveClient(
            sketch_directory=sketch_directory,
            host=host,
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
        container_name: str | None = None,
    ) -> CompileServer:
        from fastled.compile_server import CompileServer

        out = CompileServer(
            container_name=container_name,
            interactive=interactive,
            auto_updates=auto_updates,
            mapped_dir=None,
            auto_start=auto_start,
        )
        return out

    @staticmethod
    @contextmanager
    def server(
        interactive=False,
        auto_updates=None,
        auto_start=True,
        container_name: str | None = None,
    ) -> Generator[CompileServer, None, None]:
        server = Api.spawn_server(
            interactive=interactive,
            auto_updates=auto_updates,
            auto_start=auto_start,
            container_name=container_name,
        )
        try:
            yield server
        finally:
            server.stop()


class Test:
    @staticmethod
    def test_examples(
        examples: list[str] | None = None, host: str | CompileServer | None = None
    ) -> dict[str, Exception]:
        from fastled.test.examples import test_examples

        if isinstance(host, CompileServer):
            host = host.url()

        return test_examples(examples=examples, host=host)


__all__ = [
    "Api",
    "Test",
    "CompileServer",
    "CompileResult",
    "CompileServerError",
    "BuildMode",
]
