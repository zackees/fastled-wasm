import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

import httpx

from fastled_wasm.build_mode import BuildMode

DEFAULT_HOST = "https://fastled.onrender.com"
ENDPOINT_COMPILED_WASM = "compile/wasm"
_TIMEOUT = 60 * 4  # 2 mins timeout
_AUTH_TOKEN = "oBOT5jbsO4ztgrpNsQwlmFLIKB"


@dataclass
class WebCompileResult:
    success: bool
    stdout: str
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


_CONNECTION_ERROR_MAP: dict[str, bool] = {}


def web_compile(
    directory: Path,
    host: str | None = None,
    auth_token: str | None = None,
    build_mode: BuildMode | None = None,
    profile: bool = False,
) -> WebCompileResult:
    host = _sanitize_host(host or DEFAULT_HOST)
    auth_token = auth_token or _AUTH_TOKEN
    # zip up the files
    print("Zipping files...")

    # Create a temporary zip file
    with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as tmp_zip:
        # Create temporary directory for organizing files
        with tempfile.TemporaryDirectory() as temp_dir:
            # Create wasm subdirectory
            wasm_dir = Path(temp_dir) / "wasm"

            # Copy all files from source to wasm subdirectory, excluding fastled_js
            def ignore_fastled_js(dir: str, files: list[str]) -> list[str]:
                if "fastled_js" in dir:
                    return files
                if dir.startswith("."):
                    return files
                return []

            shutil.copytree(directory, wasm_dir, ignore=ignore_fastled_js)
            # Create zip archive from the temp directory
            shutil.make_archive(tmp_zip.name[:-4], "zip", temp_dir)

    print(f"Web compiling on {host}...")

    try:
        with open(tmp_zip.name, "rb") as zip_file:
            files = {"file": ("wasm.zip", zip_file, "application/x-zip-compressed")}

            tested = host in _CONNECTION_ERROR_MAP
            if not tested:
                test_url = f"{host}/healthz"
                print(f"Testing connection to {test_url}")
                timeout = 5
                with httpx.Client(
                    transport=httpx.HTTPTransport(local_address="0.0.0.0")
                ) as test_client:
                    test_response = test_client.get(test_url, timeout=timeout)
                    if test_response.status_code != 200:
                        print(f"Connection to {test_url} failed")
                        _CONNECTION_ERROR_MAP[host] = True
                        return WebCompileResult(
                            success=False, stdout="Connection failed", zip_bytes=b""
                        )
                    _CONNECTION_ERROR_MAP[host] = False

            ok = not _CONNECTION_ERROR_MAP[host]
            if not ok:
                return WebCompileResult(
                    success=False, stdout="Connection failed", zip_bytes=b""
                )

            with httpx.Client(
                transport=httpx.HTTPTransport(local_address="0.0.0.0"),  # forces IPv4
                timeout=_TIMEOUT,  # 60 seconds timeout
            ) as client:
                url = f"{host}/{ENDPOINT_COMPILED_WASM}"
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
                response = client.post(
                    url,
                    files=files,
                    headers=headers,
                )

                if response.status_code != 200:
                    print("Compilation failed:")
                    print(response.text)
                    json_response = response.json()
                    detail = json_response.get("detail", "Could not compile")
                    return WebCompileResult(success=False, stdout=detail, zip_bytes=b"")

                print(f"Response status code: {response}")
                # Create a temporary directory to extract the zip
                with tempfile.TemporaryDirectory() as extract_dir:
                    extract_path = Path(extract_dir)

                    # Write the response content to a temporary zip file
                    temp_zip = extract_path / "response.zip"
                    temp_zip.write_bytes(response.content)

                    # Extract the zip
                    shutil.unpack_archive(temp_zip, extract_path, "zip")

                    # Read stdout from out.txt if it exists
                    stdout_file = extract_path / "out.txt"
                    stdout = stdout_file.read_text() if stdout_file.exists() else ""

                    return WebCompileResult(
                        success=True, stdout=stdout, zip_bytes=response.content
                    )
    except httpx.HTTPError as e:
        print(f"Error: {e}")
        return WebCompileResult(success=False, stdout=str(e), zip_bytes=b"")
    finally:
        try:
            Path(tmp_zip.name).unlink()
        except PermissionError:
            print("Warning: Could not delete temporary zip file")
