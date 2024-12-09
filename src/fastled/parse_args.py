import argparse
import os
import sys
from pathlib import Path

from fastled import __version__
from fastled.docker_manager import DockerManager
from fastled.env import DEFAULT_URL
from fastled.project_init import project_init
from fastled.select_sketch_directory import select_sketch_directory
from fastled.sketch import (
    find_sketch_directories,
    looks_like_fastled_repo,
    looks_like_sketch_directory,
)


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=f"FastLED WASM Compiler {__version__}")
    parser.add_argument(
        "--version", action="version", version=f"%(prog)s {__version__}"
    )
    parser.add_argument(
        "directory",
        type=str,
        nargs="?",
        default=None,
        help="Directory containing the FastLED sketch to compile",
    )
    parser.add_argument(
        "--init",
        action="store_true",
        help="Initialize the FastLED sketch in the current directory",
    )
    parser.add_argument(
        "--just-compile",
        action="store_true",
        help="Just compile, skip opening the browser and watching for changes.",
    )
    parser.add_argument(
        "--web",
        "-w",
        type=str,
        nargs="?",
        # const does not seem to be working as expected
        const=DEFAULT_URL,  # Default value when --web is specified without value
        help="Use web compiler. Optional URL can be provided (default: https://fastled.onrender.com)",
    )
    parser.add_argument(
        "-i",
        "--interactive",
        action="store_true",
        help="Run in interactive mode (Not available with --web)",
    )
    parser.add_argument(
        "--profile",
        action="store_true",
        help="Enable profiling for web compilation",
    )
    parser.add_argument(
        "--force-compile",
        action="store_true",
        help="Skips the test to see if the current directory is a valid FastLED sketch directory",
    )
    parser.add_argument(
        "--no-auto-updates",
        action="store_true",
        help="Disable automatic updates of the wasm compiler image when using docker.",
    )
    parser.add_argument(
        "--update",
        "--upgrade",
        action="store_true",
        help="Update the wasm compiler (if necessary) before running",
    )
    parser.add_argument(
        "--localhost",
        "--local",
        "-l",
        action="store_true",
        help="Use localhost for web compilation from an instance of fastled --server, creating it if necessary",
    )
    parser.add_argument(
        "--server",
        "-s",
        action="store_true",
        help="Run the server in the current directory, volume mapping fastled if we are in the repo",
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

    cwd_is_fastled = looks_like_fastled_repo(Path(os.getcwd()))

    args = parser.parse_args()

    if args.init:
        args.directory = project_init()
        print("\nInitialized FastLED project in", args.directory)
        print(f"Use 'fastled {args.directory}' to compile the project.")
        sys.exit(0)

    if not args.update:
        if args.no_auto_updates:
            args.auto_update = False
        else:
            args.auto_update = None

        if (
            not cwd_is_fastled
            and not args.localhost
            and not args.web
            and not args.server
        ):
            # print(f"Using web compiler at {DEFAULT_URL}")
            args.web = DEFAULT_URL
            if DockerManager.is_docker_installed():
                print("Docker is installed. Use --server to run the compiler locally.")
                args.localhost = True
            else:
                print("Docker is not installed. Using web compiler.")
        if cwd_is_fastled and not args.web and not args.server:
            print("Forcing --local mode because we are in the FastLED repo")
            args.localhost = True
        if args.localhost:
            args.web = "localhost"
        if args.interactive and not args.server:
            print("--interactive forces --server mode")
            args.server = True
        if args.directory is None and not args.server:
            # does current directory look like a sketch?
            maybe_sketch_dir = Path(os.getcwd())
            if looks_like_sketch_directory(maybe_sketch_dir):
                args.directory = str(maybe_sketch_dir)
            else:
                sketch_directories = find_sketch_directories(maybe_sketch_dir)
                selected_dir = select_sketch_directory(
                    sketch_directories, cwd_is_fastled
                )
                if selected_dir:
                    print(f"Using sketch directory: {selected_dir}")
                    args.directory = selected_dir
                else:
                    print(
                        "\nYou either need to specify a sketch directory or run in --server mode."
                    )
                    sys.exit(1)
        elif args.directory is not None and os.path.isfile(args.directory):
            dir_path = Path(args.directory).parent
            if looks_like_sketch_directory(dir_path):
                print(f"Using sketch directory: {dir_path}")
                args.directory = str(dir_path)

    return args
