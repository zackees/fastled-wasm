"""Locate the Rust ``fastled-rs`` binary.

In wheel-installed deployments the Rust binary is bundled directly into the
venv's ``Scripts/`` / ``bin/`` directory via ``[tool.maturin] data`` (see
``pyproject.toml``), so ``shutil.which(\"fastled-rs\")`` returns it. In editable
dev installs the Rust binary lives under ``target/`` from a local
``cargo build --bin fastled-rs``.

Search order:
    1. Workspace ``target/{release,debug}/fastled-rs[.exe]`` (dev / editable).
    2. ``$CARGO_HOME/bin/fastled-rs[.exe]`` (where ``cargo binstall`` installs).
    3. ``shutil.which(\"fastled-rs\")`` (wheel install puts it on ``PATH``).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path


def _exe_name() -> str:
    return "fastled-rs.exe" if sys.platform == "win32" else "fastled-rs"


def _find_workspace_root() -> Path | None:
    current = Path(__file__).resolve().parent
    for _ in range(10):
        if (current / "Cargo.toml").is_file():
            return current
        parent = current.parent
        if parent == current:
            break
        current = parent
    return None


def find_rust_fastled_cli() -> Path | None:
    """Return the path to the Rust ``fastled-rs`` binary, or ``None``."""
    exe = _exe_name()

    # 1. Workspace target/ tree (dev / editable).
    workspace_root = _find_workspace_root()
    if workspace_root is not None:
        for profile in ("release", "debug"):
            candidate = workspace_root / "target" / profile / exe
            if candidate.is_file():
                return candidate
        target_dir = workspace_root / "target"
        if target_dir.is_dir():
            for arch_dir in target_dir.iterdir():
                if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                    for profile in ("release", "debug"):
                        candidate = arch_dir / profile / exe
                        if candidate.is_file():
                            return candidate

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
    # Scripts/bin dir under the distinct fastled-rs name, so a Python
    # `fastled` compatibility shim cannot resolve itself here.
    found = shutil.which(exe)
    if found:
        return Path(found)
    return None


def invoke_rust_fastled_cli(argv: list[str] | None = None) -> int:
    """Run the Rust FastLED CLI and return its exit code."""
    args = list(argv or [])
    env = os.environ.copy()
    env.setdefault("FASTLED_PYTHON_EXECUTABLE", sys.executable)

    cli = find_rust_fastled_cli()
    if cli is not None:
        return subprocess.run([str(cli), *args], check=False, env=env).returncode

    workspace_root = _find_workspace_root()
    if workspace_root is not None:
        soldr = shutil.which("soldr")
        cargo = [soldr, "cargo"] if soldr else ["cargo"]
        return subprocess.run(
            [*cargo, "run", "--quiet", "--bin", "fastled-rs", "--", *args],
            check=False,
            cwd=workspace_root,
            env=env,
        ).returncode

    raise RuntimeError("Could not locate the Rust fastled-rs CLI binary.")
