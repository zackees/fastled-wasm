"""Keep migrated infrastructure behind the shared kernal-api boundary."""

from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib


def test_migrated_filesystem_capabilities_are_owned_by_kernal_api():
    root = Path(__file__).resolve().parents[2]
    manifest = tomllib.loads((root / "crates/fastled-cli/Cargo.toml").read_text())
    dependencies = manifest["dependencies"]
    assert "kernal-api" in dependencies
    assert "fs2" not in dependencies
    assert "globset" not in dependencies
    assert "notify" not in dependencies
    for source in (root / "crates/fastled-cli/src").rglob("*.rs"):
        assert "fs2::" not in source.read_text(), source
        assert "globset::" not in source.read_text(), source
        assert "notify::" not in source.read_text(), source
