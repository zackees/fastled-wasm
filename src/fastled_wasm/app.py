"""
Uses the latest wasm compiler image to compile the FastLED sketch.


"""

import argparse
import os
import sys
from pathlib import Path

from fastled_wasm.compile import compile
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

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    result = compile(args.directory, args.reuse)

    if result.return_code != 0:
        print("\nCompilation failed.")
        return result.return_code

    if result.return_code == 0 and not args.no_open:
        open_browser(Path(result.fastled_js))
    elif result.return_code == 0:
        print(
            "\nIf you want to open the compiled sketch in a web browser, run this command with --open flag."
        )

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
