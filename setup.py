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


def _candidate_binaries() -> list[Path]:
    candidates: list[Path] = []
    target = os.environ.get("CARGO_BUILD_TARGET") or os.environ.get(
        "FASTLED_RUST_TARGET"
    )
    if target:
        candidates.append(ROOT / "target" / target / "release" / EXE_NAME)
    if sys.platform == "win32":
        candidates.extend(
            [
                ROOT / "target" / "x86_64-pc-windows-msvc" / "release" / EXE_NAME,
                ROOT / "target" / "aarch64-pc-windows-msvc" / "release" / EXE_NAME,
            ]
        )
    candidates.append(ROOT / "target" / "release" / EXE_NAME)
    return candidates


def _copy_newest_available_binary() -> bool:
    PACKAGE_BIN_DIR.mkdir(parents=True, exist_ok=True)
    dest = PACKAGE_BIN_DIR / EXE_NAME
    existing = [candidate for candidate in _candidate_binaries() if candidate.is_file()]
    if not existing:
        return False
    newest = max(existing, key=lambda path: path.stat().st_mtime)
    shutil.copy2(newest, dest)
    return True


def _newest_rust_source_mtime() -> float:
    newest = 0.0
    for path in (ROOT / "Cargo.toml", ROOT / "Cargo.lock"):
        if path.is_file():
            newest = max(newest, path.stat().st_mtime)
    for path in (ROOT / "crates").rglob("*"):
        if path.is_file():
            newest = max(newest, path.stat().st_mtime)
    return newest


def ensure_bundled_fastled_binary() -> None:
    dest = PACKAGE_BIN_DIR / EXE_NAME

    if not (ROOT / "Cargo.toml").is_file():
        if dest.is_file():
            return
        raise RuntimeError(
            "Cargo.toml was not found and no staged fastled binary exists; "
            "cannot build bundled fastled"
        )

    # A staged binary at least as new as every Rust source is current. This
    # covers CI, which cross-compiles and stages the exe immediately before
    # building the wheel. Anything older is stale and must be rebuilt so
    # `pip install .` always bundles a binary matching the checkout.
    if dest.is_file() and dest.stat().st_mtime >= _newest_rust_source_mtime():
        return

    command = [*_cargo_command(), "build", "--release", "--bin", "fastled"]
    target = os.environ.get("FASTLED_RUST_TARGET")
    if target:
        command.extend(["--target", target])
    subprocess.check_call(command, cwd=ROOT)
    if not _copy_newest_available_binary():
        raise RuntimeError("built fastled binary was not found")


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        return True


class bdist_wheel(_bdist_wheel):
    def run(self) -> None:
        ensure_bundled_fastled_binary()
        super().run()

    def get_tag(self) -> tuple[str, str, str]:
        *_, platform = super().get_tag()
        return "py3", "none", platform


setup(
    version=_workspace_package_version(),
    distclass=BinaryDistribution,
    cmdclass={
        "bdist_wheel": bdist_wheel,
    },
)
