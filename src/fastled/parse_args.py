import argparse
import os
import sys
from pathlib import Path

from fastled.args import Args
from fastled.project_init import project_init
from fastled.select_sketch_directory import select_sketch_directory
from fastled.settings import DEFAULT_URL, IMAGE_NAME
from fastled.sketch import (
    find_sketch_directories,
    looks_like_fastled_repo,
    looks_like_sketch_directory,
)


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
  --init [example]      Initialize one of the top tier WASM examples
  --web [url]           Use web compiler
  --server              Run the compiler server
  --quick               Build in quick mode (default)
  --profile             Enable profiling the C++ build system
  --update              Update the docker image for the wasm compiler
  --purge               Remove all FastLED containers and images
  --version             Show version information
  --help                Show detailed help
Examples:
  fastled (will auto detect the sketch directory and prompt you)
  fastled my_sketch
  fastled my_sketch --web (compiles using the web compiler only)
  fastled --init Blink (initializes a new sketch directory with the Blink example)
  fastled --server (runs the compiler server in the current directory)

For those using Docker:
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
        "--ram-disk-size",
        type=str,
        default="0",
        help="Set the size of the ramdisk for the docker container. Use suffixes like '25mb' or '1gb'.",
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
        help="Enable profiling of the C++ build system used for wasm compilation.",
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
        "-u",
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
        help="(Default): Use localhost for web compilation from an instance of fastled --server, creating it if necessary",
    )
    parser.add_argument(
        "--build",
        "-b",
        action="store_true",
        help="Build the wasm compiler image from the FastLED repo",
    )
    parser.add_argument(
        "--server",
        "-s",
        action="store_true",
        help="Run the server in the current directory, volume mapping fastled if we are in the repo",
    )
    parser.add_argument(
        "--purge",
        action="store_true",
        help="Remove all FastLED containers and images",
    )

    parser.add_argument(
        "--clear",
        action="store_true",
        help="Remove all FastLED containers and images",
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

    cwd_is_fastled = looks_like_fastled_repo(Path(os.getcwd()))

    args = parser.parse_args()

    # TODO: propagate the library.
    # from fastled.docker_manager import force_remove_previous

    # if force_remove_previous():
    #     print("Removing previous containers...")
    # do itinfront he camer
    # nonw invoke via the
    #
    # Work in progress.
    # set_ramdisk_size("50mb")

    # if args.ram_disk_size != "0":
    #     from fastled.docker_manager import set_ramdisk_size
    #     from fastled.util import banner_string

    #     msg = banner_string(f"Setting tmpfs size to {args.ram_disk_size}")
    #     print(msg)
    #     set_ramdisk_size(args.ram_disk_size)

    if args.purge:
        from fastled.docker_manager import DockerManager

        docker = DockerManager()
        docker.purge(IMAGE_NAME)
        sys.exit(0)

    if args.init:
        example = args.init if args.init is not True else None
        try:
            args.directory = project_init(example, args.directory)
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
            # if --quick, --debug, --release are not specified then default to --debug
            args.quick = True
            print("Defaulting to --quick mode in fastled repo")
        else:
            args.quick = True
            print("Defaulting to --quick mode")

    if args.build or args.interactive:
        if args.directory is not None:
            args.directory = str(Path(args.directory).absolute())
        if not is_fastled_dir:
            print("This command must be run from within the FastLED repo. Exiting...")
            sys.exit(1)
        if cwd != fastled_dir and fastled_dir is not None:
            print(f"Switching to FastLED repo at {fastled_dir}")
            os.chdir(fastled_dir)
        if args.directory is None:
            args.directory = str(Path("examples/wasm").absolute())
        if args.interactive:
            if not args.build:
                print("Adding --build flag when using --interactive")
                args.build = True
        user_wants_update = args.update
        if user_wants_update is not True:
            args.no_auto_updates = True
        return Args.from_namespace(args)

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
            from fastled.docker_manager import DockerManager

            if DockerManager.is_docker_installed():
                if not DockerManager.ensure_linux_containers_for_windows():
                    print(
                        f"Windows must be in linux containers mode, but is in Windows container mode, Using web compiler at {DEFAULT_URL}."
                    )
                    args.web = DEFAULT_URL
                else:
                    print(
                        "Docker is installed. Defaulting to --local mode, use --web to override and use the web compiler instead."
                    )
                    args.localhost = True
            else:
                print(f"Docker is not installed. Using web compiler at {DEFAULT_URL}.")
                args.web = DEFAULT_URL
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
                print("Searching for sketch directories...")
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

    return Args.from_namespace(args)
