"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import sys
import threading
from pathlib import Path

from fastled_wasm.compile import compile
from fastled_wasm.filewatcher import FileChangedNotifier
from fastled_wasm.open_browser import open_browser


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
        "--no-open",
        action="store_true",
        help="Just compile, skip the step where the browser is opened.",
    )
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="Reuse the existing container if it exists.",
    )
    parser.add_argument(
        "--watch",
        action="store_true",
        help="Watch for file changes and recompile automatically.",
    )

    args = parser.parse_args()
    if args.watch:
        args.no_open = False

    return args


def open_browser_threaded(fastled_js: Path) -> None:
    """Opens browser in a daemon thread"""
    browser_thread = threading.Thread(
        target=open_browser, args=(fastled_js,), daemon=True
    )
    browser_thread.start()


def main() -> int:
    args = parse_args()
    open_web_browser = not args.no_open

    # Initial compilation
    result = compile(args.directory, args.reuse)
    if result.return_code != 0:
        print("\nInitial compilation failed.")
        return result.return_code

    if result.return_code == 0 and open_web_browser:
        open_browser_threaded(Path(result.fastled_js))
        if not args.watch:
            print("\nPress Ctrl+C to exit...")
            try:
                while True:
                    pass
            except KeyboardInterrupt:
                print("\nExiting...")
                return 0
    elif result.return_code == 0:
        print(
            "\nIf you want to open the compiled sketch in a web browser, run this command with --open flag."
        )

    if not args.watch:
        return result.return_code

    # Watch mode
    print("\nWatching for changes. Press Ctrl+C to stop...")
    watcher = FileChangedNotifier(args.directory)
    watcher.start()

    try:
        while True:
            changed_file = watcher.get_next_change()
            if changed_file:
                print(f"\nFile changed: {changed_file}")
                result = compile(args.directory, args.reuse)
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
