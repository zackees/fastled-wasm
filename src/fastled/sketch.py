import os
from pathlib import Path

_MAX_FILES_SEARCH_LIMIT = 10000


def find_sketch_directories(directory: Path) -> list[Path]:
    file_count = 0
    sketch_directories: list[Path] = []
    # search all the paths one level deep
    for path in directory.iterdir():
        if path.is_dir():
            dir_name = path.name
            if str(dir_name).startswith("."):
                continue
            file_count += 1
            if file_count > _MAX_FILES_SEARCH_LIMIT:
                print(
                    f"More than {_MAX_FILES_SEARCH_LIMIT} files found. Stopping search."
                )
                break

            if looks_like_sketch_directory(path, quick=True):
                sketch_directories.append(path)
            if dir_name.lower() == "examples":
                for example in path.iterdir():
                    if example.is_dir():
                        if looks_like_sketch_directory(example, quick=True):
                            sketch_directories.append(example)
    # make relative to cwd
    sketch_directories = [p.relative_to(directory) for p in sketch_directories]
    return sketch_directories


def get_sketch_files(directory: Path) -> list[Path]:
    file_count = 0
    files: list[Path] = []
    for root, dirs, filenames in os.walk(directory):
        # ignore hidden directories
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        # ignore fastled_js directory
        dirs[:] = [d for d in dirs if "fastled_js" not in d]
        # ignore hidden files
        filenames = [f for f in filenames if not f.startswith(".")]
        outer_break = False
        for filename in filenames:
            if "platformio.ini" in filename:
                continue
            file_count += 1
            if file_count > _MAX_FILES_SEARCH_LIMIT:
                print(
                    f"More than {_MAX_FILES_SEARCH_LIMIT} files found. Stopping search."
                )
                outer_break = True
                break
            files.append(Path(root) / filename)
        if outer_break:
            break

    return files


def looks_like_fastled_repo(directory: Path) -> bool:
    libprops = directory / "library.properties"
    if not libprops.exists():
        return False
    txt = libprops.read_text(encoding="utf-8", errors="ignore")
    return "FastLED" in txt


def _lots_and_lots_of_files(directory: Path) -> bool:
    return len(get_sketch_files(directory)) > 100


def looks_like_sketch_directory(directory: Path, quick=False) -> bool:
    if looks_like_fastled_repo(directory):
        print("Directory looks like the FastLED repo")
        return False

    if not quick:
        if _lots_and_lots_of_files(directory):
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
