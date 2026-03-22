"""
Uses native EMSDK to compile FastLED sketches to WASM.
"""

import os
import sys
from pathlib import Path

from fastled.emoji_util import EMO
from fastled.parse_args import parse_args


def purge_cache(cache_dir: Path, fastled_path: Path | str | None = None) -> None:
    """Purge cached FastLED repo and WASM build artifacts."""
    import shutil

    if cache_dir.exists():
        shutil.rmtree(cache_dir, ignore_errors=True)
        print(f"Purged FastLED cache: {cache_dir}")
    else:
        print("No FastLED cache to purge.")
    # Also purge WASM build caches in the fastled_path if provided
    if fastled_path:
        fastled_build = Path(fastled_path) / ".build"
        for wasm_dir in fastled_build.glob("meson-wasm-*"):
            for stale in ["wasm_ld_args.json", "wasm_ld_args.key", "fastled_glue.js"]:
                f = wasm_dir / stale
                if f.exists():
                    f.unlink()
                    print(f"Purged: {f}")


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

    # Handle --purge: clear cached FastLED repo and WASM build artifacts
    if args.purge:
        cache_dir = Path.home() / ".fastled" / "cache"
        purge_cache(cache_dir, args.fastled_path)

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
