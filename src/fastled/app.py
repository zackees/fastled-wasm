"""
Uses the latest wasm compiler image to compile the FastLED sketch.
"""

import os
import sys
import time
from pathlib import Path

from fastled.client_server import run_client_server
from fastled.compile_server import CompileServer
from fastled.emoji_util import EMO
from fastled.filewatcher import file_watcher_set
from fastled.parse_args import Args, parse_args
from fastled.settings import DEFAULT_URL
from fastled.sketch import find_sketch_directories, looks_like_fastled_repo


def run_server(args: Args) -> int:
    interactive = args.interactive
    auto_update = args.auto_update
    mapped_dir = Path(args.directory).absolute() if args.directory else None
    if interactive and mapped_dir is None:
        print("Select a sketch when you enter interactive mode.")
        return 1
    compile_server = CompileServer(
        interactive=interactive,
        auto_updates=auto_update,
        mapped_dir=mapped_dir,
        auto_start=True,
        remove_previous=args.clear,
        no_platformio=args.no_platformio,
    )

    if not interactive:
        print(f"Server started at {compile_server.url()}")
    try:
        while True:
            if not compile_server.process_running():
                print("Server process is not running. Exiting...")
                return 1
            time.sleep(0.1)
    except KeyboardInterrupt:
        print("\nExiting from server...")
        return 1
    finally:
        compile_server.stop()
    return 0


def main() -> int:
    from fastled import __version__
    from fastled.select_sketch_directory import select_sketch_directory

    args = parse_args()

    if args.emsdk_headers:
        import httpx

        out_path = args.emsdk_headers
        base_url = args.web if isinstance(args.web, str) else DEFAULT_URL
        try:
            response = httpx.get(f"{base_url}/headers/emsdk")
            if response.status_code == 200:
                Path(out_path).parent.mkdir(parents=True, exist_ok=True)
                with open(out_path, "wb") as f:
                    f.write(response.content)
                print(f"{EMO('✅','SUCCESS:')} EMSDK headers exported to {out_path}")
                return 0
            else:
                print(
                    f"{EMO('❌','ERROR:')} Failed to export EMSDK headers: HTTP {response.status_code}"
                )
                return 1
        except KeyboardInterrupt:
            print("\nExiting from main...")
            return 1
        except Exception as e:
            print(f"{EMO('❌','ERROR:')} Exception: {e}")
            return 1

    # Handle --install command early
    if args.install:
        from fastled.install.main import fastled_install

        result = fastled_install(
            dry_run=args.dry_run, no_interactive=args.no_interactive
        )
        return 0 if result else 1

    interactive: bool = args.interactive
    has_server = args.server
    update: bool = args.update
    build: bool = args.build
    just_compile: bool = args.just_compile
    # directory: Path | None = Path(args.directory).absolute() if args.directory else None
    directory: Path | None = Path(args.directory) if args.directory else None
    cwd_looks_like_fastled_repo = looks_like_fastled_repo()

    # now it is safe to print out the version
    print(f"FastLED version: {__version__}")

    # Print current working directory
    print(f"Current working directory: {os.getcwd()}")

    # Check if Playwright browsers are installed
    playwright_dir = Path.home() / ".fastled" / "playwright"
    if playwright_dir.exists() and any(playwright_dir.iterdir()):
        print(f"{EMO('🎭', '*')} Playwright browsers available at: {playwright_dir}")

    # Resolve some of the last interactive arguments
    # 1. If interactive is set and the sketch directory is not given,
    # then prompt the user for a sketch directory.
    # 2. Tell the user they can use --server --interactive to
    # skip this prompt.
    if interactive and cwd_looks_like_fastled_repo and directory is None:
        answer = input(
            "No sketch directory selected, would you like to select one? (y/n): "
        )
        if answer.lower()[:1] == "y" or answer.lower() == "":
            sketch_list: list[Path] = find_sketch_directories()
            if sketch_list:
                maybe_dir: str | None = select_sketch_directory(
                    sketch_list, cwd_looks_like_fastled_repo
                )
                if maybe_dir is not None:
                    directory = Path(maybe_dir)
                    if not directory.exists():
                        print(
                            f"Directory {directory} does not exist, entering interactive mode without project mapped in."
                        )
                        directory = None

    if update:
        # Force auto_update to ensure update check happens
        compile_server = CompileServer(
            interactive=False, auto_updates=True, no_platformio=args.no_platformio
        )
        compile_server.stop()
        print("Finished updating.")
        return 0

    if build:
        print("Building is disabled")
        build = False

    if interactive:
        # raise NotImplementedError("Building is not yet supported.")
        file_watcher_set(False)
        # project_root = Path(".").absolute()
        # print(f"Building Docker image at {project_root}")
        from fastled import Api

        server: CompileServer = CompileServer(
            interactive=interactive,
            auto_updates=False,
            mapped_dir=directory,
            auto_start=False,
            remove_previous=args.clear,
            no_platformio=args.no_platformio,
        )

        server.start(wait_for_startup=False)

        try:
            while server.process_running():
                # wait for ctrl-c
                time.sleep(0.1)
        except KeyboardInterrupt:
            print("\nExiting from server...")
            server.stop()
            return 0

        try:
            if interactive:
                server.stop()
                return 0
            print(f"Built Docker image: {server.name}")
            if not directory:
                if not directory:
                    print("No directory specified")
                server.stop()
                return 0

            print("Running server")

            with Api.live_client(
                auto_updates=False,
                sketch_directory=directory,
                host=server,
                auto_start=True,
                keep_running=not just_compile,
            ) as _:
                while True:
                    time.sleep(0.2)  # wait for user to exit
        except KeyboardInterrupt:
            print("\nExiting from client...")
            server.stop()
            return 1

    if has_server:
        print("Running in server only mode.")
        return run_server(args)
    else:
        print("Running in client/server mode.")
        return run_client_server(args)


if __name__ == "__main__":
    # Note that the entry point for the exe is in cli.py
    try:
        # sys.argv.append("-i")
        # sys.argv.append("-b")
        # sys.argv.append("examples/wasm")
        # sys.argv.append()
        import os

        os.chdir("../fastled")
        sys.argv.append("examples/FxWave2d")
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting from main...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
