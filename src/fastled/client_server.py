import argparse
import shutil
import tempfile
import time
from multiprocessing import Process
from pathlib import Path

from fastled.build_mode import BuildMode, get_build_mode
from fastled.compile_server import CompileServer
from fastled.docker_manager import DockerManager
from fastled.env import DEFAULT_URL
from fastled.filewatcher import FileWatcherProcess
from fastled.keyboard import SpaceBarWatcher
from fastled.open_browser import open_browser_process
from fastled.sketch import looks_like_sketch_directory

# CompiledResult
from fastled.types import CompiledResult
from fastled.web_compile import (
    SERVER_PORT,
    ConnectionResult,
    find_good_connection,
    web_compile,
)


def _run_web_compiler(
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


def run_client_server(args: argparse.Namespace) -> int:
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
            return _run_web_compiler(
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

        browser_proc: Process | None = None
        if open_web_browser:
            browser_proc = open_browser_process(Path(args.directory) / "fastled_js")
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
    ) -> tuple[bool, CompiledResult]:
        changed_files = sketch_filewatcher.get_all_changes()
        if changed_files:
            print(f"\nChanges detected in {changed_files}")
            last_hash_value = last_compiled_result.hash_value
            out = compile_function(last_hash_value=last_hash_value)
            if not out.success:
                print("\nRecompilation failed.")
            else:
                print("\nRecompilation successful.")
            return True, out
        return False, last_compiled_result

    def print_status() -> None:
        print("Will compile on sketch changes or if you hit the space bar.")

    print_status()
    print("Press Ctrl+C to stop...")

    try:
        while True:
            if SpaceBarWatcher.watch_space_bar_pressed(timeout=1.0):
                print("Compiling...")
                last_compiled_result = compile_function(last_hash_value=None)
                if not last_compiled_result.success:
                    print("\nRecompilation failed.")
                else:
                    print("\nRecompilation successful.")
                # drain the space bar queue
                SpaceBarWatcher.watch_space_bar_pressed()
                print_status()
                continue
            changed, last_compiled_result = trigger_rebuild_if_sketch_changed(
                last_compiled_result
            )
            if changed:
                print_status()
                continue
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
                    while True:
                        space_bar_pressed = SpaceBarWatcher.watch_space_bar_pressed(
                            timeout=1.0
                        )
                        file_changes = source_code_watcher.get_all_changes()
                        sketch_files_changed = sketch_filewatcher.get_all_changes()

                        if file_changes:
                            print(
                                f"Changes detected in {file_changes}\nHit the space bar to trigger compile."
                            )

                        if space_bar_pressed or sketch_files_changed:
                            if space_bar_pressed:
                                print("Space bar pressed, triggering recompile...")
                            elif sketch_files_changed:
                                print(
                                    f"Changes detected in {','.join(sketch_files_changed)}, triggering recompile..."
                                )
                            last_compiled_result = compile_function(
                                last_hash_value=None
                            )
                            print("Finished recompile.")
                            # Drain the space bar queue
                            SpaceBarWatcher.watch_space_bar_pressed()
                            print_status()
                            continue

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
