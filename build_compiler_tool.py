
import sys
from pathlib import Path
import os
import shutil
import subprocess

import download  # type: ignore
from build_docker_image import get_image_name

HERE = Path(__file__).resolve().parent

IMAGE_NAME = get_image_name()

BUILD_FULL_EXE = False

def _move_files_to_dist(full: bool = False) -> None:
    suffix = "-full" if full else ""
    files = [
        ("fastled-darwin-amd64", f"fastled-wasm-darwin-amd64{suffix}"),
        ("fastled-darwin-arm64", f"fastled-wasm-darwin-arm64{suffix}"),
        ("fastled-linux-amd64", f"fastled-wasm-linux-amd64{suffix}"),
        ("fastled-windows-amd64", f"fastled-wasm-windows-amd64{suffix}.exe"),
    ]
    for src, dest in files:
        src_path = HERE / "dist" / src
        dest_path = HERE / "dist" / dest
        if not os.path.exists(src_path):
            print(f"Skipping {src} as it does not exist.")
            continue
        if dest_path.exists():
            dest_path.unlink()
        if src_path.exists():
            print(f"Moving {src_path} -> {dest_path}")
            shutil.move(str(src_path), str(dest_path))
        else:
            print(f"Warning: {src_path} does not exist and will not be moved.")
            print(f"Warning: {src_path} was expected to exist but does not.")


def setup_docker2exe() -> None:
    platform = ""
    if sys.platform == "win32":
        platform = "windows"
    elif sys.platform == "darwin":
        platform = "darwin"
    elif sys.platform == "linux":
        platform = "linux"

    cache_dir = HERE / "cache"
    cache_dir.mkdir(exist_ok=True)

    docker2exe_path = cache_dir / "docker2exe.exe"
    if not docker2exe_path.exists():
        download.download(
            f"https://github.com/rzane/docker2exe/releases/download/v0.2.1/docker2exe-{platform}-amd64",
            str(docker2exe_path),
        )
        docker2exe_path.chmod(0o755)
    else:
        print("docker2exe.exe already exists, skipping download.")

    slim_cmd = [
        str(docker2exe_path),
        "--name",
        "fastled",
        "--image",
        IMAGE_NAME,
        "--module",
        "github.com/FastLED/FastLED",
        "--target",
        f"{platform}/amd64",
    ]
    full_cmd = slim_cmd + ["--embed"]

    print("Building wasm web command...")
    subprocess.run(
        slim_cmd,
        check=True,
    )
    _move_files_to_dist(full=False)

    if BUILD_FULL_EXE:
        print("Building wasm full command with no dependencies...")
        subprocess.run(
            full_cmd,
            check=True,
        )
        _move_files_to_dist(full=True)
    print("Docker2exe done.")


if __name__ == "__main__":
    setup_docker2exe()
