from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

from setuptools import Distribution, setup
from wheel.bdist_wheel import bdist_wheel as _bdist_wheel

ROOT = Path(__file__).resolve().parent
PACKAGE_BIN_DIR = ROOT / "src" / "fastled" / "bin"
EXE_NAME = "fastled-rs.exe" if sys.platform == "win32" else "fastled-rs"


def _cargo_command() -> list[str]:
    soldr = shutil.which("soldr")
    if soldr:
        return [soldr, "cargo"]
    raise RuntimeError("soldr was not found; cannot build bundled fastled-rs")


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
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    candidates.append(cargo_home / "bin" / EXE_NAME)
    return candidates


def _copy_first_available_binary() -> bool:
    PACKAGE_BIN_DIR.mkdir(parents=True, exist_ok=True)
    dest = PACKAGE_BIN_DIR / EXE_NAME
    for candidate in _candidate_binaries():
        if candidate.is_file():
            shutil.copy2(candidate, dest)
            return True
    return False


def ensure_bundled_fastled_binary() -> None:
    dest = PACKAGE_BIN_DIR / EXE_NAME
    if dest.is_file():
        return

    if _copy_first_available_binary():
        return

    if not (ROOT / "Cargo.toml").is_file():
        raise RuntimeError("Cargo.toml was not found; cannot build bundled fastled-rs")

    command = [*_cargo_command(), "build", "--release", "--bin", "fastled-rs"]
    target = os.environ.get("FASTLED_RUST_TARGET")
    if target:
        command.extend(["--target", target])
    subprocess.check_call(command, cwd=ROOT)
    if not _copy_first_available_binary():
        raise RuntimeError("built fastled-rs binary was not found")


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


setup(
    distclass=BinaryDistribution,
    cmdclass={
        "bdist_wheel": bdist_wheel,
    },
)
