import io
import json
import os
import shutil
import tempfile
import zipfile
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path

import httpx

from fastled.build_mode import BuildMode
from fastled.compile_server import SERVER_PORT
from fastled.sketch import get_sketch_files
from fastled.util import hash_file

DEFAULT_HOST = "https://fastled.onrender.com"
ENDPOINT_COMPILED_WASM = "compile/wasm"
_TIMEOUT = 60 * 4  # 2 mins timeout
_AUTH_TOKEN = "oBOT5jbsO4ztgrpNsQwlmFLIKB"
ENABLE_EMBEDDED_DATA = True
_EXECUTOR = ProcessPoolExecutor(max_workers=8)


@dataclass
class ConnectionResult:
    host: str
    success: bool
    ipv4: bool


@dataclass
class WebCompileResult:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes

    def __bool__(self) -> bool:
        return self.success


def _sanitize_host(host: str) -> str:
    if host.startswith("http"):
        return host
    is_local_host = "localhost" in host or "127.0.0.1" in host or "0.0.0.0" in host
    use_https = not is_local_host
    if use_https:
        return host if host.startswith("https://") else f"https://{host}"
    return host if host.startswith("http://") else f"http://{host}"


def _test_connection(host: str, use_ipv4: bool) -> ConnectionResult:
    transport = httpx.HTTPTransport(local_address="0.0.0.0") if use_ipv4 else None
    try:
        with httpx.Client(
            timeout=_TIMEOUT,
            transport=transport,
        ) as test_client:
            test_response = test_client.get(
                f"{host}/healthz", timeout=3, follow_redirects=True
            )
            result = ConnectionResult(host, test_response.status_code == 200, use_ipv4)
    except Exception:
        result = ConnectionResult(host, False, use_ipv4)
    return result


def _file_info(file_path: Path) -> str:
    hash_txt = hash_file(file_path)
    file_size = file_path.stat().st_size
    json_str = json.dumps({"hash": hash_txt, "size": file_size})
    return json_str


@dataclass
class ZipResult:
    zip_bytes: bytes
    zip_embedded_bytes: bytes | None
    success: bool
    error: str | None


def zip_files(directory: Path) -> ZipResult | Exception:
    print("Zipping files...")
    try:
        files = get_sketch_files(directory)
        if not files:
            raise FileNotFoundError(f"No files found in {directory}")
        for f in files:
            print(f"Adding file: {f}")
        # Create in-memory zip file
        has_embedded_zip = False
        zip_embedded_buffer = io.BytesIO()
        zip_buffer = io.BytesIO()
        with zipfile.ZipFile(
            zip_embedded_buffer, "w", zipfile.ZIP_DEFLATED, compresslevel=9
        ) as emebedded_zip_file:
            with zipfile.ZipFile(
                zip_buffer, "w", zipfile.ZIP_DEFLATED, compresslevel=9
            ) as zip_file:
                for file_path in files:
                    relative_path = file_path.relative_to(directory)
                    achive_path = str(Path("wasm") / relative_path)
                    if str(relative_path).startswith("data") and ENABLE_EMBEDDED_DATA:
                        _file_info_str = _file_info(file_path)
                        zip_file.writestr(
                            achive_path + ".embedded.json", _file_info_str
                        )
                        emebedded_zip_file.write(file_path, relative_path)
                        has_embedded_zip = True
                    else:
                        zip_file.write(file_path, achive_path)
        result = ZipResult(
            zip_bytes=zip_buffer.getvalue(),
            zip_embedded_bytes=(
                zip_embedded_buffer.getvalue() if has_embedded_zip else None
            ),
            success=True,
            error=None,
        )
        return result
    except Exception as e:
        return e


def web_compile(
    directory: Path,
    host: str | None = None,
    auth_token: str | None = None,
    build_mode: BuildMode | None = None,
    profile: bool = False,
) -> WebCompileResult:
    host = _sanitize_host(host or DEFAULT_HOST)
    print("Compiling on", host)
    auth_token = auth_token or _AUTH_TOKEN

    if not directory.exists():
        raise FileNotFoundError(f"Directory not found: {directory}")

    zip_result = zip_files(directory)

    if isinstance(zip_result, Exception):
        return WebCompileResult(
            success=False, stdout=str(zip_result), hash_value=None, zip_bytes=b""
        )
    zip_bytes = zip_result.zip_bytes
    archive_size = len(zip_bytes)
    print(f"Web compiling on {host}...")
    try:

        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        urls = [host]
        domain = host.split("://")[-1]
        if ":" not in domain:
            urls.append(f"{host}:{SERVER_PORT}")
        test_connection_result: ConnectionResult | None = None

        futures: list = []
        ip_versions = [True, False] if "localhost" not in host else [True]
        for ipv4 in ip_versions:
            for url in urls:
                f = _EXECUTOR.submit(_test_connection, url, ipv4)
                futures.append(f)

        succeeded = False
        for future in as_completed(futures):
            result: ConnectionResult = future.result()

            if result.success:
                print(f"Connection successful to {result.host}")
                succeeded = True
                # host = test_url
                test_connection_result = result
                break
            else:
                print(f"Ignoring {result.host} due to connection failure")

        if not succeeded:
            print("Connection failed to all endpoints")
            return WebCompileResult(
                success=False,
                stdout="Connection failed",
                hash_value=None,
                zip_bytes=b"",
            )
        assert test_connection_result is not None
        ipv4_stmt = "IPv4" if test_connection_result.ipv4 else "IPv6"
        transport = (
            httpx.HTTPTransport(local_address="0.0.0.0")
            if test_connection_result.ipv4
            else None
        )
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
            }

            url = f"{test_connection_result.host}/{ENDPOINT_COMPILED_WASM}"
            print(f"Compiling on {url} via {ipv4_stmt}. Zip size: {archive_size} bytes")
            response = client.post(
                url,
                follow_redirects=True,
                files=files,
                headers=headers,
                timeout=_TIMEOUT,
            )

            if response.status_code != 200:
                json_response = response.json()
                detail = json_response.get("detail", "Could not compile")
                return WebCompileResult(
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
                stdout = stdout_file.read_text() if stdout_file.exists() else ""
                hash_value = hash_file.read_text() if hash_file.exists() else None

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

                return WebCompileResult(
                    success=True,
                    stdout=stdout,
                    hash_value=hash_value,
                    zip_bytes=out_buffer.getvalue(),
                )
    except KeyboardInterrupt:
        print("Keyboard interrupt")
        raise
    except httpx.HTTPError as e:
        print(f"Error: {e}")
        return WebCompileResult(
            success=False, stdout=str(e), hash_value=None, zip_bytes=b""
        )
