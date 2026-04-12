"""FastLED Wasm Compiler package."""

from multiprocessing import Process
from pathlib import Path

from .__version__ import __version__

try:
    from fastled._native import version as _native_version
except ImportError:
    _native_version = None
from .build_service import BuildService
from .build_types import BuildRequest, BuildResult
from .site.build import build
from .types import BuildMode, CompileResult


class Api:
    @staticmethod
    def get_examples() -> list[str]:
        from fastled.project_init import get_examples

        return get_examples()

    @staticmethod
    def project_init(
        example=None,
        outputdir=None,
    ) -> Path:
        from fastled.project_init import project_init

        return project_init(example, outputdir)


class Tools:
    @staticmethod
    def string_diff(needle: str, haystack: list[str]) -> list[tuple[float, str]]:
        """Returns a sorted list with the top matches at the beginning."""
        from fastled.string_diff import string_diff

        return string_diff(needle, haystack)


class Test:
    __test__ = False  # This prevents unittest from recognizing it as a test class.

    @staticmethod
    def build_site(outputdir: Path, fast: bool | None = None, check: bool = True):
        """Builds the FastLED compiler site."""
        build(outputdir=outputdir, fast=fast, check=check)

    @staticmethod
    def spawn_http_server(
        directory: Path | str = Path("."),
        port: int | None = None,
        open_browser: bool = True,
        app: bool = False,
        enable_https: bool = False,  # Default to HTTP for tests (tests use http:// URLs)
    ) -> Process:
        from fastled.open_browser import spawn_http_server

        if isinstance(directory, str):
            directory = Path(directory)
        proc: Process = spawn_http_server(
            directory,
            port=port,
            open_browser=open_browser,
            app=app,
            enable_https=enable_https,
        )
        return proc


__all__ = [
    "Api",
    "BuildRequest",
    "BuildResult",
    "BuildService",
    "Test",
    "CompileResult",
    "BuildMode",
    "__version__",
]
