import importlib.util
import re
import subprocess
from pathlib import Path
from unittest.mock import patch

import fastled
from fastled import _rust_cli


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


def test_rust_launcher_exports_absolute_packaged_frontend_dir() -> None:
    bundled_cli = Path("/tmp/fastled-wheel/bin/fastled")
    bundled_uv = Path("/tmp/fastled-wheel/bin/uv")
    with (
        patch.object(_rust_cli, "find_rust_fastled_cli", return_value=bundled_cli),
        patch.object(_rust_cli, "_managed_uv_executable", return_value=bundled_uv),
        patch.object(_rust_cli.subprocess, "run") as run,
    ):
        run.return_value = subprocess.CompletedProcess([], 0)
        assert _rust_cli.invoke_rust_fastled_cli(["--version"]) == 0

    env = run.call_args.kwargs["env"]
    frontend = Path(env["FASTLED_FRONTEND_DIR"])
    assert frontend.is_absolute()
    assert frontend == Path(_rust_cli.__file__).resolve().parent / "frontend"
    assert env["FASTLED_UV_EXECUTABLE"] == str(bundled_uv)
