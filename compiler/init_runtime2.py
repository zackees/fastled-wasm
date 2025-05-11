import glob
import os
import warnings
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

SRC_MAPPED_HOST_COMPLER_DIR = Path("/host/fastled/src/platforms/wasm/compiler")
SRC_STANDARD_HOST_COMPILER_DIR = Path(
    "/js/compiler/fastled/src/platforms/wasm/compiler"
)

COMPILER_TARGET = Path("/js")

if SRC_MAPPED_HOST_COMPLER_DIR.exists():
    SRC_COMPILER_DIR = SRC_MAPPED_HOST_COMPLER_DIR
else:
    SRC_COMPILER_DIR = SRC_STANDARD_HOST_COMPILER_DIR

HERE = Path(__file__).parent


def copy_task(src: str | Path) -> None:
    src = Path(src)
    if "entrypoint.sh" in str(src):
        return
    link_dst = COMPILER_TARGET / src.name

    # Handle shell scripts
    if src.suffix == ".sh":
        os.system(f"dos2unix {src} && chmod +x {src}")

    # if link exists, remove it
    if link_dst.exists():
        print(f"Removing existing link {link_dst}")
        try:
            os.remove(link_dst)
        except Exception as e:
            warnings.warn(f"Failed to remove {link_dst}: {e}")

    if not link_dst.exists():
        print(f"Linking {src} to {link_dst}")
        try:
            os.symlink(str(src), str(link_dst))
        except FileExistsError:
            print(f"Target {link_dst} already exists")
    else:
        print(f"Target {link_dst} already exists")


def make_links() -> None:
    # Define file patterns to include
    patterns = [
        "*.h",
        "*.hpp",
        "*.cpp",
        "*.py",
        "*.sh",
        "*.ino",
        "*.ini",
        "*.txt",
    ]

    # Get all matching files in compiler directory
    files = []
    for pattern in patterns:
        files.extend(glob.glob(str(SRC_COMPILER_DIR / pattern)))

    # Process files in parallel using ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=16) as executor:
        executor.map(copy_task, files)


def init_runtime() -> None:
    os.chdir(str(HERE))
    make_links()


if __name__ == "__main__":
    init_runtime()
