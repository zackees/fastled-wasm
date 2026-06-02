from __future__ import annotations

import re
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path


def _workspace_package_version() -> str | None:
    for parent in Path(__file__).resolve().parents:
        cargo_toml = parent / "Cargo.toml"
        if cargo_toml.is_file():
            return _read_workspace_package_version(cargo_toml)
    return None


def _read_workspace_package_version(cargo_toml: Path) -> str | None:
    in_workspace_package = False
    version_pattern = re.compile(r'^version\s*=\s*"([^"]+)"')
    for raw_line in cargo_toml.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line.startswith("[") and line.endswith("]"):
            in_workspace_package = line == "[workspace.package]"
            continue
        if in_workspace_package:
            match = version_pattern.match(line)
            if match:
                return match.group(1)
    return None


def _installed_or_source_version() -> str:
    source_version = _workspace_package_version()
    if source_version:
        return source_version
    try:
        return version("fastled")
    except PackageNotFoundError:
        raise


__version__ = _installed_or_source_version()
