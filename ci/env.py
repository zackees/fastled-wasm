from __future__ import annotations

import os
import platform
import shutil
import subprocess
import tomllib
from pathlib import Path


def cargo_home() -> Path:
    if os.environ.get("CARGO_HOME"):
        return Path(os.environ["CARGO_HOME"]).expanduser()
    return Path.home() / ".cargo"


def cargo_bin() -> Path:
    return cargo_home() / "bin"


def rustup_home() -> Path:
    if os.environ.get("RUSTUP_HOME"):
        return Path(os.environ["RUSTUP_HOME"]).expanduser()
    return Path.home() / ".rustup"


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def toolchain_file() -> Path:
    return repo_root() / "rust-toolchain.toml"


def load_toolchain_channel() -> str:
    with toolchain_file().open("rb") as handle:
        data = tomllib.load(handle)
    toolchain = data.get("toolchain")
    if not isinstance(toolchain, dict):
        raise RuntimeError(f"missing [toolchain] in {toolchain_file()}")
    channel = toolchain.get("channel")
    if not isinstance(channel, str) or not channel:
        raise RuntimeError(f"missing toolchain.channel in {toolchain_file()}")
    return channel


def host_target_triple() -> str:
    system = platform.system()
    machine = platform.machine().lower()
    arch = {
        "amd64": "x86_64",
        "x86_64": "x86_64",
        "arm64": "aarch64",
        "aarch64": "aarch64",
    }.get(machine)
    if arch is None:
        raise RuntimeError(f"unsupported architecture: {machine}")
    if system == "Windows":
        return f"{arch}-pc-windows-msvc"
    if system == "Linux":
        return f"{arch}-unknown-linux-gnu"
    if system == "Darwin":
        return f"{arch}-apple-darwin"
    raise RuntimeError(f"unsupported platform: {system}")


def toolchain_name() -> str:
    return f"{load_toolchain_channel()}-{host_target_triple()}"


def toolchain_bin() -> Path:
    return rustup_home() / "toolchains" / toolchain_name() / "bin"


def _find_vswhere() -> Path | None:
    candidates = [
        Path(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        Path(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def _find_vsdevcmd() -> Path | None:
    vswhere = _find_vswhere()
    if vswhere is None:
        return None
    result = subprocess.run(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    installation_path = result.stdout.strip()
    if not installation_path:
        return None
    candidate = Path(installation_path) / "Common7" / "Tools" / "VsDevCmd.bat"
    if candidate.is_file():
        return candidate
    return None


def _windows_build_env() -> dict[str, str]:
    env = os.environ.copy()
    toolchain_bin_dir = toolchain_bin()
    if toolchain_bin_dir.is_dir():
        env["PATH"] = str(toolchain_bin_dir) + os.pathsep + env.get("PATH", "")
        cargo_exe = toolchain_bin_dir / "cargo.exe"
        rustc_exe = toolchain_bin_dir / "rustc.exe"
        if cargo_exe.is_file():
            env["CARGO"] = str(cargo_exe)
        if rustc_exe.is_file():
            env["RUSTC"] = str(rustc_exe)
        env["RUSTUP_TOOLCHAIN"] = toolchain_name()
        env["CARGO_BUILD_TARGET"] = host_target_triple()

    vsdevcmd = _find_vsdevcmd()
    if vsdevcmd is None:
        return env

    command = f'"{vsdevcmd}" -arch=x64 -host_arch=x64 >nul && set'
    result = subprocess.run(
        ["cmd", "/d", "/s", "/c", command],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    if result.returncode != 0:
        return env
    for line in result.stdout.splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        env[key] = value
    return env


def activate() -> None:
    bin_dir = cargo_bin()
    if not bin_dir.is_dir():
        return

    current_path = os.environ.get("PATH", "")
    path_parts = current_path.split(os.pathsep) if current_path else []
    normalized_cargo_bin = os.path.normcase(os.path.normpath(str(bin_dir)))
    normalized_parts = {
        os.path.normcase(os.path.normpath(part)) for part in path_parts if part
    }
    if normalized_cargo_bin in normalized_parts:
        return
    os.environ["PATH"] = (
        str(bin_dir) if not current_path else str(bin_dir) + os.pathsep + current_path
    )


def _apply_sccache(env: dict[str, str]) -> dict[str, str]:
    """Set RUSTC_WRAPPER=sccache when sccache is available."""
    if env.get("RUSTC_WRAPPER"):
        return env
    sccache = shutil.which("sccache", path=env.get("PATH"))
    if sccache:
        env["RUSTC_WRAPPER"] = sccache
    return env


def clean_env() -> dict[str, str]:
    activate()
    env = os.environ.copy()
    env.pop("VIRTUAL_ENV", None)
    env.setdefault("PYTHONUTF8", "1")
    if platform.system() == "Windows":
        env = env | _windows_build_env()
        env.pop("VIRTUAL_ENV", None)
        env.setdefault("PYTHONUTF8", "1")
    env.setdefault("RUSTUP_TOOLCHAIN", toolchain_name())
    env = _apply_sccache(env)
    return env


def build_env() -> dict[str, str]:
    return clean_env()
