"""FastLED Wasm Compiler package."""

from pathlib import Path
from typing import Any

# from .compile_server import CompileServer
from .types import WebCompileResult

__version__ = "1.1.35"


class Api:
    @staticmethod
    def get_examples():
        from fastled.project_init import get_examples

        return get_examples()

    @staticmethod
    def project_init(
        example=None, outputdir=None, host: str | Any | None = None
    ) -> Path:
        from fastled.compile_server import CompileServer
        from fastled.project_init import project_init

        if isinstance(host, CompileServer):
            host = host.url()
        else:
            assert isinstance(host, str) or host is None
        return project_init(example, outputdir, host)

    @staticmethod
    def web_compile(directory, host: str | Any | None = None) -> WebCompileResult:
        from fastled.compile_server import CompileServer
        from fastled.web_compile import web_compile

        if isinstance(host, CompileServer):
            host = host.url()
        else:
            assert isinstance(host, str) or host is None

        out: WebCompileResult = web_compile(directory, host)
        return out

    @staticmethod
    def spawn_server(
        sketch_directory: Path | None = None,
        interactive=False,
        auto_updates=None,
        auto_start=True,
        container_name: str | None = None,
    ):
        from fastled.compile_server import CompileServer

        out = CompileServer(
            container_name=container_name,
            interactive=interactive,
            auto_updates=auto_updates,
            mapped_dir=sketch_directory,
            auto_start=auto_start,
        )
        return out
