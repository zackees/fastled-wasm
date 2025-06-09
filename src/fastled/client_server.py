import shutil
import tempfile
import threading
import time
import traceback
import warnings
from multiprocessing import Process
from pathlib import Path

from fastled.compile_server import CompileServer
from fastled.docker_manager import DockerManager
from fastled.filewatcher import DebouncedFileWatcherProcess, FileWatcherProcess
from fastled.keyboard import SpaceBarWatcher
from fastled.open_browser import spawn_http_server
from fastled.parse_args import Args
from fastled.settings import DEFAULT_URL, IMAGE_NAME
from fastled.sketch import looks_like_sketch_directory
from fastled.types import BuildMode, CompileResult, CompileServerError
from fastled.web_compile import (
    SERVER_PORT,
    ConnectionResult,
    find_good_connection,
    web_compile,
)


def _create_error_html(error_message: str) -> str:
    return f"""<!DOCTYPE html>
<html>
<head>
    <!-- no cache -->
    <meta http-equiv="Cache-Control" content="no-store" />
    <meta http-equiv="Pragma" content="no-cache" />
    <meta http-equiv="Expires" content="0" />
    <title>FastLED Compilation Error ZACH</title>
    <style>
        body {{
            background-color: #1a1a1a;
            color: #ffffff;
            font-family: Arial, sans-serif;
            margin: 20px;
            padding: 20px;
        }}
        pre {{
            color: #ffffff;
            background-color: #1a1a1a;
            border: 1px solid #444444;
            border-radius: 4px;
            padding: 15px;
            white-space: pre-wrap;
            word-wrap: break-word;
        }}
    </style>
</head>
<body>
    <h1>Compilation Failed</h1>
    <pre>{error_message}</pre>
</body>
</html>"""


# Override this function in your own code to run tests before compilation
def TEST_BEFORE_COMPILE(url) -> None:
    pass


def _chunked_print(stdout: str) -> None:
    lines = stdout.splitlines()
    for line in lines:
        print(line)


def _run_web_compiler(
    directory: Path,
    host: str,
    build_mode: BuildMode,
    profile: bool,
    last_hash_value: str | None,
) -> CompileResult:
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
        _chunked_print(web_result.stdout)
        # Create error page
        output_dir.mkdir(exist_ok=True)
        error_html = _create_error_html(web_result.stdout)
        (output_dir / "index.html").write_text(error_html, encoding="utf-8")
        return web_result

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
        return web_result

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

    _chunked_print(web_result.stdout)
    print_results()
    return web_result


def _try_start_server_or_get_url(
    auto_update: bool, args_web: str | bool, localhost: bool, clear: bool
) -> tuple[str, CompileServer | None]:
    is_local_host = localhost or (
        isinstance(args_web, str)
        and ("localhost" in args_web or "127.0.0.1" in args_web)
    )
    # test to see if there is already a local host server
    local_host_needs_server = False
    if is_local_host:
        addr = "localhost" if localhost or not isinstance(args_web, str) else args_web
        urls = [addr]
        if ":" not in addr:
            urls.append(f"{addr}:{SERVER_PORT}")

        result: ConnectionResult | None = find_good_connection(urls)
        if result is not None:
            print(f"Found local server at {result.host}")
            return (result.host, None)
        else:
            local_host_needs_server = True

    if not local_host_needs_server and args_web:
        if isinstance(args_web, str):
            return (args_web, None)
        if isinstance(args_web, bool):
            return (DEFAULT_URL, None)
        return (args_web, None)
    else:
        try:
            print("No local server found, starting one...")
            compile_server = CompileServer(
                auto_updates=auto_update, remove_previous=clear
            )
            print("Waiting for the local compiler to start...")
            if not compile_server.ping():
                print("Failed to start local compiler.")
                raise CompileServerError("Failed to start local compiler.")
            return (compile_server.url(), compile_server)
        except KeyboardInterrupt:
            raise
        except Exception as e:
            warnings.warn(
                f"Failed to start local compile server because of {e}, using web compiler instead."
            )
            return (DEFAULT_URL, None)


