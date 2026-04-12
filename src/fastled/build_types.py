from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal

from fastled.types import BuildMode, CompileResult

BuildStrategy = Literal["cold", "incremental"]


@dataclass(frozen=True)
class BuildRequest:
    sketch_dir: Path
    build_mode: BuildMode
    profile: bool = False
    fastled_path: Path | None = None
    force_clean: bool = False

    @property
    def output_dir(self) -> Path:
        return self.sketch_dir / "fastled_js"


@dataclass
class BuildResult:
    compile_result: CompileResult
    strategy: BuildStrategy
    output_dir: Path
    artifacts: dict[str, Path] = field(default_factory=dict)

    @property
    def success(self) -> bool:
        return self.compile_result.success

    @property
    def stdout(self) -> str:
        return self.compile_result.stdout

    @property
    def sketch_time(self) -> float:
        return self.compile_result.sketch_time

    @property
    def zip_bytes(self) -> bytes:
        return self.compile_result.zip_bytes
