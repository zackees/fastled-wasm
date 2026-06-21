"""Locate the native Rust ``fastled`` binary.

Wheel-installed deployments bundle the Rust binary under ``fastled/bin``.
Editable dev installs usually resolve the binary from ``target/`` after a
local ``soldr cargo build --bin fastled``.

Search order:
    1. Package-local ``fastled/bin/fastled[.exe]`` (wheel install).
    2. Workspace ``target/{release,debug}/fastled[.exe]`` (dev / editable).
    3. ``$CARGO_HOME/bin/fastled[.exe]`` (where ``cargo-binstall`` installs).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path


def _exe_name() -> str:
    return "fastled.exe" if sys.platform == "win32" else "fastled"


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
    """Return the path to the Rust ``fastled`` binary, or ``None``."""
    exe = _exe_name()

    # 1. Wheel-bundled native binary.
    candidate = Path(__file__).resolve().parent / "bin" / exe
    if candidate.is_file():
        return candidate

    # 2. Workspace target/ tree (dev / editable).
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

    # 3. cargo-binstall install location.
    cargo_home = os.environ.get("CARGO_HOME")
    if cargo_home:
        candidate = Path(cargo_home) / "bin" / exe
        if candidate.is_file():
            return candidate
    candidate = Path.home() / ".cargo" / "bin" / exe
    if candidate.is_file():
        return candidate

    return None


def invoke_rust_fastled_cli(argv: list[str] | None = None) -> int:
    """Run the Rust FastLED CLI and return its exit code."""
    args = list(argv or [])
    env = os.environ.copy()
    env.setdefault("FASTLED_PYTHON_EXECUTABLE", sys.executable)

    # Set FASTLED_FRONTEND_DIR to the bundled frontend if not already set.
    if "FASTLED_FRONTEND_DIR" not in env:
        frontend_dir = Path(__file__).resolve().parent / "frontend"
        if frontend_dir.is_dir():
            env["FASTLED_FRONTEND_DIR"] = str(frontend_dir)

    cli = find_rust_fastled_cli()
    if cli is not None:
        return subprocess.run([str(cli), *args], check=False, env=env).returncode

    workspace_root = _find_workspace_root()
    if workspace_root is not None:
        soldr = shutil.which("soldr")
        if soldr is None:
            raise RuntimeError(
                "soldr is required to build the Rust fastled CLI binary."
            )
        return subprocess.run(
            [soldr, "cargo", "run", "--quiet", "--bin", "fastled", "--", *args],
            check=False,
            cwd=workspace_root,
            env=env,
        ).returncode

    raise RuntimeError("Could not locate the Rust fastled CLI binary.")
