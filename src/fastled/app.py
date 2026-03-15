"""
Uses native EMSDK to compile FastLED sketches to WASM.
"""

import os
import sys
from pathlib import Path

from fastled.emoji_util import EMO
from fastled.parse_args import parse_args


def main() -> int:
    from fastled import __version__

    args = parse_args()

    # Handle --install command early
    if args.install:
        from fastled.install.main import fastled_install

        result = fastled_install(
            dry_run=args.dry_run, no_interactive=args.no_interactive
        )
        return 0 if result else 1

    just_compile: bool = args.just_compile
    directory: Path | None = Path(args.directory) if args.directory else None

    # now it is safe to print out the version
    print(f"FastLED version: {__version__}")

    # Print current working directory
    print(f"Current working directory: {os.getcwd()}")

    # Check if Playwright browsers are installed
    playwright_dir = Path.home() / ".fastled" / "playwright"
    if playwright_dir.exists() and any(playwright_dir.iterdir()):
        print(
            f"{EMO('theatre', '*')} Playwright browsers available at: {playwright_dir}"
        )

    # Native compilation mode (the only mode now)
    from fastled.compile_native import run_native_compile
    from fastled.types import BuildMode

    if directory is None:
        print("Error: No sketch directory specified for compilation.")
        return 1

    print("Running in native EMSDK compilation mode (no Docker required).")
    build_mode = BuildMode.from_args(args)
    return run_native_compile(
        directory=directory,
        build_mode=build_mode,
        profile=args.profile,
        open_browser=not just_compile,
        keep_running=not just_compile,
        enable_https=args.enable_https,
        fastled_path=args.fastled_path,
        app=args.app,
    )


if __name__ == "__main__":
    # Note that the entry point for the exe is in cli.py
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting from main...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
