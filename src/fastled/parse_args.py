import argparse
import os
import sys
from pathlib import Path

from fastled.args import Args
from fastled.project_init import project_init
from fastled.select_sketch_directory import select_sketch_directory
from fastled.sketch import (
    find_sketch_by_partial_name,
    find_sketch_directories,
    looks_like_fastled_repo,
    looks_like_sketch_directory,
)


def _resolve_path(path_str: str) -> str:
    """Resolve a path string, handling MSYS /c/... style paths on Windows."""
    import re

    resolved = Path(path_str).resolve()
    if resolved.exists():
        return str(resolved)
    # Handle MSYS-style paths: /c/Users/... -> C:\Users\...
    if sys.platform == "win32":
        m = re.match(r"^/([a-zA-Z])/(.*)", path_str)
        if m:
            win_path = Path(f"{m.group(1).upper()}:/{m.group(2)}")
            if win_path.exists():
                return str(win_path.resolve())
    # Try expanduser for ~ paths
    expanded = Path(os.path.expanduser(path_str)).resolve()
    if expanded.exists():
        return str(expanded)
    return path_str


def _find_fastled_repo(start: Path) -> Path | None:
    """Find the FastLED repo directory by searching upwards from the current directory."""
    current = start
    while current != current.parent:
        if looks_like_fastled_repo(current):
            return current
        current = current.parent
    return None


_DEFAULT_HELP_TEXT = """
FastLED WASM Compiler - Useful options:
  <directory>           Directory containing the FastLED sketch to compile
  --serve-dir <dir>     Serve an existing output directory without compiling
  --init [example]      Initialize one of the top tier WASM examples
  --no-https            Disable HTTPS and use HTTP for local server
  --quick               Build in quick mode (default)
  --profile             Enable profiling the C++ build system
  --version             Show version information
  --help                Show detailed help
Examples:
  fastled (will auto detect the sketch directory and compile natively)
  fastled my_sketch
  fastled --init Blink (initializes a new sketch directory with the Blink example)

Build modes:
  --debug               Build with debug symbols for dev-tools debugging
  --release             Build in optimized release mode
"""


