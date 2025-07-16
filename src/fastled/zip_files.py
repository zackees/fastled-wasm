import io
import json
import zipfile
from dataclasses import dataclass
from pathlib import Path

from fastled.sketch import get_sketch_files
from fastled.types import BuildMode
from fastled.util import hash_file


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


def zip_files(directory: Path, build_mode: BuildMode) -> ZipResult | Exception:
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
                    if "fastled_js" in str(file_path):
                        # These can be huge, don't send the output files back to the server!
                        continue
                    relative_path = file_path.relative_to(directory)
                    achive_path = str(Path("wasm") / relative_path)
                    if str(relative_path).startswith("data"):
                        _file_info_str = _file_info(file_path)
                        zip_file.writestr(
                            achive_path + ".embedded.json", _file_info_str
                        )
                        emebedded_zip_file.write(file_path, relative_path)
                        has_embedded_zip = True
                    else:
                        zip_file.write(file_path, achive_path)
                # write build mode into the file as build.txt so that sketches are fingerprinted
                # based on the build mode. Otherwise the same sketch with different build modes
                # will have the same fingerprint.
                zip_file.writestr(
                    str(Path("wasm") / "build_mode.txt"), build_mode.value
                )
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
