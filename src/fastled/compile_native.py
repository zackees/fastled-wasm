"""
Native EMSDK Compilation Integration Module

This module provides native compilation functionality using locally installed EMSDK
instead of Docker containers. It uses the EmscriptenToolchain from the toolchain module.
"""

import shutil
import tempfile
import time
from pathlib import Path

from fastled.emoji_util import EMO
from fastled.types import BuildMode, CompileResult


def compile_native(
    directory: Path,
    build_mode: BuildMode = BuildMode.QUICK,
    profile: bool = False,
    fastled_path: Path | str | None = None,
) -> CompileResult:
    """
    Compile a FastLED sketch using native EMSDK toolchain.

    Args:
        directory: Path to the sketch directory
        build_mode: Build mode (DEBUG, QUICK, RELEASE)
        profile: Enable profiling output
        fastled_path: Path to FastLED library. If None, downloads from master repo.

    Returns:
        CompileResult with compilation status and output
    """
    from fastled.toolchain.emscripten import EmscriptenToolchain

    output_dir = directory / "fastled_js"

    print(f"{EMO('🔨', 'BUILDING:')} Compiling sketch: {directory}")
    print(f"{EMO('📋', 'MODE:')} Build mode: {build_mode.value}")
    print(f"{EMO('📁', 'OUTPUT:')} Output directory: {output_dir}")

    # Create toolchain
    toolchain = EmscriptenToolchain(fastled_path=fastled_path)

    # Check if Emscripten is installed
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

    start_time = time.time()

    try:
        # Compile using native EMSDK
        js_file = toolchain.compile(
            sketch_dir=directory,
            output_dir=output_dir,
            build_mode=build_mode,
            profile=profile,
        )

        compile_time = time.time() - start_time

        # Create a zip of the output for consistency with web compilation
        zip_start = time.time()
        with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as tmp_zip:
            tmp_zip_path = Path(tmp_zip.name)

        shutil.make_archive(
            str(tmp_zip_path.with_suffix("")),
            "zip",
            output_dir,
        )
        zip_bytes = tmp_zip_path.read_bytes()
        tmp_zip_path.unlink()
        zip_time = time.time() - zip_start

        wasm_file = js_file.with_suffix(".wasm")
        stdout = f"Native compilation successful!\nOutput: {js_file}\nWASM: {wasm_file}"

        return CompileResult(
            success=True,
            stdout=stdout,
            hash_value=None,  # Native doesn't use hash caching currently
            zip_bytes=zip_bytes,
            zip_time=zip_time,
            libfastled_time=0.0,  # Library is pre-built
            sketch_time=compile_time,
            response_processing_time=0.0,
        )

    except Exception as e:
        compile_time = time.time() - start_time
        error_msg = f"Native compilation failed: {e}"
        if profile:
            import traceback

            error_msg += f"\n{traceback.format_exc()}"

        return CompileResult(
            success=False,
            stdout=error_msg,
            hash_value=None,
            zip_bytes=b"",
            zip_time=0.0,
            libfastled_time=0.0,
            sketch_time=compile_time,
            response_processing_time=0.0,
        )


def run_native_compile(
    directory: Path,
    build_mode: BuildMode = BuildMode.QUICK,
    profile: bool = False,
    open_browser: bool = True,
    keep_running: bool = True,
    enable_https: bool = True,
    fastled_path: Path | str | None = None,
) -> int:
    """
    Run native compilation with optional browser and file watching.

    Args:
        directory: Path to the sketch directory
        build_mode: Build mode (DEBUG, QUICK, RELEASE)
        profile: Enable profiling output
        open_browser: Whether to open browser after compilation
        keep_running: Whether to watch for file changes and recompile
        enable_https: Enable HTTPS for the local server
        fastled_path: Path to FastLED library. If None, downloads from master repo.

    Returns:
        Exit code (0 for success, non-zero for failure)
    """
    from fastled.filewatcher import DebouncedFileWatcherProcess, FileWatcherProcess
    from fastled.keyboard import SpaceBarWatcher
    from fastled.open_browser import spawn_http_server

    result = compile_native(directory, build_mode, profile, fastled_path)

    if not result.success:
        print(f"\n{EMO('❌', 'ERROR:')} Compilation failed:")
        print(result.stdout)
        return 1

    print(f"\n{EMO('✅', 'SUCCESS:')} Compilation successful!")
    print(f"  Time: {result.sketch_time:.2f} seconds")
    print(f"  Output: {directory / 'fastled_js'}")

    if not open_browser and not keep_running:
        return 0

    # Start HTTP server
    output_dir = directory / "fastled_js"
    http_proc = None

    if open_browser:
        http_proc = spawn_http_server(
            output_dir,
            port=None,  # Auto-select port
            compile_server_port=0,  # No compile server for native
            open_browser=True,
            app=False,
            enable_https=enable_https,
        )

    if not keep_running:
        return 0

    # Set up file watching
    excluded_patterns = ["fastled_js"]
    debounced_watcher = DebouncedFileWatcherProcess(
        FileWatcherProcess(directory, excluded_patterns=excluded_patterns),
    )

    print("\nWill compile on sketch changes or if you hit the space bar.")
    print("Press Ctrl+C to stop...")

    try:
        while True:
            if SpaceBarWatcher.watch_space_bar_pressed(timeout=1.0):
                print("\nCompiling...")
                result = compile_native(directory, build_mode, profile, fastled_path)
                if result.success:
                    print(f"{EMO('✅', 'SUCCESS:')} Recompilation successful!")
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
                    result = compile_native(
                        directory, build_mode, profile, fastled_path
                    )
                    if result.success:
                        print(f"{EMO('✅', 'SUCCESS:')} Recompilation successful!")
                    else:
                        print(f"{EMO('❌', 'ERROR:')} Recompilation failed:")
                        print(result.stdout)
                    continue

    except KeyboardInterrupt:
        print("\nStopping watch mode...")
    finally:
        debounced_watcher.stop()
        if http_proc:
            http_proc.kill()

    return 0