def parse_args() -> Args:
    """Parse command-line arguments."""
    from fastled import __version__

    # Check if no arguments were provided
    if len(sys.argv) == 1:
        print(_DEFAULT_HELP_TEXT)

    parser = argparse.ArgumentParser(description=f"FastLED WASM Compiler {__version__}")
    parser.add_argument("--version", action="version", version=f"{__version__}")
    parser.add_argument(
        "directory",
        type=str,
        nargs="?",
        default=None,
        help="Directory containing the FastLED sketch to compile",
    )
    parser.add_argument(
        "--serve-dir",
        type=str,
        default=None,
        help="Serve an existing directory without compiling a sketch",
    )
    parser.add_argument(
        "--init",
        nargs="?",
        const=True,
        help="Initialize the FastLED sketch in the current directory. Optional name can be provided",
    )
    parser.add_argument(
        "--just-compile",
        action="store_true",
        help="Just compile, skip opening the browser and watching for changes.",
    )
    parser.add_argument(
        "--profile",
        action="store_true",
        help="Enable profiling of the C++ build system used for wasm compilation.",
    )
    parser.add_argument(
        "--app",
        action="store_true",
        help="Use Playwright app-like browser experience (will download browsers if needed)",
    )

    parser.add_argument(
        "--install",
        action="store_true",
        help="Install FastLED development environment with VSCode configuration and Auto Debug extension",
    )

    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run in dry-run mode (simulate actions without making changes)",
    )

    parser.add_argument(
        "--no-interactive",
        action="store_true",
        help="Run in non-interactive mode (fail instead of prompting for input)",
    )

    parser.add_argument(
        "--no-https",
        action="store_true",
        help="Disable HTTPS and use HTTP for the local server (useful for debugging)",
    )

    parser.add_argument(
        "--local",
        action="store_true",
        help="Deprecated, only kept for backwards compatibility",
    )

    parser.add_argument(
        "--latest",
        action="store_true",
        help="Use latest release when initializing examples with --init (default behavior)",
    )
    parser.add_argument(
        "--branch",
        type=str,
        default=None,
        help="Use a specific branch when initializing examples with --init (e.g. --branch master)",
    )
    parser.add_argument(
        "--commit",
        type=str,
        default=None,
        help="Use a specific commit SHA when initializing examples with --init",
    )

    parser.add_argument(
        "--fastled-path",
        type=str,
        default=None,
        help="Path to FastLED library for native compilation (defaults to downloading from master repo)",
    )

    parser.add_argument(
        "--purge",
        action="store_true",
        help="Purge the cached FastLED repo download, forcing a fresh re-download on next build",
    )

    build_mode = parser.add_mutually_exclusive_group()
    build_mode.add_argument("--debug", action="store_true", help="Build in debug mode")
    build_mode.add_argument(
        "--quick",
        action="store_true",
        help="Build in quick mode (default)",
    )
    build_mode.add_argument(
        "--release", action="store_true", help="Build in release mode"
    )

    args = parser.parse_args()

    # Resolve --fastled-path to an absolute path (handles MSYS /c/... paths on Windows)
    if args.fastled_path:
        args.fastled_path = _resolve_path(args.fastled_path)
    if args.serve_dir:
        args.serve_dir = _resolve_path(args.serve_dir)

    # Auto-detect FastLED repo for native compilation
    if not args.fastled_path:
        cwd = Path(os.getcwd())
        fastled_repo = _find_fastled_repo(cwd)
        if fastled_repo is not None:
            print(
                f"Detected FastLED repo at {fastled_repo}, using it for native compilation."
            )
            args.fastled_path = str(fastled_repo)

    # Auto-enable app mode if debug is used and Playwright cache exists
    if args.debug and not args.app:
        playwright_dir = Path.home() / ".fastled" / "playwright"
        if playwright_dir.exists() and any(playwright_dir.iterdir()):
            from fastled.emoji_util import EMO

            print(
                f"{EMO('warning', 'WARNING:')} Debug mode detected with Playwright installed - automatically enabling app mode"
            )
            args.app = True
        elif not args.no_interactive:
            # Prompt user to install Playwright only if not in no-interactive mode
            answer = (
                input("Would you like to install the FastLED debugger? [y/n] ")
                .strip()
                .lower()
            )
            if answer in ["y", "yes"]:
                print(
                    "To install Playwright, run: pip install playwright && python -m playwright install"
                )
                print("Then run your command again with --app flag")
                sys.exit(0)

    # Handle --install early before other processing
    if args.install:
        # Don't process other arguments when --install is used
        return Args.from_namespace(args)

    if args.serve_dir:
        return Args.from_namespace(args)

    # Handle --purge early: if no directory given, skip directory resolution
    if args.purge and args.directory is None:
        return Args.from_namespace(args)

    if args.init:
        example = args.init if args.init is not True else None
        # --latest is mutually exclusive with --branch and --commit
        if args.latest and (args.branch or args.commit):
            print("Error: --latest cannot be used with --branch or --commit")
            sys.exit(1)
        # Resolve ref: --commit takes precedence over --branch
        ref: str | None = None
        if args.commit:
            ref = args.commit
        elif args.branch:
            ref = args.branch
        # --latest (or default) leaves ref=None, which means latest release
        try:
            args.directory = project_init(example, args.directory, ref=ref)
        except Exception as e:
            print(f"Failed to initialize project: {e}")
            sys.exit(1)
        print("\nInitialized FastLED project in", args.directory)
        print(f"Use 'fastled {args.directory}' to compile the project.")
        sys.exit(0)

    cwd: Path = Path(os.getcwd())
    fastled_dir: Path | None = _find_fastled_repo(cwd)
    is_fastled_dir: bool = fastled_dir is not None

    if not (args.debug or args.quick or args.release):
        if is_fastled_dir:
            args.quick = True
            print("Defaulting to --quick mode in fastled repo")
        else:
            args.quick = True
            print("Defaulting to --quick mode")

    if args.directory is None:
        # does current directory look like a sketch?
        maybe_sketch_dir = Path(os.getcwd())
        if looks_like_sketch_directory(maybe_sketch_dir):
            args.directory = str(maybe_sketch_dir)
        else:
            print("Searching for sketch directories...")
            cwd_is_fastled = looks_like_fastled_repo(Path(os.getcwd()))
            sketch_directories = find_sketch_directories(maybe_sketch_dir)
            selected_dir = select_sketch_directory(
                sketch_directories, cwd_is_fastled, is_followup=True
            )
            if selected_dir:
                print(f"Using sketch directory: {selected_dir}")
                args.directory = selected_dir
            else:
                print("\nYou need to specify a sketch directory.")
                sys.exit(1)
    elif args.directory is not None:
        # Check if directory is a file path
        if os.path.isfile(args.directory):
            dir_path = Path(args.directory).parent
            if looks_like_sketch_directory(dir_path):
                print(f"Using sketch directory: {dir_path}")
                args.directory = str(dir_path)
        # Check if directory exists as a path
        elif not os.path.exists(args.directory):
            # Directory doesn't exist - try partial name matching
            try:
                matched_dir = find_sketch_by_partial_name(args.directory)
                print(f"Matched '{args.directory}' to sketch directory: {matched_dir}")
                args.directory = str(matched_dir)
            except ValueError as e:
                print(f"Error: {e}")
                sys.exit(1)

    return Args.from_namespace(args)
