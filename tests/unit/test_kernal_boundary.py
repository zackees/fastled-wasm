"""Keep migrated infrastructure behind the shared kernal-api boundary."""

from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib


def test_migrated_capabilities_are_owned_by_kernal_api():
    root = Path(__file__).resolve().parents[2]
    manifest = tomllib.loads((root / "crates/fastled-cli/Cargo.toml").read_text())
    dependencies = manifest["dependencies"]
    assert "kernal-api" in dependencies
    assert "fs2" not in dependencies
    assert "globset" not in dependencies
    assert "notify" not in dependencies
    assert "zccache-fingerprint" not in dependencies
    assert "running-process" not in dependencies
    assert "sha2" not in dependencies
    assert "dirs" not in dependencies
    assert "tokio-stream" not in dependencies
    assert "axum" not in dependencies
    assert "tower-http" not in dependencies
    for backend in ("zip", "tar", "zstd", "flate2"):
        assert backend not in dependencies
    for target in manifest.get("target", {}).values():
        assert "windows-sys" not in target.get("dependencies", {})
    for source in (root / "crates/fastled-cli/src").rglob("*.rs"):
        assert "fs2::" not in source.read_text(), source
        assert "globset::" not in source.read_text(), source
        assert "notify::" not in source.read_text(), source
        assert "zccache_fingerprint::" not in source.read_text(), source
        assert "running_process::" not in source.read_text(), source
        assert "windows_sys::" not in source.read_text(), source
        assert "sha2::" not in source.read_text(), source
        assert "dirs::" not in source.read_text(), source
        for backend in ("zip", "tar", "zstd", "flate2"):
            assert f"{backend}::" not in source.read_text(), source


def test_archive_download_uses_kernel_http():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/archive.rs").read_text()
    assert "reqwest::" not in source
    assert "kernal_api::http::" in source


def test_obsolete_manifest_dependencies_are_removed():
    root = Path(__file__).resolve().parents[2]
    package = tomllib.loads((root / "crates/fastled-cli/Cargo.toml").read_text())
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    assert "indexmap" not in package.get("build-dependencies", {})
    assert "thiserror" not in workspace["workspace"]["dependencies"]


def test_http_callers_use_kernel():
    root = Path(__file__).resolve().parents[2]
    package = tomllib.loads((root / "crates/fastled-cli/Cargo.toml").read_text())
    assert "reqwest" not in package["dependencies"]
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    assert "reqwest" not in manifest["workspace"]["dependencies"]
    assert "reqwest" not in package.get("dev-dependencies", {})
    for name in ("install.rs", "project.rs", "dwarf_smoke.rs", "server.rs"):
        source = (root / "crates/fastled-cli/src" / name).read_text()
        assert "reqwest::" not in source, name


def test_migrated_async_operations_use_kernel():
    root = Path(__file__).resolve().parents[2]
    for path in (root / "crates/fastled-cli/src").rglob("*.rs"):
        source = path.read_text()
        for backend in (
            "tokio::runtime::",
            "tokio::time::",
            "tokio::sync::Semaphore",
            "tokio::spawn(",
            "tokio::task::JoinHandle",
            "tokio::sync::mpsc",
            "tokio::sync::broadcast",
            "tokio_stream::",
            "tokio::fs::write",
            "tokio::fs::create_dir_all",
            "tokio::fs::read",
            "tokio::net::TcpListener",
            "axum::",
            "tower_http::",
        ):
            assert backend not in source, path
