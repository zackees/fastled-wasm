"""FastLED Wasm Compiler package."""

from pathlib import Path

from .types import WebCompileResult

__version__ = "1.1.35"


class Api:
    @staticmethod
    def get_examples():
        from fastled.project_init import get_examples

        return get_examples()

    @staticmethod
    def project_init(example=None, outputdir=None, host: str | None = None) -> Path:
        from fastled.project_init import project_init

        return project_init(example, outputdir, host)

    @staticmethod
    def web_compile(directory, host=None) -> WebCompileResult:
        from fastled.web_compile import web_compile

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
