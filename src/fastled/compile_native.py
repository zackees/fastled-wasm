"""
Native EMSDK compilation entrypoints.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from fastled.build_service import BuildService
from fastled.build_types import BuildRequest
from fastled.emoji_util import EMO
from fastled.interrupts import handle_keyboard_interrupt
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
    """
    from fastled.filewatcher import DebouncedFileWatcherProcess, FileWatcherProcess
    from fastled.keyboard import SpaceBarWatcher
    from fastled.open_browser import spawn_http_server
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

    if not open_browser and not keep_running:
        return 0

    http_proc = None
    if open_browser:
        http_proc = spawn_http_server(
            result.output_dir,
            port=None,
            open_browser=True,
            app=app,
            enable_https=enable_https,
            sketch_dir=directory,
            fastled_path=request.fastled_path,
        )

    if not keep_running:
        return 0

    excluded_patterns = ["fastled_js", ".build"]
    debounced_watcher = DebouncedFileWatcherProcess(
        FileWatcherProcess(directory, excluded_patterns=excluded_patterns),
    )

    print("\nWill compile on sketch changes or if you hit the space bar.")
    print("Press Ctrl+C to stop...")

    try:
        while True:
            if SpaceBarWatcher.watch_space_bar_pressed(timeout=1.0):
                print("\nCompiling...")
                result = service.build(request)
                if result.success:
                    print(
                        f"{EMO('✅', 'SUCCESS:')} Recompilation successful! ({result.strategy})"
                    )
                else:
                    print(f"{EMO('❌', 'ERROR:')} Recompilation failed:")
                    print(result.stdout)
                SpaceBarWatcher.watch_space_bar_pressed()
                continue

            changed_files = debounced_watcher.get_all_changes()
            if changed_files:
                sketch_changes = [
                    f for f in changed_files if "fastled_js" not in Path(f).parts
                ]
                if sketch_changes:
                    print(f"\nChanges detected in {sketch_changes}")
                    print("Compiling...")
                    result = service.build(request)
                    if result.success:
                        print(
                            f"{EMO('✅', 'SUCCESS:')} Recompilation successful! ({result.strategy})"
                        )
                    else:
                        print(f"{EMO('❌', 'ERROR:')} Recompilation failed:")
                        print(result.stdout)

    except KeyboardInterrupt as ki:
        print("\nStopping watch mode...")
        handle_keyboard_interrupt(ki)
    finally:
        debounced_watcher.stop()
        if http_proc:
            http_proc.kill()

    return 0
