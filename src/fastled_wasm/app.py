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

from fastled_wasm.compile import compile
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


def main() -> int:
    args = parse_args()
    open_web_browser = not args.just_compile

    # Choose between web and local compilation
    if args.web:
        host = args.web_host
        input_dir = Path(args.directory)
        output_dir = input_dir / "fastled_js"
        start = time.time()
        web_result = web_compile(input_dir, host)
        diff = time.time() - start
        if not web_result:
            print("\nWeb compilation failed:")
            print(f"Time taken: {diff:.2f} seconds")
            print(web_result.stdout)
            return 1

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
        print(
            f"\nWeb compilation successful\n  Time: {diff:.2f}\n  output: {output_dir}"
        )
        if open_web_browser:
            open_browser_thread(output_dir)
            try:
                while True:
                    time.sleep(1)
            except KeyboardInterrupt:
                print("\nExiting...")
                return 0
        return 0

    # Compile the sketch locally.
    result = compile(args.directory, args.reuse, force_update=args.update)
    if result.return_code != 0:
        print("\nCompilation failed.")
        return result.return_code

    if result.return_code == 0:
        if open_web_browser:
            open_browser_thread(Path(result.fastled_js))
        else:
            print(
                "\nCompilation successful. Run without --just-compile to open in browser and watch for changes."
            )
            return 0

    if args.just_compile:
        return result.return_code

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
                result = compile(args.directory, args.reuse, force_update=args.update)
                if result.return_code != 0:
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
