"""Locate the Rust ``fastled`` binary.

In wheel-installed deployments the Rust binary is bundled directly into the
venv's ``Scripts/`` / ``bin/`` directory via ``[tool.maturin] data`` (see
``pyproject.toml``), so ``shutil.which(\"fastled\")`` returns it. In editable
dev installs the Rust binary lives under ``target/`` from a local
``cargo build --bin fastled``.

Search order:
    1. Workspace ``target/{release,debug}/fastled[.exe]`` (dev / editable).
    2. ``$CARGO_HOME/bin/fastled[.exe]`` (where ``cargo binstall`` installs).
    3. ``shutil.which(\"fastled\")`` (wheel install puts it on ``PATH``).
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path


def _exe_name() -> str:
    return "fastled.exe" if sys.platform == "win32" else "fastled"


def find_rust_fastled_cli() -> Path | None:
    """Return the path to the Rust ``fastled`` binary, or ``None``."""
    exe = _exe_name()

    # 1. Workspace target/ tree (dev / editable).
    current = Path(__file__).resolve().parent
    for _ in range(10):
        if (current / "Cargo.toml").is_file():
            for profile in ("release", "debug"):
                candidate = current / "target" / profile / exe
                if candidate.is_file():
                    return candidate
            target_dir = current / "target"
            if target_dir.is_dir():
                for arch_dir in target_dir.iterdir():
                    if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                        for profile in ("release", "debug"):
                            candidate = arch_dir / profile / exe
                            if candidate.is_file():
                                return candidate
            break
        parent = current.parent
        if parent == current:
            break
        current = parent

    # 2. cargo-binstall install location.
    cargo_home = os.environ.get("CARGO_HOME")
    if cargo_home:
        candidate = Path(cargo_home) / "bin" / exe
        if candidate.is_file():
            return candidate
    candidate = Path.home() / ".cargo" / "bin" / exe
    if candidate.is_file():
        return candidate

    # 3. Wheel install drops the Rust binary directly into the venv's
    # Scripts/bin dir; there is no Python `fastled` entry-point shim
    # competing for the name anymore (see pyproject.toml). Plain PATH lookup
    # finds it.
    found = shutil.which(exe)
    if found:
        return Path(found)
    return None