def _try_make_compile_server(clear: bool = False) -> CompileServer | None:
    if not DockerManager.is_docker_installed():
        return None
    try:
        print(
            "\nNo host specified, but Docker is installed, attempting to start a compile server using Docker."
        )
        from fastled.util import find_free_port

        free_port = find_free_port(start_port=9723, end_port=9743)
        if free_port is None:
            return None
        compile_server = CompileServer(auto_updates=False, remove_previous=clear)
        print("Waiting for the local compiler to start...")
        if not compile_server.ping():
            print("Failed to start local compiler.")
            raise CompileServerError("Failed to start local compiler.")
        return compile_server
    except KeyboardInterrupt:
        import _thread

        _thread.interrupt_main()
        raise
    except Exception as e:
        warnings.warn(f"Error starting local compile server: {e}")
        return None


def _is_local_host(host: str) -> bool:
    return (
        host.startswith("http://localhost")
        or host.startswith("http://127.0.0.1")
        or host.startswith("http://0.0.0.0")
        or host.startswith("http://[::]")
        or host.startswith("http://[::1]")
        or host.startswith("http://[::ffff:127.0.0.1]")
    )


def run_client(
    directory: Path,
    host: str | CompileServer | None,
    open_web_browser: bool = True,
    keep_running: bool = True,  # if false, only one compilation will be done.
    build_mode: BuildMode = BuildMode.QUICK,
    profile: bool = False,
    shutdown: threading.Event | None = None,
    http_port: (
        int | None
    ) = None,  # None means auto select a free port, http_port < 0 means no server.
    clear: bool = False,
) -> int:
    has_checked_newer_version_yet = False
    compile_server: CompileServer | None = None

    if host is None:
        # attempt to start a compile server if docker is installed.
        compile_server = _try_make_compile_server(clear=clear)
        if compile_server is None:
            host = DEFAULT_URL
    elif isinstance(host, CompileServer):
        # if the host is a compile server, use that
        compile_server = host

    shutdown = shutdown or threading.Event()

    def get_url(host=host, compile_server=compile_server) -> str:
        if compile_server is not None:
            return compile_server.url()
        if isinstance(host, str):
            return host
        return DEFAULT_URL

    url = get_url()
    is_local_host = _is_local_host(url)
    # parse out the port from the url
    # use a standard host address parser to grab it
    import urllib.parse

    parsed_url = urllib.parse.urlparse(url)
    if parsed_url.port is not None:
        port = parsed_url.port
    else:
        if is_local_host:
            raise ValueError(
                "Cannot use local host without a port. Please specify a port."
            )
        # Assume default port for www
        port = 80

    try:

        def compile_function(
            url: str = url,
            build_mode: BuildMode = build_mode,
            profile: bool = profile,
            last_hash_value: str | None = None,
        ) -> CompileResult:
            TEST_BEFORE_COMPILE(url)
            return _run_web_compiler(
                directory,
                host=url,
                build_mode=build_mode,
                profile=profile,
                last_hash_value=last_hash_value,
            )

        result: CompileResult = compile_function(last_hash_value=None)
        last_compiled_result: CompileResult = result

        if not result.success:
            print("\nCompilation failed.")

        use_http_server = http_port is None or http_port >= 0
        if not use_http_server and open_web_browser:
            warnings.warn(
                f"Warning: --http-port={http_port} specified but open_web_browser is False, ignoring --http-port."
            )
            use_http_server = False

        http_proc: Process | None = None
        if use_http_server:
            http_proc = spawn_http_server(
                directory / "fastled_js",
                port=http_port,
                compile_server_port=port,
                open_browser=open_web_browser,
            )
        else:
            print("\nCompilation successful.")
            if compile_server:
                print("Shutting down compile server...")
                compile_server.stop()
            return 0

        if not keep_running or shutdown.is_set():
            if http_proc:
                http_proc.kill()
            return 0 if result.success else 1
    except KeyboardInterrupt:
        print("\nExiting from main")
        return 1

    excluded_patterns = ["fastled_js"]
    debounced_sketch_watcher = DebouncedFileWatcherProcess(
        FileWatcherProcess(directory, excluded_patterns=excluded_patterns),
    )

    source_code_watcher: FileWatcherProcess | None = None
    if compile_server and compile_server.using_fastled_src_dir_volume():
        assert compile_server.fastled_src_dir is not None
        source_code_watcher = FileWatcherProcess(
            compile_server.fastled_src_dir, excluded_patterns=excluded_patterns
        )

    def trigger_rebuild_if_sketch_changed(
        last_compiled_result: CompileResult,
    ) -> tuple[bool, CompileResult]:
        changed_files = debounced_sketch_watcher.get_all_changes()
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
            if shutdown.is_set():
                print("\nStopping watch mode...")
                return 0

            # Check for newer Docker image version after first successful compilation
            if (
                not has_checked_newer_version_yet
                and last_compiled_result.success
                and is_local_host
            ):
                has_checked_newer_version_yet = True
                try:

                    docker_manager = DockerManager()
                    has_update, message = docker_manager.has_newer_version(
                        image_name=IMAGE_NAME, tag="latest"
                    )
                    if has_update:
                        print(f"\n🔄 {message}")
                        print(
                            "Run with `fastled -u` to update the docker image to the latest version."
                        )
                except Exception as e:
                    # Don't let Docker check failures interrupt the main flow
                    warnings.warn(f"Failed to check for Docker image updates: {e}")

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
            if compile_server and not compile_server.process_running():
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
                        sketch_files_changed = (
                            debounced_sketch_watcher.get_all_changes()
                        )

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
        debounced_sketch_watcher.stop()
        if compile_server:
            compile_server.stop()
        if http_proc:
            http_proc.kill()


