import glob
import os
import warnings
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

COMPILER_ROOT = Path("/js")
COMPILER_DIR = COMPILER_ROOT / "compiler"

SRC_MAPPED_HOST_COMPLER_DIR = Path("/host/fastled/src/platforms/wasm/compiler")
if SRC_MAPPED_HOST_COMPLER_DIR.exists():
    print(f"Using mapped host compiler directory: {SRC_MAPPED_HOST_COMPLER_DIR}")
    FASTLED_COMPILER_DIR = SRC_MAPPED_HOST_COMPLER_DIR
else:
    print(f"Using standard host compiler directory: {SRC_MAPPED_HOST_COMPLER_DIR}")
    FASTLED_COMPILER_DIR = COMPILER_ROOT / "fastled/src/platforms/wasm/compiler"


HERE = Path(__file__).parent


def symlink_task(src: str | Path, dst: Path) -> None:
    src = Path(src)
    # Handle shell scripts
    if src.suffix == ".sh":
        os.system(f"dos2unix {src} && chmod +x {src}")

    # if link exists, remove it
    if dst.exists():
        print(f"Removing existing link {dst}")
        try:
            os.remove(dst)
        except Exception as e:
            warnings.warn(f"Failed to remove {dst}: {e}")

    if not dst.exists():
        print(f"Linking {src} to {dst}")
        try:
            os.symlink(str(src), str(dst))
        except FileExistsError:
            print(f"Target {dst} already exists")
    else:
        print(f"Target {dst} already exists")


def _collect_docker_compile_files(globs: list[str]) -> list[tuple[Path, Path]]:
    """
    Collects files matching the given glob patterns from the Docker compile directory.
    """
    files = []
    for pattern in globs:
        for file_path in glob.glob(str(COMPILER_DIR / pattern)):
            src = Path(file_path)
            if "entrypoint.sh" in str(src):
                continue
            dst = COMPILER_ROOT / src.name
            files.append((src, dst))
    return files


def _collect_fastled_compile_files(globs: list[str]) -> list[tuple[Path, Path]]:
    """
    Collects files matching the given glob patterns from the FastLED compile directory.
    """
    files = []
    for pattern in globs:
        for file_path in glob.glob(str(FASTLED_COMPILER_DIR / pattern)):
            src = Path(file_path)
            if "entrypoint.sh" in str(src):
                continue
            dst = COMPILER_ROOT / src.name
            files.append((src, dst))
    return files


def make_links() -> None:
    # Define file patterns to include
    patterns = [
        "*.h",
        "*.py",
        "*.hpp",
        "*.cpp",
        "*.sh",
        "*.ino",
        "*.txt",
    ]

    # Get all matching files in compiler directory
    tasks = _collect_docker_compile_files(globs=patterns)
    tasks += _collect_fastled_compile_files(globs=patterns)
    tasks += [
        ("/platformio/wasm_compiler_flags.py", COMPILER_ROOT / "wasm_compiler_flags.py")
    ]
    tasks += [("/platformio/fastled.json", COMPILER_ROOT / "fastled.json")]

    # Process files in parallel using ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=16) as executor:

        def functor(args):
            src, dst = args
            symlink_task(src, dst)

        executor.map(functor, tasks)


def init_runtime() -> None:
    os.chdir(str(HERE))
    make_links()


if __name__ == "__main__":
    init_runtime()
