
import os
from pathlib import Path
import glob
import warnings
from concurrent.futures import ThreadPoolExecutor

HERE = Path(__file__).parent


_COMPILER_DIR = Path("/js/compiler")
_FASTLED_SRC_DIR = Path("/js/fastled/src")
_WASM_DIR = _FASTLED_SRC_DIR / "platforms" / "wasm"
_FASTLED_COMPILER_DIR = _WASM_DIR / "compiler"

def copy_task(src: str | Path) -> None:
    src = Path(src)
    if "entrypoint.sh" in str(src):
        return
    link_dst = Path("/js") / src.name
    
    # Handle shell scripts
    if src.suffix == '.sh':
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
    patterns = ['*.h', '*.py', '*.css', '*.sh', "*.ino", "*.hpp", "*.cpp", "*.ini", "*.txt"]
    
    # Get all matching files in compiler directory
    files = []
    for pattern in patterns:
        files.extend(glob.glob(str(_COMPILER_DIR / pattern)))

    for pattern in patterns:
        files.extend(glob.glob(str(_FASTLED_COMPILER_DIR / pattern)))

    # Process files in parallel using ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=16) as executor:
        executor.map(copy_task, files)

def check() -> None:
    print("Checking...")
    if not Path("/js/Arduino.h").exists():
        raise RuntimeError("Arduino.h not found")

def init_runtime() -> None:
    os.chdir(str(HERE))
    make_links()
    try:
        check()
    except Exception:
        # print out the entire directory of /js, one level deep
        print("Directory listing:")
        for root, dirs, files in os.walk("/js"):
            for name in files:
                print(os.path.join(root, name))
        raise




if __name__ == "__main__":
    init_runtime()
