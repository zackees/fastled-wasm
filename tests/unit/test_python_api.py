import importlib.util
import re
from pathlib import Path

import fastled


def test_python_package_is_cli_only() -> None:
    assert fastled.__version__
    assert fastled.__all__ == ["__version__"]
    assert not hasattr(fastled, "BuildService")


def test_python_version_matches_cargo_workspace_version() -> None:
    cargo_toml = Path(__file__).resolve().parents[2] / "Cargo.toml"
    in_workspace_package = False
    cargo_version = None
    version_pattern = re.compile(r'^version\s*=\s*"([^"]+)"')
    for raw_line in cargo_toml.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line.startswith("[") and line.endswith("]"):
            in_workspace_package = line == "[workspace.package]"
            continue
        if in_workspace_package:
            match = version_pattern.match(line)
            if not match:
                continue
            cargo_version = match.group(1)
            break

    assert cargo_version
    assert fastled.__version__ == cargo_version


def test_native_extension_is_not_packaged() -> None:
    assert importlib.util.find_spec("fastled._native") is None
