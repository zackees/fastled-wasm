"""Python request/result DTOs for the native build service wrapper.

These dataclasses are part of the public Python compatibility surface. They do
not own build orchestration or fallback service behavior.
"""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from fastled.types import BuildMode, CompileResult

BuildStrategy = Literal["cold", "incremental"]


def _optional_path(value: Path | str | None) -> Path | None:
    if value is None:
        return None
    return value if isinstance(value, Path) else Path(value)


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
class BuildArtifacts:
    js: Path | None = None
    wasm: Path | None = None
    dwarf: Path | None = None
    symbol_map: Path | None = None
    frontend_assets: Path | None = None

    def __post_init__(self) -> None:
        self.js = _optional_path(self.js)
        self.wasm = _optional_path(self.wasm)
        self.dwarf = _optional_path(self.dwarf)
        self.symbol_map = _optional_path(self.symbol_map)
        self.frontend_assets = _optional_path(self.frontend_assets)

    def as_dict(self) -> dict[str, Path]:
        return {
            name: path
            for name, path in {
                "js": self.js,
                "wasm": self.wasm,
                "dwarf": self.dwarf,
                "symbol_map": self.symbol_map,
                "frontend_assets": self.frontend_assets,
            }.items()
            if path is not None
        }

    @classmethod
    def from_mapping(cls, artifacts: dict[str, Path | str]) -> "BuildArtifacts":
        return cls(
            js=_optional_path(artifacts.get("js")),
            wasm=_optional_path(artifacts.get("wasm")),
            dwarf=_optional_path(artifacts.get("dwarf")),
            symbol_map=_optional_path(artifacts.get("symbol_map")),
            frontend_assets=_optional_path(artifacts.get("frontend_assets")),
        )

    def __getitem__(self, name: str) -> Path:
        artifacts = self.as_dict()
        if name not in artifacts:
            raise KeyError(name)
        return artifacts[name]

    def get(self, name: str, default: Path | None = None) -> Path | None:
        return self.as_dict().get(name, default)

    def items(self) -> Iterator[tuple[str, Path]]:
        return iter(self.as_dict().items())


@dataclass
class NativeBuildPayload:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes
    zip_time: float
    libfastled_time: float
    sketch_time: float
    response_processing_time: float
    strategy: BuildStrategy
    output_dir: Path
    artifacts: BuildArtifacts

    def __post_init__(self) -> None:
        if not isinstance(self.output_dir, Path):
            self.output_dir = Path(self.output_dir)
        if not isinstance(self.artifacts, BuildArtifacts):
            raise TypeError("artifacts must be BuildArtifacts")


@dataclass
class BuildResult:
    compile_result: CompileResult
    strategy: BuildStrategy
    output_dir: Path
    artifacts: BuildArtifacts

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
