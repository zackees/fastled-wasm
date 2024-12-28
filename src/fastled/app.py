"""
Uses the latest wasm compiler image to compile the FastLED sketch.
"""

import argparse
import sys
import time
from pathlib import Path

from fastled.client_server import run_client_server
from fastled.compile_server import CompileServer
from fastled.parse_args import parse_args


def run_server(args: argparse.Namespace) -> int:
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
    args = parse_args()
    if args.update:
        # Force auto_update to ensure update check happens
        compile_server = CompileServer(interactive=False, auto_updates=True)
        compile_server.stop()
        print("Finished updating.")
        return 0

    if args.build:
        try:
            project_root = Path(".").absolute()
            print(f"Building Docker image at {project_root}")
            from fastled import Api, Docker

            docker_image_name = Docker.build_from_fastled_repo(
                project_root=project_root
            )
            print(f"Built Docker image: {docker_image_name}")
            print("Running server")
            with Api.server(
                auto_updates=False, container_name=docker_image_name
            ) as server:
                print(f"Server started at {server.url()}")
                sketch_dir = Path("examples/wasm")
                with Api.live_client(
                    sketch_directory=sketch_dir, host=server
                ) as client:
                    print(f"Client started at {client.url()}")
                    while True:
                        time.sleep(0.1)
        except KeyboardInterrupt:
            print("\nExiting from client...")
            return 1

    if args.server:
        print("Running in server only mode.")
        return run_server(args)
    else:
        print("Running in client/server mode.")
        return run_client_server(args)


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
