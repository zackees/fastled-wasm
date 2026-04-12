from __future__ import annotations

import io
import json
import shutil
import time
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

from fastled.build_types import BuildRequest, BuildResult, BuildStrategy
from fastled.interrupts import handle_keyboard_interrupt
from fastled.types import BuildMode, CompileResult


class _CompilerToolchain(Protocol):
    def compile(
        self,
        sketch_dir: Path,
        output_dir: Path,
        build_mode: BuildMode,
        profile: bool,
    ) -> Path: ...


@dataclass
class _BuildState:
    build_mode: str
    profile: bool
    fastled_path: str | None


class BuildService:
    def __init__(self) -> None:
        self._toolchains: dict[str | None, _CompilerToolchain] = {}
        self._states: dict[Path, _BuildState] = {}

    def register_toolchain(
        self, fastled_path: Path | None, toolchain: _CompilerToolchain
    ) -> None:
        key = str(fastled_path.resolve()) if fastled_path else None
        self._toolchains[key] = toolchain

    def build(self, request: BuildRequest) -> BuildResult:
        from fastled.toolchain.emscripten import EmscriptenToolchain

        strategy = self.detect_strategy(request)
        output_dir = request.output_dir

        if request.force_clean and output_dir.exists():
            shutil.rmtree(output_dir, ignore_errors=True)

        toolchain_key = (
            str(request.fastled_path.resolve()) if request.fastled_path else None
        )
        toolchain = self._toolchains.get(toolchain_key)
        if toolchain is None:
            toolchain = EmscriptenToolchain(fastled_path=request.fastled_path)
            self._toolchains[toolchain_key] = toolchain

        start_time = time.time()
        zip_time = 0.0
        try:
            output_dir.mkdir(parents=True, exist_ok=True)
            js_file = toolchain.compile(
                sketch_dir=request.sketch_dir,
                output_dir=output_dir,
                build_mode=request.build_mode,
                profile=request.profile,
            )
            compile_time = time.time() - start_time
            zip_start = time.time()
            zip_bytes = self._zip_output(output_dir)
            zip_time = time.time() - zip_start
            compile_result = CompileResult(
                success=True,
                stdout=(
                    "Native compilation successful!\n"
                    f"Output: {js_file}\n"
                    f"WASM: {js_file.with_suffix('.wasm')}"
                ),
                hash_value=None,
                zip_bytes=zip_bytes,
                zip_time=zip_time,
                libfastled_time=0.0,
                sketch_time=compile_time,
                response_processing_time=0.0,
            )
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
            raise
        except Exception as exc:
            compile_time = time.time() - start_time
            compile_result = CompileResult(
                success=False,
                stdout=f"Native compilation failed: {exc}",
                hash_value=None,
                zip_bytes=b"",
                zip_time=0.0,
                libfastled_time=0.0,
                sketch_time=compile_time,
                response_processing_time=0.0,
            )

        if compile_result.success:
            state = _BuildState(
                build_mode=request.build_mode.value,
                profile=request.profile,
                fastled_path=toolchain_key,
            )
            self._states[request.sketch_dir.resolve()] = state
            self._write_state(output_dir, state)

        return BuildResult(
            compile_result=compile_result,
            strategy=strategy,
            output_dir=output_dir,
            artifacts=self._discover_artifacts(output_dir),
        )

    def detect_strategy(self, request: BuildRequest) -> BuildStrategy:
        if request.force_clean:
            return "cold"

        output_dir = request.output_dir
        if not output_dir.exists():
            return "cold"

        required_artifacts = [
            output_dir / "fastled.js",
            output_dir / "fastled.wasm",
        ]
        if not all(path.exists() for path in required_artifacts):
            return "cold"

        previous = self._states.get(request.sketch_dir.resolve())
        if previous is None:
            previous = self._read_state(output_dir)
            if previous is not None:
                self._states[request.sketch_dir.resolve()] = previous
        if previous is None:
            return "cold"

        current_fastled_path = (
            str(request.fastled_path.resolve()) if request.fastled_path else None
        )
        if previous.fastled_path != current_fastled_path:
            return "cold"
        if previous.build_mode != request.build_mode.value:
            return "cold"
        if previous.profile != request.profile:
            return "cold"
        return "incremental"

    def purge(self, sketch_dir: Path) -> None:
        resolved = sketch_dir.resolve()
        self._states.pop(resolved, None)
        output_dir = resolved / "fastled_js"
        if output_dir.exists():
            shutil.rmtree(output_dir, ignore_errors=True)

    @staticmethod
    def _state_file(output_dir: Path) -> Path:
        return output_dir / ".fastled_build_state.json"

    @classmethod
    def _write_state(cls, output_dir: Path, state: _BuildState) -> None:
        try:
            cls._state_file(output_dir).write_text(
                json.dumps(
                    {
                        "build_mode": state.build_mode,
                        "profile": state.profile,
                        "fastled_path": state.fastled_path,
                    }
                ),
                encoding="utf-8",
            )
        except OSError:
            pass

    @classmethod
    def _read_state(cls, output_dir: Path) -> _BuildState | None:
        state_file = cls._state_file(output_dir)
        if not state_file.exists():
            return None
        try:
            payload = json.loads(state_file.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return None
        if not isinstance(payload, dict):
            return None
        build_mode = payload.get("build_mode")
        profile = payload.get("profile")
        fastled_path = payload.get("fastled_path")
        if not isinstance(build_mode, str) or not isinstance(profile, bool):
            return None
        if fastled_path is not None and not isinstance(fastled_path, str):
            return None
        return _BuildState(
            build_mode=build_mode,
            profile=profile,
            fastled_path=fastled_path,
        )

    @staticmethod
    def _zip_output(output_dir: Path) -> bytes:
        zip_buffer = io.BytesIO()
        zip_start = time.time()
        with zipfile.ZipFile(zip_buffer, "w", zipfile.ZIP_DEFLATED) as zf:
            for file_path in output_dir.rglob("*"):
                if file_path.is_file():
                    zf.write(file_path, file_path.relative_to(output_dir))
        _ = time.time() - zip_start
        return zip_buffer.getvalue()

    @staticmethod
    def _discover_artifacts(output_dir: Path) -> dict[str, Path]:
        artifacts: dict[str, Path] = {}
        candidates = {
            "js": output_dir / "fastled.js",
            "wasm": output_dir / "fastled.wasm",
            "dwarf": output_dir / "fastled.wasm.dwarf",
            "symbol_map": output_dir / "fastled.js.symbols",
            "frontend_assets": output_dir / "assets",
        }
        for name, path in candidates.items():
            if path.exists():
                artifacts[name] = path
        if "frontend_assets" not in artifacts and output_dir.exists():
            artifacts["frontend_assets"] = output_dir
        return artifacts
