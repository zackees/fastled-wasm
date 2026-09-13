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
    assert "getrandom" not in dependencies
    assert "tempfile" not in dependencies
    assert "strsim" not in dependencies
    assert "crossterm" not in dependencies
    assert "tokio" not in dependencies
    assert "ctcb-core" not in dependencies
    assert "shell-words" not in dependencies
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    assert "tokio" not in workspace["workspace"]["dependencies"]
    assert "ctcb-core" not in workspace["workspace"]["dependencies"]
    assert "shell-words" not in workspace["workspace"]["dependencies"]
    assert "tempfile" not in manifest.get("dev-dependencies", {})
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
        assert "getrandom::" not in source.read_text(), source
        assert "tempfile::" not in source.read_text(), source
        assert "strsim::" not in source.read_text(), source
        assert "crossterm::" not in source.read_text(), source
        assert "ctcb_core::" not in source.read_text(), source
        assert "shell_words::" not in source.read_text(), source
        for backend in ("zip", "tar", "zstd", "flate2"):
            assert f"{backend}::" not in source.read_text(), source


def test_archive_download_uses_kernel_http():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/archive.rs").read_text()
    assert "reqwest::" not in source
    assert "kernal_api::http::" in source


def test_toml_configuration_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    for path in (root / "Cargo.toml", root / "crates/fastled-cli/Cargo.toml"):
        data = tomllib.loads(path.read_text())
        assert "toml" not in data.get("dependencies", {})
        assert "toml" not in data.get("workspace", {}).get("dependencies", {})
    for source in (root / "crates/fastled-cli/src").rglob("*.rs"):
        assert "toml::" not in source.read_text(), source


def test_project_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/project.rs").read_text()
    assert "serde_json::" not in source
    assert "kernal_api::json" in source


def test_direct_cflags_cache_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/wasm_build.rs").read_text()
    cache_code = source.split("const DIRECT_CFLAGS_SCHEMA", 1)[1].split(
        "fn spawn_line_reader", 1
    )[0]
    assert "serde_json::" not in cache_code
    assert "Serialize" not in cache_code
    assert "Deserialize" not in cache_code
    assert "json::parse_members" in cache_code
    assert "json::encode" in cache_code


def test_wasm_build_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/wasm_build.rs").read_text()
    assert "serde_json::" not in source
    assert "serde::" not in source
    assert "kernal_api::json" in source


def test_installer_receipt_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/install.rs").read_text()
    records = source.split("struct ToolchainReceipt", 1)[1].split(
        "fn validate_managed_install", 1
    )[0]
    assert "serde_json::" not in records
    assert "serde(" not in records
    assert "Serialize" not in records
    assert "Deserialize" not in records
    assert "json::parse_members" in records
    assert "json::encode" in records


def test_dwarf_smoke_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/dwarf_smoke.rs").read_text()
    assert "serde_json::" not in source
    assert "kernal_api::json" in source


def test_editor_json_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/clangd_config.rs").read_text()
    production = source.split("#[cfg(test)]", 1)[0]
    assert "serde_json::" not in source
    assert "json!(" not in production
    assert "install::read_json_file" not in production
    assert "install::write_json_file" not in production
    assert "kernal_api::json" in production


def test_cpp_source_analysis_uses_kernel():
    root = Path(__file__).resolve().parents[2]
    for path in (root / "Cargo.toml", root / "crates/fastled-cli/Cargo.toml"):
        data = tomllib.loads(path.read_text())
        for name in ("tree-sitter", "tree-sitter-cpp"):
            assert name not in data.get("dependencies", {})
            assert name not in data.get("workspace", {}).get("dependencies", {})
    for source in (root / "crates/fastled-cli/src").rglob("*.rs"):
        for backend in ("tree_sitter::", "tree_sitter_cpp::"):
            assert backend not in source.read_text(), source


def test_viewer_backend_is_owned_by_kernel():
    root = Path(__file__).resolve().parents[2]
    manifests = [root / "Cargo.toml", root / "crates/fastled-cli/Cargo.toml"]
    for path in manifests:
        data = tomllib.loads(path.read_text())
        sections = [data, data.get("workspace", {})]
        sections.extend(data.get("target", {}).values())
        for section in sections:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for backend in ("tauri", "tauri-build", "gtk", "webkit2gtk"):
                    assert backend not in section.get(kind, {}), (path, backend)
    for source in (root / "crates/fastled-cli").rglob("*.rs"):
        if "target" in source.parts or "gen" in source.parts:
            continue
        for backend in ("tauri::", "tauri_build::", "gtk::", "webkit2gtk::"):
            assert backend not in source.read_text(), (source, backend)


def test_obsolete_manifest_dependencies_are_removed():
    root = Path(__file__).resolve().parents[2]
    package = tomllib.loads((root / "crates/fastled-cli/Cargo.toml").read_text())
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    assert "indexmap" not in package.get("build-dependencies", {})
    assert "thiserror" not in workspace["workspace"]["dependencies"]


def test_python_shim_has_no_obsolete_runtime_requirements():
    root = Path(__file__).resolve().parents[2]
    project = tomllib.loads((root / "pyproject.toml").read_text())["project"]
    requirements = project["dependencies"]
    for obsolete in ("typeguard", "zcmds_win32", "zcmds-win32"):
        assert not any(requirement.startswith(obsolete) for requirement in requirements)
    for required in ("meson", "ninja", "uv"):
        assert any(requirement.startswith(required + ">=") for requirement in requirements)


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
        assert "tokio::" not in source, path
        for backend in (
            "tokio::runtime::",
            "tokio::time::",
            "tokio::signal::",
            "tokio::pin!",
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


def test_source_receipt_uses_kernal_json() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/source.rs").read_text()
    assert "serde::" not in source
    assert "serde_json::" not in source
    assert "json::parse_members" in source


def test_debug_manifest_uses_kernal_json() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/debug_symbols.rs").read_text()
    assert "serde::" not in source
    assert "serde_json::" not in source
    assert "json::parse_members" in source


def test_server_json_uses_kernal() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/server.rs").read_text()
    assert "serde::" not in source
    assert "serde_json::" not in source
    assert "json::parse_members" in source


def test_cache_json_uses_kernal() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/dynamic_cache.rs").read_text()
    assert "serde::" not in source
    assert "serde_json::" not in source
    assert "json::parse_members" in source


def test_snapshot_json_uses_kernal() -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "crates/fastled-cli/src/sketch_preprocessor.rs").read_text()
    assert "serde::" not in source
    assert "serde_json::" not in source
    assert "json::parse_members" in source
