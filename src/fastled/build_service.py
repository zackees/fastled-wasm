"""Compatibility build-service facade backed by the native Rust service.

This module preserves the Python ``BuildService`` API for callers that still
import it directly. Build orchestration is owned by ``NativeBuildService``;
Python only adapts request/result types.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, cast

from fastled._native import NativeBuildService as _NativeBuildService
from fastled.build_types import BuildRequest, BuildResult, BuildStrategy
from fastled.interrupts import handle_keyboard_interrupt
from fastled.types import CompileResult


def _path_key(fastled_path: Path | None) -> str | None:
    return str(fastled_path.resolve()) if fastled_path else None


class BuildService:
    def __init__(self) -> None:
        self._native = _NativeBuildService()

    def register_toolchain(self, fastled_path: Path | None, toolchain: Any) -> None:
        """Compatibility no-op for callers that used to inject toolchains."""
        self._native.register_toolchain(toolchain, _path_key(fastled_path))

    def build(self, request: BuildRequest) -> BuildResult:
        try:
            payload = self._native.build(
                str(request.sketch_dir),
                request.build_mode.value,
                request.build_mode,
                request.profile,
                _path_key(request.fastled_path),
                request.force_clean,
            )
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
            raise

        compile_result = CompileResult(
            success=bool(payload["success"]),
            stdout=str(payload["stdout"]),
            hash_value=cast(str | None, payload["hash_value"]),
            zip_bytes=bytes(payload["zip_bytes"]),
            zip_time=float(payload["zip_time"]),
            libfastled_time=float(payload["libfastled_time"]),
            sketch_time=float(payload["sketch_time"]),
            response_processing_time=float(payload["response_processing_time"]),
        )

        artifacts = {
            name: Path(path_str)
            for name, path_str in cast(dict[str, str], payload["artifacts"]).items()
        }

        return BuildResult(
            compile_result=compile_result,
            strategy=cast(BuildStrategy, payload["strategy"]),
            output_dir=Path(str(payload["output_dir"])),
            artifacts=artifacts,
        )

    def detect_strategy(self, request: BuildRequest) -> BuildStrategy:
        return cast(
            BuildStrategy,
            self._native.detect_strategy(
                str(request.sketch_dir),
                request.build_mode.value,
                request.profile,
                _path_key(request.fastled_path),
                request.force_clean,
            ),
        )

    def purge(self, sketch_dir: Path) -> None:
        self._native.purge(str(sketch_dir))
