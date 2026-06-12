from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from setuptools import Distribution, setup
from wheel.bdist_wheel import bdist_wheel as _bdist_wheel

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

ROOT = Path(__file__).resolve().parent
PACKAGE_BIN_DIR = ROOT / "src" / "fastled" / "bin"
EXE_NAME = "fastled.exe" if sys.platform == "win32" else "fastled"


def _cargo_command() -> list[str]:
    soldr = shutil.which("soldr")
    if soldr:
        return [soldr, "cargo"]
    raise RuntimeError("soldr was not found; cannot build bundled fastled")


def _workspace_package_version() -> str:
    cargo_toml = ROOT / "Cargo.toml"
    with cargo_toml.open("rb") as handle:
        manifest: dict[str, Any] = tomllib.load(handle)
    try:
        version = manifest["workspace"]["package"]["version"]
    except KeyError as exc:
        raise RuntimeError("Cargo.toml is missing workspace.package.version") from exc
    if not isinstance(version, str) or not version:
        raise RuntimeError("Cargo.toml workspace.package.version must be a string")
    return version


def _candidate_binaries(root: Path) -> list[Path]:
    candidates: list[Path] = []
    target = os.environ.get("CARGO_BUILD_TARGET") or os.environ.get(
        "FASTLED_RUST_TARGET"
    )
    if target:
        candidates.append(root / "target" / target / "release" / EXE_NAME)
    if sys.platform == "win32":
        candidates.extend(
            [
                root / "target" / "x86_64-pc-windows-msvc" / "release" / EXE_NAME,
                root / "target" / "aarch64-pc-windows-msvc" / "release" / EXE_NAME,
            ]
        )
    candidates.append(root / "target" / "release" / EXE_NAME)
    return candidates


def _stale_bundled_binary_paths(root: Path) -> list[Path]:
    paths = [root / "src" / "fastled" / "bin" / EXE_NAME]
    build_dir = root / "build"
    if build_dir.is_dir():
        paths.extend(build_dir.glob(f"lib*/fastled/bin/{EXE_NAME}"))
    return paths


def _remove_stale_bundled_binaries(root: Path) -> None:
    for path in _stale_bundled_binary_paths(root):
        if path.is_file():
            path.unlink()


def _copy_first_available_binary(root: Path) -> bool:
    bin_dir = root / "src" / "fastled" / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    dest = bin_dir / EXE_NAME
    for candidate in _candidate_binaries(root):
        if candidate.is_file():
            shutil.copy2(candidate, dest)
            # copy2 preserves the source mtime; bump it so setuptools'
            # mtime-based "newer" checks always replace stale build/lib copies.
            os.utime(dest)
            return True
    return False


def ensure_bundled_fastled_binary(root: Path = ROOT) -> None:
    dest = root / "src" / "fastled" / "bin" / EXE_NAME

    if (root / "Cargo.toml").is_file():
        # Source build: a pre-existing binary may predate the current source
        # (issue #165 shipped a broken viewer for days this way), so always
        # rebuild and never harvest stale copies from the tree or elsewhere.
        _remove_stale_bundled_binaries(root)
        command = [*_cargo_command(), "build", "--release", "--bin", "fastled"]
        target = os.environ.get("FASTLED_RUST_TARGET")
        if target:
            command.extend(["--target", target])
        subprocess.check_call(command, cwd=root)
        if not _copy_first_available_binary(root):
            raise RuntimeError("built fastled binary was not found")
        return

    if dest.is_file():
        # Building from an archive that already bundles the binary.
        return

    raise RuntimeError(
        "Cargo.toml was not found and no bundled fastled binary exists; "
        "cannot build fastled"
    )


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        return True


class bdist_wheel(_bdist_wheel):
    def run(self) -> None:
        ensure_bundled_fastled_binary()
        super().run()

    def get_tag(self) -> tuple[str, str, str]:
        _python, _abi, platform = super().get_tag()
        return "py3", "none", platform


if __name__ == "__main__":
    setup(
        version=_workspace_package_version(),
        distclass=BinaryDistribution,
        cmdclass={
            "bdist_wheel": bdist_wheel,
        },
    )
