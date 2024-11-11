"""
curl -X 'POST' \
  'https://fastled.onrender.com/compile/' \
  -H 'accept: application/json' \
  -H 'Content-Type: multipart/form-data' \
  -F 'file=@wasm.zip;type=application/x-zip-compressed'
"""

import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

import httpx

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


def web_compile(
    directory: Path, host: str | None = None, auth_token: str | None = None
) -> WebCompileResult:
    host = host or DEFAULT_HOST
    auth_token = auth_token or _AUTH_TOKEN
    # zip up the files
    print("Zipping files...")

    # Create a temporary zip file
    with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as tmp_zip:
        # Create temporary directory for organizing files
        with tempfile.TemporaryDirectory() as temp_dir:
            # Create wasm subdirectory
            wasm_dir = Path(temp_dir) / "wasm"
            # Copy all files from source to wasm subdirectory
            shutil.copytree(directory, wasm_dir)
            # Create zip archive from the temp directory
            shutil.make_archive(tmp_zip.name[:-4], "zip", temp_dir)

    print(f"Uploading to {host}...")

    try:
        with open(tmp_zip.name, "rb") as zip_file:
            files = {"file": ("wasm.zip", zip_file, "application/x-zip-compressed")}

            with httpx.Client(
                transport=httpx.HTTPTransport(local_address="0.0.0.0"),  # forces IPv4
                timeout=_TIMEOUT,  # 60 seconds timeout
            ) as client:
                response = client.post(
                    f"{host}/{ENDPOINT_COMPILED_WASM}",
                    files=files,
                    headers={"accept": "application/json"},
                )

                response.raise_for_status()

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
