from __future__ import annotations

import hashlib
import platform
import shutil
import stat
import subprocess
import sys
import tarfile
from pathlib import Path

import fasteners
import httpx

ESBUILD_VERSION = "0.28.0"
INSTALL_ROOT = Path.home() / ".fastled" / "toolchains" / "esbuild"
ARCHIVE_CACHE_DIR = Path.home() / ".fastled" / "toolchains" / "archives"
FRONTEND_DIR = Path(__file__).resolve().parent / "frontend"


def _platform_arch() -> tuple[str, str]:
    if sys.platform == "win32":
        plat = "win32"
    elif sys.platform == "darwin":
        plat = "darwin"
    else:
        plat = "linux"

    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        arch = "x64"
    elif machine in ("arm64", "aarch64"):
        arch = "arm64"
    else:
        raise RuntimeError(f"Unsupported architecture for esbuild: {machine}")
    return plat, arch


def _package_name(platform: str, arch: str) -> str:
    return f"@esbuild/{platform}-{arch}"


def _tarball_url(platform: str, arch: str) -> str:
    package_name = _package_name(platform, arch)
    package_tail = package_name.split("/", 1)[1]
    return f"https://registry.npmjs.org/{package_name}/-/{package_tail}-{ESBUILD_VERSION}.tgz"


def install_esbuild(force: bool = False) -> Path:
    platform, arch = _platform_arch()
    install_dir = INSTALL_ROOT / platform / arch / ESBUILD_VERSION
    install_dir.mkdir(parents=True, exist_ok=True)
    lock = fasteners.InterProcessLock(str(install_dir / ".install.lock"))

    with lock:
        exe_name = "esbuild.exe" if platform == "win32" else "esbuild"
        esbuild_path = install_dir / exe_name
        done_file = install_dir / "done.txt"
        if done_file.exists() and esbuild_path.exists() and not force:
            return esbuild_path

        ARCHIVE_CACHE_DIR.mkdir(parents=True, exist_ok=True)
        archive_path = (
            ARCHIVE_CACHE_DIR / f"esbuild-{platform}-{arch}-{ESBUILD_VERSION}.tgz"
        )
        if force and archive_path.exists():
            archive_path.unlink()

        if not archive_path.exists():
            response = httpx.get(
                _tarball_url(platform, arch), follow_redirects=True, timeout=120
            )
            response.raise_for_status()
            archive_path.write_bytes(response.content)

        if esbuild_path.exists():
            esbuild_path.unlink()

        with tarfile.open(archive_path, mode="r:gz") as tf:
            member_name = (
                f"package/{exe_name}" if platform == "win32" else "package/bin/esbuild"
            )
            member = tf.getmember(member_name)
            extracted = tf.extractfile(member)
            if extracted is None:
                raise RuntimeError(
                    f"Could not extract {member_name} from {archive_path}"
                )
            esbuild_path.write_bytes(extracted.read())

        if platform != "win32":
            current_mode = esbuild_path.stat().st_mode
            esbuild_path.chmod(
                current_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
            )

        done_file.write_text("ok\n", encoding="utf-8")
        return esbuild_path


def _copy_tree(src: Path, dst: Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)


def _copy_file(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)


def _get_source_mtime(source_dir: Path) -> float:
    max_mtime = 0.0
    skip_names = frozenset(("dist",))
    for path in source_dir.rglob("*"):
        if any(part in skip_names for part in path.parts):
            continue
        if path.is_file():
            max_mtime = max(max_mtime, path.stat().st_mtime)
    return max_mtime


def _compute_dir_hash(directory: Path) -> str:
    digest = hashlib.md5(usedforsecurity=False)
    for file_path in sorted(p for p in directory.rglob("*") if p.is_file()):
        digest.update(str(file_path.relative_to(directory)).encode("utf-8"))
        digest.update(file_path.read_bytes())
    return digest.hexdigest()


def _run_esbuild(source_dir: Path, args: list[str]) -> None:
    esbuild = install_esbuild()
    result = subprocess.run(
        [
            str(esbuild),
            "--alias:three=./vendor/three/build/three.module.js",
            *args,
        ],
        cwd=str(source_dir),
    )
    if result.returncode != 0:
        raise RuntimeError(f"esbuild failed with exit code {result.returncode}")


def _build_dist(source_dir: Path) -> Path:
    dist_dir = source_dir / "dist"
    marker = dist_dir / ".esbuild_marker"
    source_mtime = _get_source_mtime(source_dir)
    if dist_dir.exists() and marker.exists():
        try:
            if float(marker.read_text(encoding="utf-8").strip()) >= source_mtime:
                return dist_dir
        except ValueError:
            pass

    if dist_dir.exists():
        shutil.rmtree(dist_dir)
    dist_dir.mkdir(parents=True, exist_ok=True)

    _run_esbuild(
        source_dir,
        [
            str(source_dir / "app.ts"),
            "--bundle",
            "--format=esm",
            "--platform=browser",
            "--target=es2021",
            "--sourcemap",
            f"--outfile={source_dir / 'dist' / 'app.js'}",
            "--log-level=warning",
        ],
    )
    _run_esbuild(
        source_dir,
        [
            str(source_dir / "modules" / "core" / "fastled_background_worker.ts"),
            "--bundle",
            "--format=esm",
            "--platform=browser",
            "--target=es2021",
            "--sourcemap",
            f"--outfile={source_dir / 'dist' / 'fastled_background_worker.js'}",
            "--log-level=warning",
        ],
    )

    index_html = (
        (source_dir / "index.html")
        .read_text(encoding="utf-8")
        .replace("./app.ts", "./app.js")
    )
    (dist_dir / "index.html").write_text(index_html, encoding="utf-8")
    _copy_file(source_dir / "index.css", dist_dir / "index.css")
    _copy_file(
        source_dir / "modules" / "audio" / "audio_worklet_processor.js",
        dist_dir / "audio_worklet_processor.js",
    )

    assets_dir = source_dir / "assets"
    if assets_dir.exists():
        _copy_tree(assets_dir, dist_dir / "assets")

    marker.write_text(str(source_mtime), encoding="utf-8")
    return dist_dir


def copy_frontend_to_output(output_dir: Path, source_dir: Path | None = None) -> None:
    source = source_dir or FRONTEND_DIR
    dist_dir = _build_dist(source)

    hash_marker = output_dir / ".frontend_hash"
    current_hash = _compute_dir_hash(dist_dir)
    if (
        hash_marker.exists()
        and hash_marker.read_text(encoding="utf-8").strip() == current_hash
    ):
        print("  Frontend assets unchanged, skipping copy.")
        return

    for item in dist_dir.iterdir():
        destination = output_dir / item.name
        if item.is_dir():
            _copy_tree(item, destination)
        else:
            _copy_file(item, destination)

    hash_marker.write_text(current_hash, encoding="utf-8")
