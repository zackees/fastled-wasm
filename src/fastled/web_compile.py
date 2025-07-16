import io
import os
import shutil
import tempfile
import time
import zipfile
from pathlib import Path

import httpx

from fastled.find_good_connection import find_good_connection
from fastled.settings import SERVER_PORT
from fastled.types import BuildMode, CompileResult
from fastled.zip_files import ZipResult, zip_files

DEFAULT_HOST = "https://fastled.onrender.com"
ENDPOINT_COMPILED_WASM = "compile/wasm"
_TIMEOUT = 60 * 4  # 2 mins timeout
_AUTH_TOKEN = "oBOT5jbsO4ztgrpNsQwlmFLIKB"


def _sanitize_host(host: str) -> str:
    if host.startswith("http"):
        return host
    is_local_host = "localhost" in host or "127.0.0.1" in host or "0.0.0.0" in host
    use_https = not is_local_host
    if use_https:
        return host if host.startswith("https://") else f"https://{host}"
    return host if host.startswith("http://") else f"http://{host}"


def _banner(msg: str) -> str:
    """
    Create a banner for the given message.
    Example:
    msg = "Hello, World!"
    print -> "#################"
             "# Hello, World! #"
             "#################"
    """
    lines = msg.split("\n")
    # Find the width of the widest line
    max_width = max(len(line) for line in lines)
    width = max_width + 4  # Add 4 for "# " and " #"

    # Create the top border
    banner = "\n" + "#" * width + "\n"

    # Add each line with proper padding
    for line in lines:
        padding = max_width - len(line)
        banner += f"# {line}{' ' * padding} #\n"

    # Add the bottom border
    banner += "#" * width + "\n"
    return f"\n{banner}\n"


def _print_banner(msg: str) -> None:
    print(_banner(msg))


def _compile_libfastled(
    host: str,
    auth_token: str,
    build_mode: BuildMode,
) -> httpx.Response:
    """Compile the FastLED library separately."""
    host = _sanitize_host(host)
    urls = [host]
    domain = host.split("://")[-1]
    if ":" not in domain:
        urls.append(f"{host}:{SERVER_PORT}")

    connection_result = find_good_connection(urls)
    if connection_result is None:
        raise ConnectionError(
            "Connection failed to all endpoints for libfastled compilation"
        )

    ipv4_stmt = "IPv4" if connection_result.ipv4 else "IPv6"
    transport = (
        httpx.HTTPTransport(local_address="0.0.0.0") if connection_result.ipv4 else None
    )

    with httpx.Client(
        transport=transport,
        timeout=_TIMEOUT * 2,  # Give more time for library compilation
    ) as client:
        headers = {
            "accept": "application/json",
            "authorization": auth_token,
            "build": build_mode.value.lower(),
        }

        url = f"{connection_result.host}/compile/libfastled"
        print(f"Compiling libfastled on {url} via {ipv4_stmt}")
        response = client.post(
            url,
            headers=headers,
            timeout=_TIMEOUT * 2,
        )

        return response


def _send_compile_request(
    host: str,
    zip_bytes: bytes,
    auth_token: str,
    build_mode: BuildMode,
    profile: bool,
    no_platformio: bool,
    allow_libcompile: bool,
) -> httpx.Response:
    """Send the compile request to the server and return the response."""
    host = _sanitize_host(host)
    urls = [host]
    domain = host.split("://")[-1]
    if ":" not in domain:
        urls.append(f"{host}:{SERVER_PORT}")

    connection_result = find_good_connection(urls)
    if connection_result is None:
        raise ConnectionError("Connection failed to all endpoints")

    ipv4_stmt = "IPv4" if connection_result.ipv4 else "IPv6"
    transport = (
        httpx.HTTPTransport(local_address="0.0.0.0") if connection_result.ipv4 else None
    )

    archive_size = len(zip_bytes)

    with httpx.Client(
        transport=transport,
        timeout=_TIMEOUT,
    ) as client:
        headers = {
            "accept": "application/json",
            "authorization": auth_token,
            "build": (
                build_mode.value.lower()
                if build_mode
                else BuildMode.QUICK.value.lower()
            ),
            "profile": "true" if profile else "false",
            "no-platformio": "true" if no_platformio else "false",
            "allow-libcompile": "false",  # Always false since we handle it manually
        }

        url = f"{connection_result.host}/{ENDPOINT_COMPILED_WASM}"
        print(
            f"Compiling sketch on {url} via {ipv4_stmt}. Zip size: {archive_size} bytes"
        )
        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        response = client.post(
            url,
            follow_redirects=True,
            files=files,
            headers=headers,
            timeout=_TIMEOUT,
        )

        return response


