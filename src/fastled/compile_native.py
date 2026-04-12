"""
Native EMSDK compilation entrypoints.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from fastled.build_service import BuildService
from fastled.build_types import BuildRequest
from fastled.emoji_util import EMO
from fastled.types import BuildMode, CompileResult

if TYPE_CHECKING:
    from fastled.toolchain.emscripten import EmscriptenToolchain


def compile_native(
    directory: Path,
    build_mode: BuildMode = BuildMode.QUICK,
    profile: bool = False,
    fastled_path: Path | str | None = None,
    toolchain: EmscriptenToolchain | None = None,
) -> CompileResult:
    """
    Compile a FastLED sketch using the native build service.

    The optional `toolchain` argument is kept for compatibility with existing callers.
    """
    from fastled.toolchain.emscripten import EmscriptenToolchain

    output_dir = directory / "fastled_js"

    print(f"{EMO('🔨', 'BUILDING:')} Compiling sketch: {directory}")
    print(f"{EMO('📋', 'MODE:')} Build mode: {build_mode.value}")
    print(f"{EMO('📁', 'OUTPUT:')} Output directory: {output_dir}")

    if toolchain is None:
        toolchain = EmscriptenToolchain(fastled_path=fastled_path)

    if not toolchain.check_installation():
        version_info = toolchain.get_version()
        if version_info:
            print(f"Emscripten version: {version_info}")
        else:
            return CompileResult(
                success=False,
                stdout=(
                    "Emscripten SDK not found. Please install EMSDK:\n\n"
                    "  1. Download from: https://emscripten.org/docs/getting_started/downloads.html\n"
                    "  2. Install and activate:\n"
                    "     git clone https://github.com/emscripten-core/emsdk.git\n"
                    "     cd emsdk\n"
                    "     ./emsdk install latest\n"
                    "     ./emsdk activate latest\n"
                    "     source ./emsdk_env.sh  # or emsdk_env.bat on Windows\n"
                ),
                hash_value=None,
                zip_bytes=b"",
                zip_time=0.0,
                libfastled_time=0.0,
                sketch_time=0.0,
                response_processing_time=0.0,
            )

    version = toolchain.get_version()
    if version:
        print(f"{EMO('✨', 'EMSDK:')} {version}")

    service = BuildService()
    result = service.build(
        BuildRequest(
            sketch_dir=directory,
            build_mode=build_mode,
            profile=profile,
            fastled_path=Path(fastled_path) if fastled_path else None,
        )
    )
    return result.compile_result


def run_native_compile(
    directory: Path,
    build_mode: BuildMode = BuildMode.QUICK,
    profile: bool = False,
    open_browser: bool = True,
    keep_running: bool = True,
    enable_https: bool = True,
    fastled_path: Path | str | None = None,
    app: bool = False,
) -> int:
    """
    Run native compilation with optional browser and file watching.

    NOTE: The HTTP server and file watching are now handled by the Rust CLI
    (compile_and_serve in main.rs).  When called via ``--just-compile`` this
    function only performs the compilation step and returns immediately.
    """
    del open_browser, keep_running, enable_https, app  # handled by Rust CLI

    from fastled.toolchain.emscripten import EmscriptenToolchain

    toolchain = EmscriptenToolchain(fastled_path=fastled_path)
    request = BuildRequest(
        sketch_dir=directory,
        build_mode=build_mode,
        profile=profile,
        fastled_path=Path(fastled_path) if fastled_path else None,
    )
    service = BuildService()
    service.register_toolchain(request.fastled_path, toolchain)
    result = service.build(request)

    if not result.success:
        print(f"\n{EMO('❌', 'ERROR:')} Compilation failed:")
        print(result.stdout)
        return 1

    print(f"\n{EMO('✅', 'SUCCESS:')} Compilation successful!")
    print(f"  Time: {result.sketch_time:.2f} seconds")
    print(f"  Strategy: {result.strategy}")
    print(f"  Output: {result.output_dir}")

    return 0
