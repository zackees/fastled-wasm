import os
from pathlib import Path


def get_sketch_files(directory: Path) -> list[Path]:
    files: list[Path] = []
    for root, dirs, filenames in os.walk(directory):
        # ignore hidden directories
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        # ignore fastled_js directory
        dirs[:] = [d for d in dirs if "fastled_js" not in d]
        # ignore hidden files
        filenames = [f for f in filenames if not f.startswith(".")]
        for filename in filenames:
            if "platformio.ini" in filename:
                continue
            files.append(Path(root) / filename)
    return files
