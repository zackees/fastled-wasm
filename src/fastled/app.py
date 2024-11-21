"""
Uses the latest wasm compiler image to compile the FastLED sketch.
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

from fastled import __version__
from fastled.build_mode import BuildMode, get_build_mode
from fastled.compile_server import CompileServer
from fastled.docker_manager import DockerManager
from fastled.filewatcher import FileWatcherProcess
from fastled.keyboard import SpaceBarWatcher
from fastled.open_browser import open_browser_thread
from fastled.sketch import (
    find_sketch_directories,
    looks_like_fastled_repo,
    looks_like_sketch_directory,
)
from fastled.web_compile import (
    SERVER_PORT,
    ConnectionResult,
    find_good_connection,
    web_compile,
)

machine = platform.machine().lower()
IS_ARM: bool = "arm" in machine or "aarch64" in machine
PLATFORM_TAG: str = "-arm64" if IS_ARM else ""
CONTAINER_NAME = f"fastled-wasm-compiler{PLATFORM_TAG}"
DEFAULT_URL = "https://fastled.onrender.com"


@dataclass
class CompiledResult:
    """Dataclass to hold the result of the compilation."""

    success: bool
    fastled_js: str
    hash_value: str | None


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
    if args.update:
        args.auto_update = True
    elif args.no_auto_updates:
        args.auto_update = False
    else:
        args.auto_update = None

    if not cwd_is_fastled and not args.localhost and not args.web and not args.server:
        print(f"Using web compiler at {DEFAULT_URL}")
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
            sketch_directories = find_sketch_directories(maybe_sketch_dir)
            if len(sketch_directories) == 1:
                print(f"\nUsing sketch directory: {sketch_directories[0]}")
                args.directory = str(sketch_directories[0])
            elif len(sketch_directories) > 1:
                print("\nMultiple Directories found, choose one:")
                for i, sketch_dir in enumerate(sketch_directories):
                    print(f"  [{i+1}]: {sketch_dir}")
                which = input("\nPlease specify a sketch directory: ")
                try:
                    index = int(which) - 1
                    args.directory = str(sketch_directories[index])
                except (ValueError, IndexError):
                    print("Invalid selection.")
                    sys.exit(1)
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


def run_web_compiler(
    directory: Path,
    host: str,
    build_mode: BuildMode,
    profile: bool,
    last_hash_value: str | None,
) -> CompiledResult:
    input_dir = Path(directory)
    output_dir = input_dir / "fastled_js"
    start = time.time()
    web_result = web_compile(
        directory=input_dir, host=host, build_mode=build_mode, profile=profile
    )
    diff = time.time() - start
    if not web_result.success:
        print("\nWeb compilation failed:")
        print(f"Time taken: {diff:.2f} seconds")
        print(web_result.stdout)
        return CompiledResult(success=False, fastled_js="", hash_value=None)

    def print_results() -> None:
        hash_value = (
            web_result.hash_value
            if web_result.hash_value is not None
            else "NO HASH VALUE"
        )
        print(
            f"\nWeb compilation successful\n  Time: {diff:.2f}\n  output: {output_dir}\n  hash: {hash_value}\n  zip size: {len(web_result.zip_bytes)} bytes"
        )

    # now check to see if the hash value is the same as the last hash value
    if last_hash_value is not None and last_hash_value == web_result.hash_value:
        print("\nSkipping redeploy: No significant changes found.")
        print_results()
        return CompiledResult(
            success=True, fastled_js=str(output_dir), hash_value=web_result.hash_value
        )

    # Extract zip contents to fastled_js directory
    output_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)
        temp_zip = temp_path / "result.zip"
        temp_zip.write_bytes(web_result.zip_bytes)

        # Clear existing contents
        shutil.rmtree(output_dir, ignore_errors=True)
        output_dir.mkdir(exist_ok=True)

        # Extract zip contents
        shutil.unpack_archive(temp_zip, output_dir, "zip")

    print(web_result.stdout)
    print_results()
    return CompiledResult(
        success=True, fastled_js=str(output_dir), hash_value=web_result.hash_value
    )


def _try_start_server_or_get_url(args: argparse.Namespace) -> str | CompileServer:
    auto_update = args.auto_update
    is_local_host = "localhost" in args.web or "127.0.0.1" in args.web or args.localhost
    # test to see if there is already a local host server
    local_host_needs_server = False
    if is_local_host:
        addr = "localhost" if args.localhost else args.web
        urls = [addr]
        if ":" not in addr:
            urls.append(f"{addr}:{SERVER_PORT}")

        result: ConnectionResult | None = find_good_connection(urls)
        if result is not None:
            print(f"Found local server at {result.host}")
            return result.host
        else:
            local_host_needs_server = True

    if not local_host_needs_server and args.web:
        if isinstance(args.web, str):
            return args.web
        if isinstance(args.web, bool):
            return DEFAULT_URL
        return args.web
    else:
        try:
            print("No local server found, starting one...")
            compile_server = CompileServer(auto_updates=auto_update)
            print("Waiting for the local compiler to start...")
            if not compile_server.wait_for_startup():
                print("Failed to start local compiler.")
                raise RuntimeError("Failed to start local compiler.")
            return compile_server
        except KeyboardInterrupt:
            raise
        except RuntimeError:
            print("Failed to start local compile server, using web compiler instead.")
            return DEFAULT_URL


def run_client(args: argparse.Namespace) -> int:
    compile_server: CompileServer | None = None
    open_web_browser = not args.just_compile and not args.interactive
    profile = args.profile
    if not args.force_compile and not looks_like_sketch_directory(Path(args.directory)):
        print(
            "Error: Not a valid FastLED sketch directory, if you are sure it is, use --force-compile"
        )
        return 1

    # If not explicitly using web compiler, check Docker installation
    if not args.web and not DockerManager.is_docker_installed():
        print(
            "\nDocker is not installed on this system - switching to web compiler instead."
        )
        args.web = True

    url: str
    try:
        try:
            url_or_server: str | CompileServer = _try_start_server_or_get_url(args)
            if isinstance(url_or_server, str):
                print(f"Found URL: {url_or_server}")
                url = url_or_server
            else:
                compile_server = url_or_server
                print(f"Server started at {compile_server.url()}")
                url = compile_server.url()
        except KeyboardInterrupt:
            print("\nExiting from first try...")
            if compile_server:
                compile_server.stop()
            return 1
        except Exception as e:
            print(f"Error: {e}")
            return 1
        build_mode: BuildMode = get_build_mode(args)

        def compile_function(
            url: str = url,
            build_mode: BuildMode = build_mode,
            profile: bool = profile,
            last_hash_value: str | None = None,
        ) -> CompiledResult:
            return run_web_compiler(
                args.directory,
                host=url,
                build_mode=build_mode,
                profile=profile,
                last_hash_value=last_hash_value,
            )

        result: CompiledResult = compile_function(last_hash_value=None)
        last_compiled_result: CompiledResult = result

        if not result.success:
            print("\nCompilation failed.")

        browser_proc: subprocess.Popen | None = None
        if open_web_browser:
            browser_proc = open_browser_thread(Path(args.directory) / "fastled_js")
        else:
            print("\nCompilation successful.")
            if compile_server:
                print("Shutting down compile server...")
                compile_server.stop()
            return 0

        if args.just_compile:
            if compile_server:
                compile_server.stop()
            if browser_proc:
                browser_proc.kill()
            return 0 if result.success else 1
    except KeyboardInterrupt:
        print("\nExiting from main")
        if compile_server:
            compile_server.stop()
        return 1

    print("\nWatching for changes. Press Ctrl+C to stop...")
    sketch_filewatcher = FileWatcherProcess(
        args.directory, excluded_patterns=["fastled_js"]
    )

    source_code_watcher: FileWatcherProcess | None = None
    if compile_server and compile_server.using_fastled_src_dir_volume():
        assert compile_server.fastled_src_dir is not None
        source_code_watcher = FileWatcherProcess(
            compile_server.fastled_src_dir, excluded_patterns=[]
        )

    def trigger_rebuild_if_sketch_changed(
        last_compiled_result: CompiledResult,
    ) -> CompiledResult:
        changed_files = sketch_filewatcher.get_all_changes()
        if changed_files:
            print(f"\nChanges detected in {changed_files}")
            last_hash_value = last_compiled_result.hash_value
            out = compile_function(last_hash_value=last_hash_value)
            if not out.success:
                print("\nRecompilation failed.")
            else:
                print("\nRecompilation successful.")
            return out
        return last_compiled_result

    try:
        while True:
            last_compiled_result = trigger_rebuild_if_sketch_changed(
                last_compiled_result
            )
            if compile_server and not compile_server.proceess_running():
                print("Server process is not running. Exiting...")
                return 1
            if source_code_watcher is not None:
                changed_files = source_code_watcher.get_all_changes()
                # de-duplicate changes
                changed_files = sorted(list(set(changed_files)))
                if changed_files:
                    print(f"\nChanges detected in FastLED source code: {changed_files}")
                    print("Press space bar to trigger compile.")

                    space_key_watcher = SpaceBarWatcher()
                    try:
                        while True:
                            if space_key_watcher.space_bar_pressed():
                                print("Space bar pressed, triggering recompile...")
                                last_compiled_result = compile_function(
                                    last_hash_value=None
                                )
                                print("Finished recompile.")
                                break
                            elif len(sketch_filewatcher.get_all_changes()) > 0:
                                last_compiled_result = compile_function(
                                    last_hash_value=None
                                )
                                break
                            time.sleep(0.1)
                    finally:
                        space_key_watcher.stop()

    except KeyboardInterrupt:
        print("\nStopping watch mode...")
        return 0
    except Exception as e:
        print(f"Error: {e}")
        return 1
    finally:
        sketch_filewatcher.stop()
        if compile_server:
            compile_server.stop()
        if browser_proc:
            browser_proc.kill()


def run_server(args: argparse.Namespace) -> int:
    interactive = args.interactive
    auto_update = args.auto_update
    compile_server = CompileServer(interactive=interactive, auto_updates=auto_update)
    if not interactive:
        print(f"Server started at {compile_server.url()}")
    compile_server.wait_for_startup()
    try:
        while True:
            if not compile_server.proceess_running():
                print("Server process is not running. Exiting...")
                return 1
            time.sleep(1)
    except KeyboardInterrupt:
        print("\nExiting from server...")
        return 1
    finally:
        compile_server.stop()
    return 0


def main() -> int:
    args = parse_args()
    if args.server:
        print("Running in server only mode.")
        return run_server(args)
    else:
        print("Running in client/server mode.")
        return run_client(args)


if __name__ == "__main__":
    try:
        os.chdir("../fastled")
        sys.argv.append("--server")
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nExiting from main...")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
