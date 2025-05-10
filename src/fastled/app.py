"""
Uses the latest wasm compiler image to compile the FastLED sketch.
"""

import sys
import time
from pathlib import Path

from fastled.client_server import run_client_server
from fastled.compile_server import CompileServer
from fastled.filewatcher import file_watcher_set
from fastled.parse_args import Args, parse_args


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

    args = parse_args()
    interactive: bool = args.interactive
    has_server = args.server
    update: bool = args.update
    build: bool = args.build
    just_compile: bool = args.just_compile
    # directory: Path | None = Path(args.directory).absolute() if args.directory else None
    directory: Path | None = Path(args.directory) if args.directory else None

    # now it is safe to print out the version
    print(f"FastLED version: {__version__}")

    if update:
        # Force auto_update to ensure update check happens
        compile_server = CompileServer(interactive=False, auto_updates=True)
        compile_server.stop()
        print("Finished updating.")
        return 0

    if build:
        print("Building is disabled")
        build = False

    if build:
        raise NotImplementedError("Building is not yet supported.")
        file_watcher_set(False)
        project_root = Path(".").absolute()
        print(f"Building Docker image at {project_root}")
        from fastled import Api, Docker

        server = Docker.spawn_server_from_fastled_repo(
            project_root=project_root,
            interactive=interactive,
            sketch_folder=directory,
        )
        assert isinstance(server, CompileServer)

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
