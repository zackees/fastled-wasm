"""Compatibility Emscripten helpers for the native Rust WASM backend.

Build orchestration now lives in ``fastled_cli::wasm_build`` and is exposed to
Python through ``fastled._native.NativeBuildService``. This module keeps the
small public helper surface that older callers and tests import directly.
"""

from __future__ import annotations

import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from fastled.build_types import BuildRequest
from fastled.types import BuildMode

_clang_tool_chain_emscripten_dir_cache: Path | None | str = "UNSET"


def _resolve_example_name(
    sketch_dir: Path, fastled_dir: Path
) -> tuple[str, Path, bool]:
    """Resolve an example name relative to ``FastLED/examples``."""
    try:
        rel_path = sketch_dir.relative_to(fastled_dir / "examples")
        return str(rel_path).replace("\\", "/"), sketch_dir, True
    except ValueError:
        name = sketch_dir.name
        return name, fastled_dir / "examples" / name, False


def _get_clang_tool_chain_emscripten_dir() -> Path | None:
    """Resolve the Rust-installed Emscripten directory."""
    global _clang_tool_chain_emscripten_dir_cache
    if _clang_tool_chain_emscripten_dir_cache != "UNSET":
        return _clang_tool_chain_emscripten_dir_cache  # type: ignore[return-value]

    rust_install = os.environ.get("FASTLED_EMSCRIPTEN_DIR")
    if rust_install:
        candidate = Path(rust_install)
        if (candidate / "emscripten" / "emcc.py").exists():
            _clang_tool_chain_emscripten_dir_cache = candidate
            return candidate

    env_path = os.environ.get("CLANG_TOOL_CHAIN_DOWNLOAD_PATH")
    base_dir = Path(env_path) if env_path else Path.home() / ".clang-tool-chain"
    emscripten_base = base_dir / "emscripten"

    if not emscripten_base.exists():
        _clang_tool_chain_emscripten_dir_cache = None
        return None

    candidates = [emscripten_base]
    candidates.extend(path for path in emscripten_base.glob("*") if path.is_dir())
    candidates.extend(path for path in emscripten_base.glob("*/*") if path.is_dir())
    for candidate in candidates:
        if (candidate / "emscripten" / "emcc.py").exists():
            _clang_tool_chain_emscripten_dir_cache = candidate
            return candidate

    _clang_tool_chain_emscripten_dir_cache = None
    return None


def ensure_clang_tool_chain_emscripten() -> Path | None:
    """Return the resolved Emscripten installation directory, if present."""
    return _get_clang_tool_chain_emscripten_dir()


def _setup_emscripten_env(env: dict[str, str]) -> None:
    """Populate environment variables used by Emscripten subprocesses."""
    if sys.platform == "win32":
        env["EMCC_CORES"] = "128"

    clang_tool_chain_dir = _get_clang_tool_chain_emscripten_dir()
    if not clang_tool_chain_dir:
        return

    emscripten_dir = clang_tool_chain_dir / "emscripten"
    config_path = clang_tool_chain_dir / ".emscripten"
    bin_dir = clang_tool_chain_dir / "bin"

    if emscripten_dir.exists():
        env["EMSCRIPTEN"] = str(emscripten_dir)
        env["EMSCRIPTEN_ROOT"] = str(emscripten_dir)
    if config_path.exists():
        env["EM_CONFIG"] = str(config_path)
    env["EMSDK_PYTHON"] = sys.executable
    env["EMCC_SKIP_SANITY_CHECK"] = "1"
    if bin_dir.exists():
        env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"


@dataclass(frozen=True)
class CompilerPaths:
    """Paths to Emscripten compiler entry points."""

    emcc: Path
    empp: Path
    emar: Path


class EmscriptenToolchain:
    """Compatibility wrapper around the native Rust WASM build service."""

    def __init__(
        self,
        fastled_path: Path | str | None = None,
        emsdk_path: Path | str | None = None,
    ):
        self._fastled_path: Path | None = Path(fastled_path) if fastled_path else None
        self._emsdk_path: Path | None = Path(emsdk_path) if emsdk_path else None
        self._compiler_paths: CompilerPaths | None = None
        self._version: str | None = None

    def _find_compilers(self) -> CompilerPaths:
        """Find compiler entry points without installing or building anything."""
        clang_tool_chain_dir = ensure_clang_tool_chain_emscripten()
        if clang_tool_chain_dir and clang_tool_chain_dir.exists():
            scripts_dir = clang_tool_chain_dir / "emscripten"
            emcc = scripts_dir / "emcc.py"
            empp = scripts_dir / "em++.py"
            emar = scripts_dir / "emar.py"
            if emcc.exists() and empp.exists():
                return CompilerPaths(
                    emcc=emcc,
                    empp=empp,
                    emar=emar if emar.exists() else emcc,
                )

        emcc = shutil.which("emcc")
        empp = shutil.which("em++")
        emar = shutil.which("emar")
        if emcc and empp:
            return CompilerPaths(
                emcc=Path(emcc),
                empp=Path(empp),
                emar=Path(emar) if emar else Path(emcc),
            )

        if self._emsdk_path:
            emscripten_dir = self._emsdk_path / "upstream" / "emscripten"
            emcc = emscripten_dir / ("emcc.bat" if sys.platform == "win32" else "emcc")
            empp = emscripten_dir / ("em++.bat" if sys.platform == "win32" else "em++")
            emar = emscripten_dir / ("emar.bat" if sys.platform == "win32" else "emar")
            if emcc.exists() and empp.exists():
                return CompilerPaths(emcc=emcc, empp=empp, emar=emar)

        raise FileNotFoundError(
            "Emscripten SDK not found. Install it through the native fastled CLI "
            "or set FASTLED_EMSCRIPTEN_DIR."
        )

    def compile(
        self,
        sketch_dir: Path,
        output_dir: Path,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,
    ) -> Path:
        """Compile via ``BuildService`` for legacy direct-toolchain callers."""
        from fastled.build_service import BuildService

        request = BuildRequest(
            sketch_dir=sketch_dir,
            build_mode=build_mode,
            profile=profile,
            fastled_path=self._fastled_path,
        )
        result = BuildService().build(request)
        if not result.success:
            raise RuntimeError(result.stdout)

        expected_output = sketch_dir / "fastled_js"
        if output_dir != expected_output:
            output_dir.mkdir(parents=True, exist_ok=True)
            for artifact in expected_output.iterdir():
                target = output_dir / artifact.name
                if artifact.is_dir():
                    if target.exists():
                        shutil.rmtree(target)
                    shutil.copytree(artifact, target)
                else:
                    shutil.copy2(artifact, target)
        return output_dir / "fastled.js"

    def check_installation(self) -> bool:
        """Return whether compiler entry points can be found."""
        try:
            self._compiler_paths = self._find_compilers()
            return True
        except FileNotFoundError:
            return False

    def get_version(self) -> str | None:
        """Return a cached lightweight version marker when Emscripten is present."""
        if self._version is not None:
            return self._version
        try:
            self._compiler_paths = self._find_compilers()
        except FileNotFoundError:
            return None
        self._version = str(self._compiler_paths.emcc)
        return self._version