def _process_compile_response(
    response: httpx.Response,
    zip_result: ZipResult,
    start_time: float,
) -> CompileResult:
    """Process the compile response and return the final result."""
    if response.status_code != 200:
        json_response = response.json()
        detail = json_response.get("detail", "Could not compile")
        return CompileResult(
            success=False, stdout=detail, hash_value=None, zip_bytes=b""
        )

    print(f"Response status code: {response}")
    # Create a temporary directory to extract the zip
    with tempfile.TemporaryDirectory() as extract_dir:
        extract_path = Path(extract_dir)

        # Write the response content to a temporary zip file
        temp_zip = extract_path / "response.zip"
        temp_zip.write_bytes(response.content)

        # Extract the zip
        shutil.unpack_archive(temp_zip, extract_path, "zip")

        if zip_result.zip_embedded_bytes:
            # extract the embedded bytes, which were not sent to the server
            temp_zip.write_bytes(zip_result.zip_embedded_bytes)
            shutil.unpack_archive(temp_zip, extract_path, "zip")

        # we don't need the temp zip anymore
        temp_zip.unlink()

        # Read stdout from out.txt if it exists
        stdout_file = extract_path / "out.txt"
        hash_file = extract_path / "hash.txt"
        stdout = (
            stdout_file.read_text(encoding="utf-8", errors="replace")
            if stdout_file.exists()
            else ""
        )
        hash_value = (
            hash_file.read_text(encoding="utf-8", errors="replace")
            if hash_file.exists()
            else None
        )

        # now rezip the extracted files since we added the embedded json files
        out_buffer = io.BytesIO()
        with zipfile.ZipFile(
            out_buffer, "w", zipfile.ZIP_DEFLATED, compresslevel=9
        ) as out_zip:
            for root, _, _files in os.walk(extract_path):
                for file in _files:
                    file_path = Path(root) / file
                    relative_path = file_path.relative_to(extract_path)
                    out_zip.write(file_path, relative_path)

        diff_time = time.time() - start_time
        msg = f"Compilation success, took {diff_time:.2f} seconds"
        _print_banner(msg)
        return CompileResult(
            success=True,
            stdout=stdout,
            hash_value=hash_value,
            zip_bytes=out_buffer.getvalue(),
        )


def web_compile(
    directory: Path | str,
    host: str | None = None,
    auth_token: str | None = None,
    build_mode: BuildMode | None = None,
    profile: bool = False,
    no_platformio: bool = False,
    allow_libcompile: bool = True,
) -> CompileResult:
    start_time = time.time()
    if isinstance(directory, str):
        directory = Path(directory)
    host = _sanitize_host(host or DEFAULT_HOST)
    build_mode = build_mode or BuildMode.QUICK
    _print_banner(f"Compiling on {host}")
    auth_token = auth_token or _AUTH_TOKEN
    if not directory.exists():
        raise FileNotFoundError(f"Directory not found: {directory}")
    zip_result: ZipResult | Exception = zip_files(directory, build_mode=build_mode)
    if isinstance(zip_result, Exception):
        return CompileResult(
            success=False, stdout=str(zip_result), hash_value=None, zip_bytes=b""
        )
    zip_bytes = zip_result.zip_bytes
    print(f"Web compiling on {host}...")
    try:
        # Step 1: Compile libfastled if requested
        if allow_libcompile:
            print("Step 1: Compiling libfastled...")
            try:
                libfastled_response = _compile_libfastled(host, auth_token, build_mode)
                if libfastled_response.status_code != 200:
                    print(
                        f"Warning: libfastled compilation failed with status {libfastled_response.status_code}"
                    )
                    # Continue with sketch compilation even if libfastled fails
                else:
                    print("✅ libfastled compilation successful")
            except Exception as e:
                print(f"Warning: libfastled compilation failed: {e}")
                # Continue with sketch compilation even if libfastled fails
        else:
            print("Step 1 (skipped): Compiling libfastled")

        # Step 2: Compile the sketch
        print("Step 2: Compiling sketch...")
        response = _send_compile_request(
            host,
            zip_bytes,
            auth_token,
            build_mode,
            profile,
            no_platformio,
            False,  # allow_libcompile is always False since we handle it manually
        )

        return _process_compile_response(response, zip_result, start_time)

    except ConnectionError as e:
        _print_banner(str(e))
        return CompileResult(
            success=False,
            stdout=str(e),
            hash_value=None,
            zip_bytes=b"",
        )
    except KeyboardInterrupt:
        print("Keyboard interrupt")
        raise
    except httpx.HTTPError as e:
        print(f"Error: {e}")
        return CompileResult(
            success=False, stdout=str(e), hash_value=None, zip_bytes=b""
        )
