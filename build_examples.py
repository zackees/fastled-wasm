

import os
from shutil import which, copytree, rmtree
from pathlib import Path
import subprocess

HERE = Path(__file__).parent.resolve()
DOCS = HERE / "docs"

EXAMPLES = [
    "wasm",
    "Chromancer",
    "SdCard",
]

def _exec(cmd: str) -> None:
    subprocess.run(cmd, shell=True, check=True)

def build_example(example: str) -> None:
    src_dir = DOCS / example / "src"
    _exec(f"fastled --init={example} {src_dir}")
    assert src_dir.exists()
    _exec(f"fastled {src_dir} --just-compile")
    fastled_dir = src_dir / "fastled"
    assert fastled_dir.exists()
    # now copy it to the example dir
    example_dir = HERE / example
    copytree(fastled_dir, example_dir, dirs_exist_ok=True)
    # now remove the src dir
    rmtree(src_dir, ignore_errors=True)
    print(f"Built {example} example in {example_dir}")
    assert (example_dir / "fastled.wasm").exists()


def main() -> int:
    if not which("fastled"):
        raise FileNotFoundError("fastled executable not found")
    for example in EXAMPLES:
        build_example(example)
    return 0


if __name__ == "__main__":
    main()