def run_client_server(args: Args) -> int:
    profile = bool(args.profile)
    web: str | bool = args.web if isinstance(args.web, str) else bool(args.web)
    auto_update = bool(args.auto_update)
    localhost = bool(args.localhost)
    directory = args.directory if args.directory else Path(".")
    just_compile = bool(args.just_compile)
    interactive = bool(args.interactive)
    force_compile = bool(args.force_compile)
    open_web_browser = not just_compile and not interactive
    build_mode: BuildMode = BuildMode.from_args(args)

    if not force_compile and not looks_like_sketch_directory(directory):
        # if there is only one directory in the sketch directory, use that
        found_valid_child = False
        if len(list(directory.iterdir())) == 1:
            child_dir = next(directory.iterdir())
            if looks_like_sketch_directory(child_dir):
                found_valid_child = True
                print(
                    f"The selected directory is not a valid FastLED sketch directory, the child directory {child_dir} looks like a sketch directory, using that instead."
                )
                directory = child_dir
        if not found_valid_child:
            print(
                f"Error: {directory} is not a valid FastLED sketch directory, if you are sure it is, use --force-compile"
            )
        return 1

    # If not explicitly using web compiler, check Docker installation
    if not web and not DockerManager.is_docker_installed():
        print(
            "\nDocker is not installed on this system - switching to web compiler instead."
        )
        web = True

    url: str
    compile_server: CompileServer | None = None
    try:
        url, compile_server = _try_start_server_or_get_url(
            auto_update, web, localhost, args.clear
        )
    except KeyboardInterrupt:
        print("\nExiting from first try...")
        if compile_server is not None:
            compile_server.stop()
        return 1
    except Exception as e:
        stack_trace = e.__traceback__
        stack_trace_list: list[str] | None = (
            traceback.format_exception(type(e), e, stack_trace) if stack_trace else None
        )
        stack_trace_str = ""
        if stack_trace_list is not None:
            stack_trace_str = "".join(stack_trace_list) if stack_trace_list else ""
        if stack_trace:
            print(f"Error in starting compile server: {e}\n{stack_trace_str}")
        else:
            print(f"Error: {e}")
        if compile_server is not None:
            compile_server.stop()
        return 1

    try:
        return run_client(
            directory=directory,
            host=compile_server if compile_server else url,
            open_web_browser=open_web_browser,
            keep_running=not just_compile,
            build_mode=build_mode,
            profile=profile,
            clear=args.clear,
        )
    except KeyboardInterrupt:
        return 1
    finally:
        if compile_server:
            compile_server.stop()
