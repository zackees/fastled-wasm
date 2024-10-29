"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import platform
import sys
from pathlib import Path

from fastled_wasm.compile import compile
from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager
from fastled_wasm.filewatcher import FileChangedNotifier
from fastled_wasm.open_browser import open_browser_thread

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
        "--reuse",
        action="store_true",
        help="Reuse the existing container if it exists.",
    )
    parser.add_argument(
        "--exclude",
        type=str,
        nargs="+",
        help="Additional patterns to exclude from file watching",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Pull the latest build image before compiling",
    )

    args = parser.parse_args()

    return args


def main() -> int:
    args = parse_args()
    open_web_browser = not args.just_compile

    # Initial compilation
    result = compile(args.directory, args.reuse, force_update=args.update)
    if result.return_code != 0:
        print("\nInitial compilation failed.")
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
            # changed_file = watcher.get_next_change()
            changed_files = watcher.get_all_changes()
            if changed_files:
                print(f"\nChanges detected in {changed_files}")
                result = compile(args.directory, args.reuse, force_update=args.update)
                if result.return_code != 0:
                    print("\nRecompilation failed.")
                else:
                    print("\nRecompilation successful.")
    except KeyboardInterrupt:
        watcher.stop()
        print("\nStopping watch mode...")
        return 0
    finally:
        watcher.stop()

    return result.return_code


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
