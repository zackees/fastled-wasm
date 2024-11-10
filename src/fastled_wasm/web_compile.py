"""
curl -X 'POST' \
  'https://fastled.onrender.com/compile/' \
  -H 'accept: application/json' \
  -H 'Content-Type: multipart/form-data' \
  -F 'file=@wasm.zip;type=application/x-zip-compressed'
"""

import shutil
import tempfile
from pathlib import Path

import requests

DEFAULT_HOST = "https://fastled.onrender.com"
ENDPOINT_COMPILED_WASM = "compile/wasm"


def web_compile(directory: Path, host: str = DEFAULT_HOST) -> bytes:
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

            response = requests.post(
                f"{host}/{ENDPOINT_COMPILED_WASM}",
                files=files,
                headers={"accept": "application/json"},
            )

            response.raise_for_status()

            # Return the raw response content instead of parsing as JSON
            return response.content
    finally:
        try:
            Path(tmp_zip.name).unlink()
        except PermissionError:
            print("Warning: Could not delete temporary zip file")
