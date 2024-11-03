import os
import shutil
import subprocess
import sys

import download  # type: ignore

from fastled_wasm.paths import PROJECT_ROOT

BUILD_FULL_EXE = False

DOCKER_USERNAME = "niteris"
IMAGE_NAME = "fastled-wasm"
TAG = "main"


def get_image_name() -> str:
    return f"{DOCKER_USERNAME}/{IMAGE_NAME}:{TAG}"


def _get_download_link(platform: str) -> str:
    return f"https://github.com/rzane/docker2exe/releases/download/v0.2.1/docker2exe-{platform}-amd64"


def _move_files_to_dist(full: bool = False) -> None:
    suffix = "-full" if full else ""
    files = [
        ("fastled-darwin-amd64", f"fastled-wasm-darwin-amd64{suffix}"),
        ("fastled-darwin-arm64", f"fastled-wasm-darwin-arm64{suffix}"),
        ("fastled-linux-amd64", f"fastled-wasm-linux-amd64{suffix}"),
        ("fastled-windows-amd64", f"fastled-wasm-windows-amd64{suffix}.exe"),
    ]
    for src, dest in files:
        src_path = PROJECT_ROOT / "dist" / src
        dest_path = PROJECT_ROOT / "dist" / dest
        if not os.path.exists(src_path):
            continue
        if dest_path.exists():
            dest_path.unlink()
        if src_path.exists():
            print(f"Moving {src_path} -> {dest_path}")
            shutil.move(str(src_path), str(dest_path))
        else:
            print(f"Warning: {src_path} does not exist and will not be moved.")
            print(f"Warning: {src_path} was expected to exist but does not.")


def setup_docker2exe(arch: str) -> None:
    image_name = get_image_name()
    platform = ""
    if sys.platform == "win32":
        platform = "windows"
    elif sys.platform == "darwin":
        platform = "darwin"
    elif sys.platform == "linux":
        platform = "linux"

    cache_dir = PROJECT_ROOT / "cache"
    cache_dir.mkdir(exist_ok=True)

    platforms = [
        "windows",
        "linux",
        "darwin",
    ]
    for platform in platforms:
        docker2exe_path = str(cache_dir / "docker2exe")
        if platform == "windows":
            docker2exe_path = docker2exe_path + ".exe"
        if not docker2exe_path.exists():
            download_link = _get_download_link(platform)
            download.download(
                download_link,
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
            image_name,
            "--module",
            "github.com/FastLED/FastLED",
            "--target",
            f"{platform}/{arch}",
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
