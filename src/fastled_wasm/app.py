"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import platform
import shutil
import sys
import tempfile
import time
from pathlib import Path

from fastled_wasm.build_mode import BuildMode, get_build_mode
from fastled_wasm.compile import CompiledResult, compile_local
from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager
from fastled_wasm.filewatcher import FileChangedNotifier
from fastled_wasm.open_browser import open_browser_thread
from fastled_wasm.web_compile import web_compile

machine = platform.machine().lower()
IS_ARM: bool = "arm" in machine or "aarch64" in machine
PLATFORM_TAG: str = "-arm64" if IS_ARM else ""
CONTAINER_NAME = f"fastled-wasm-compiler{PLATFORM_TAG}"


DOCKER = DockerManager(container_name=CONTAINER_NAME)
CONFIG: Config = Config()


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description="FastLED WASM Compiler")
    parser.add_argument(
        "directory",
        type=str,
        nargs="?",
        default=os.getcwd(),
        help="Directory containing the FastLED sketch to compile",
    )
    parser.add_argument(
        "--just-compile",
        action="store_true",
        help="Just compile, skip opening the browser and watching for changes.",
    )
    parser.add_argument(
        "--web",
        "-w",
        action="store_true",
        help="Use web compiler instead of local Docker. Implies --just-compile and disables --reuse and --exclude",
    )
    parser.add_argument(
        "--web-host",
        type=str,
        help="Host URL for the web compiler (default: https://fastled.onrender.com), implies --web",
    )
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="Reuse the existing container if it exists. (Not available with --web)",
    )
    parser.add_argument(
        "--exclude",
        type=str,
        nargs="+",
        help="Additional patterns to exclude from file watching (Not available with --web)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Pull the latest build image before compiling",
    )
    build_mode = parser.add_mutually_exclusive_group()
    build_mode.add_argument("--debug", action="store_true", help="Build in debug mode")
    build_mode.add_argument(
        "--quick",
        action="store_true",
        default=True,
        help="Build in quick mode (default)",
    )
    build_mode.add_argument(
        "--release", action="store_true", help="Build in release mode"
    )

    args = parser.parse_args()

    if args.web_host:
        args.web = True

    # Handle --web implications
    if args.web:
        if args.reuse:
            parser.error("--reuse cannot be used with --web")
        if args.exclude:
            parser.error("--exclude cannot be used with --web")

    return args


def run_web_compiler(directory: Path, host: str) -> CompiledResult:
    input_dir = Path(directory)
    output_dir = input_dir / "fastled_js"
    start = time.time()
    web_result = web_compile(input_dir, host)
    diff = time.time() - start
    if not web_result:
        print("\nWeb compilation failed:")
        print(f"Time taken: {diff:.2f} seconds")
        print(web_result.stdout)
        return CompiledResult(success=False, fastled_js="")

    # Extract zip contents to fastled_js directory
    output_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)
        temp_zip = temp_path / "result.zip"
        temp_zip.write_bytes(web_result.zip_bytes)

        # Clear existing contents
        shutil.rmtree(output_dir, ignore_errors=True)
        output_dir.mkdir(exist_ok=True)

        # Extract zip contents
        shutil.unpack_archive(temp_zip, output_dir, "zip")

    print(web_result.stdout)
    print(f"\nWeb compilation successful\n  Time: {diff:.2f}\n  output: {output_dir}")
    return CompiledResult(success=True, fastled_js=str(output_dir))


def main() -> int:
    args = parse_args()
    open_web_browser = not args.just_compile

    # If not explicitly using web compiler, check Docker installation
    if not args.web and not DOCKER.is_docker_installed():
        print(
            "\nDocker is not installed on this system - switching to web compiler instead."
        )
        args.web = True

    build_mode: BuildMode = get_build_mode(args)

    def _run_web_compiler(build_mode: BuildMode = build_mode) -> CompiledResult:
        return run_web_compiler(args.directory, args.web_host)

    def _compile_local(build_mode: BuildMode = build_mode) -> CompiledResult:
        return compile_local(
            args.directory, args.reuse, force_update=args.update, build_mode=build_mode
        )

    compiler_type = "web" if args.web else "local"
    compile_function = _run_web_compiler if args.web else _compile_local  # type: ignore

    result: CompiledResult = compile_function()

    if not result.success and compiler_type == "local":
        print("Failed to run local compiler. Trying web compiler instead...")
        result = _run_web_compiler()

    if not result.success:
        print("\nCompilation failed.")
        return 1

    if open_web_browser:
        open_browser_thread(Path(args.directory) / "fastled_js")
    else:
        print(
            "\nCompilation successful. Run without --just-compile to open in browser and watch for changes."
        )
        return 0

    if args.just_compile:
        return 0 if result.success else 1

    # Watch mode
    print("\nWatching for changes. Press Ctrl+C to stop...")
    watcher = FileChangedNotifier(args.directory, excluded_patterns=["fastled_js"])
    watcher.start()

    try:
        while True:
            try:
                changed_files = watcher.get_all_changes()
            except Exception as e:
                print(f"Error getting changes: {e}")
                changed_files = []
            if changed_files:
                print(f"\nChanges detected in {changed_files}")
                result = compile_function()
                if not result.success:
                    print("\nRecompilation failed.")
                else:
                    print("\nRecompilation successful.")
            time.sleep(0.3)
    except KeyboardInterrupt:
        watcher.stop()
        print("\nStopping watch mode...")
        return 0
    except Exception as e:
        watcher.stop()
        print(f"Error: {e}")
        return 1
    finally:
        watcher.stop()


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
