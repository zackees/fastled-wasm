from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
from pathlib import Path

FRONTEND_DIR = Path(__file__).resolve().parent / "frontend"


def _resolve_esbuild() -> Path:
    """Return the path to the esbuild binary.

    The Rust CLI installs esbuild via ``crates/fastled-cli/src/install.rs``
    and exports the path through ``FASTLED_ESBUILD_PATH``. If that env var
    is missing (e.g. Python invoked directly without the Rust pre-step), the
    function falls back to the binary on ``PATH``.
    """
    env_path = os.environ.get("FASTLED_ESBUILD_PATH")
    if env_path:
        candidate = Path(env_path)
        if candidate.is_file():
            return candidate
    located = shutil.which("esbuild")
    if located:
        return Path(located)
    raise RuntimeError(
        "esbuild binary not available. Ensure FASTLED_ESBUILD_PATH is set by "
        "the Rust CLI or that esbuild is on PATH."
    )


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
    esbuild = _resolve_esbuild()
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
