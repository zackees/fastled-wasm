"""Regression tests for setup.py's bundled-binary staleness handling (issue #165).

A stale pre-fix ``fastled.exe`` left in ``src/fastled/bin/`` (or harvested from
``~/.cargo/bin``) used to be bundled into every wheel without any freshness
check, so ``pip install .`` shipped broken binaries from fixed checkouts.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="module")
def setup_module():
    spec = importlib.util.spec_from_file_location(
        "fastled_setup", REPO_ROOT / "setup.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_rebuilds_even_when_bundled_binary_exists(setup_module, tmp_path, monkeypatch):
    """A pre-existing src/fastled/bin binary must never short-circuit the build."""
    mod = setup_module
    root = tmp_path
    (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
    bin_dir = root / "src" / "fastled" / "bin"
    bin_dir.mkdir(parents=True)
    stale = bin_dir / mod.EXE_NAME
    stale.write_bytes(b"STALE")
    build_copy = (
        root / "build" / "lib.win-amd64-cpython-313" / "fastled" / "bin" / mod.EXE_NAME
    )
    build_copy.parent.mkdir(parents=True)
    build_copy.write_bytes(b"STALE")

    calls: list[list[str]] = []

    def fake_check_call(cmd, cwd=None):
        assert cwd == root
        calls.append(list(cmd))
        out = root / "target" / "release" / mod.EXE_NAME
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(b"FRESH")

    monkeypatch.setattr(mod.subprocess, "check_call", fake_check_call)
    monkeypatch.setattr(mod, "_cargo_command", lambda: ["cargo"])
    monkeypatch.delenv("CARGO_BUILD_TARGET", raising=False)
    monkeypatch.delenv("FASTLED_RUST_TARGET", raising=False)

    mod.ensure_bundled_fastled_binary(root)

    assert calls, "cargo build must run even when a bundled binary already exists"
    assert stale.read_bytes() == b"FRESH"
    assert not build_copy.exists(), "stale build/lib copy must be purged"


def test_candidate_binaries_never_harvest_cargo_home(
    setup_module, tmp_path, monkeypatch
):
    """Binaries outside the workspace (e.g. ~/.cargo/bin) must not be harvested."""
    mod = setup_module
    cargo_home = tmp_path / "cargo_home"
    (cargo_home / "bin").mkdir(parents=True)
    (cargo_home / "bin" / mod.EXE_NAME).write_bytes(b"STALE")
    monkeypatch.setenv("CARGO_HOME", str(cargo_home))
    monkeypatch.delenv("CARGO_BUILD_TARGET", raising=False)
    monkeypatch.delenv("FASTLED_RUST_TARGET", raising=False)

    root = tmp_path / "workspace"
    root.mkdir()
    candidates = mod._candidate_binaries(root)

    assert candidates, "expected at least the target/release candidate"
    for candidate in candidates:
        assert str(candidate).startswith(
            str(root)
        ), f"candidate {candidate} escapes the workspace"


def test_fails_loud_without_cargo_toml_or_binary(setup_module, tmp_path):
    with pytest.raises(RuntimeError, match="Cargo.toml"):
        setup_module.ensure_bundled_fastled_binary(tmp_path)


def test_keeps_existing_binary_when_building_from_archive(setup_module, tmp_path):
    """Without Cargo.toml (sdist-style tree), a bundled binary is trusted as-is."""
    mod = setup_module
    bin_dir = tmp_path / "src" / "fastled" / "bin"
    bin_dir.mkdir(parents=True)
    exe = bin_dir / mod.EXE_NAME
    exe.write_bytes(b"BUNDLED")

    mod.ensure_bundled_fastled_binary(tmp_path)

    assert exe.read_bytes() == b"BUNDLED"


@pytest.mark.skipif(sys.platform != "win32", reason="exercises Windows arch dirs")
def test_copy_prefers_arch_target_and_refreshes_mtime(
    setup_module, tmp_path, monkeypatch
):
    mod = setup_module
    monkeypatch.delenv("CARGO_BUILD_TARGET", raising=False)
    monkeypatch.delenv("FASTLED_RUST_TARGET", raising=False)
    root = tmp_path
    built = root / "target" / "x86_64-pc-windows-msvc" / "release" / mod.EXE_NAME
    built.parent.mkdir(parents=True)
    built.write_bytes(b"FRESH")
    import os

    old = 1_000_000_000
    os.utime(built, (old, old))

    assert mod._copy_first_available_binary(root)

    dest = root / "src" / "fastled" / "bin" / mod.EXE_NAME
    assert dest.read_bytes() == b"FRESH"
    assert dest.stat().st_mtime > old, "dest mtime must be refreshed, not copied"
