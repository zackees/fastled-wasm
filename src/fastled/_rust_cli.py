"""Locate the Rust ``fastled`` binary, NOT the Python entry-point shim.

The Python package registers ``fastled`` as a console-script in
``[project.scripts]``, which lands at ``.venv/Scripts/fastled.exe`` (or
``.venv/bin/fastled``) and shadows the Rust binary on ``PATH``. The Rust
binary needs to be invoked for the internal plumbing flags
(``--internal-ensure-fastled-repo`` etc.); the shim's argparse rejects them.

Search order:
    1. Workspace ``target/{release,debug}/fastled[.exe]`` (dev builds).
    2. ``$CARGO_HOME/bin/fastled[.exe]`` (where ``cargo binstall`` installs).
    3. PATH walk that skips Python venv / system-Python directories.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path


def _exe_name() -> str:
    return "fastled.exe" if sys.platform == "win32" else "fastled"


def _looks_like_python_dir(dir_path: Path) -> bool:
    """Return True if ``dir_path`` hosts a Python interpreter.

    Used to filter out Python venv/install dirs from the PATH walk so that a
    ``fastled`` console-script entry-point in the same directory does not
    shadow the actual Rust binary.
    """
    if sys.platform == "win32":
        return (dir_path / "python.exe").is_file()
    return (dir_path / "python").is_file() or (dir_path / "python3").is_file()


def find_rust_fastled_cli() -> Path | None:
    """Return the path to the Rust ``fastled`` binary, or ``None``."""
    exe = _exe_name()

    # 1. Workspace target/ tree (dev builds).
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

    # 3. PATH walk, skipping Python interpreter dirs (which contain the
    # Python entry-point shim that shadows the Rust binary).
    for path_entry in os.environ.get("PATH", "").split(os.pathsep):
        if not path_entry:
            continue
        path_dir = Path(path_entry)
        candidate = path_dir / exe
        if not candidate.is_file():
            continue
        if _looks_like_python_dir(path_dir):
            continue
        return candidate

    return None
