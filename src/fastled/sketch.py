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


def looks_like_fastled_repo(directory: Path) -> bool:
    libprops = directory / "library.properties"
    if not libprops.exists():
        return False
    txt = libprops.read_text(encoding="utf-8", errors="ignore")
    return "FastLED" in txt


def _lots_and_lots_of_files(directory: Path) -> bool:
    return len(get_sketch_files(directory)) > 100


def looks_like_sketch_directory(directory: Path) -> bool:
    if looks_like_fastled_repo(directory):
        print("Directory looks like the FastLED repo")
        return False

    if _lots_and_lots_of_files(directory):
        print("Too many files in the directory, bailing out")
        return False

    # walk the path and if there are over 30 files, return False
    # at the root of the directory there should either be an ino file or a src directory
    # or some cpp files
    # if there is a platformio.ini file, return True
    ino_file_at_root = list(directory.glob("*.ino"))
    if ino_file_at_root:
        return True
    cpp_file_at_root = list(directory.glob("*.cpp"))
    if cpp_file_at_root:
        return True
    platformini_file = list(directory.glob("platformio.ini"))
    if platformini_file:
        return True
    return False
