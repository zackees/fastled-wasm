"""FastLED Wasm Compiler package."""

from pathlib import Path
from typing import Any

__version__ = "1.1.35"

class Api:
    @staticmethod
    def get_examples():
        from fastled.project_init import get_examples

        return get_examples()

    @staticmethod
    def project_init(example=None, outputdir=None) -> Path:
        from fastled.project_init import project_init

        return project_init(example, outputdir)

    @staticmethod
    def web_compile(directory, host=None) -> dict[str, Any]:
        from fastled.web_compile import web_compile

        from .web_compile import WebCompileResult

        out: WebCompileResult = web_compile(directory, host)
        return out.to_dict()

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


# __all__ = ["CompileServer", "web_compile", "project_init", "get_examples"]

__all__ = ["web_compile", "project_init"]
