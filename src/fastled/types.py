from dataclasses import asdict, dataclass
from enum import Enum
from typing import Protocol

from fastled.print_filter import PrintFilterDefault

CompileResultValue = bool | str | bytes | float | None


class _BuildModeArgs(Protocol):
    debug: bool
    release: bool


@dataclass
class CompileResultSnapshot:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes
    zip_time: float
    libfastled_time: float
    sketch_time: float
    response_processing_time: float


@dataclass
class CompileResult:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes
    zip_time: float
    libfastled_time: float
    sketch_time: float
    response_processing_time: float

    def __bool__(self) -> bool:
        return self.success

    def to_dataclass(self) -> CompileResultSnapshot:
        return CompileResultSnapshot(
            success=self.success,
            stdout=self.stdout,
            hash_value=self.hash_value,
            zip_bytes=self.zip_bytes,
            zip_time=self.zip_time,
            libfastled_time=self.libfastled_time,
            sketch_time=self.sketch_time,
            response_processing_time=self.response_processing_time,
        )

    def to_dict(self) -> dict[str, CompileResultValue]:
        return asdict(self.to_dataclass())

    def __post_init__(self):
        # Filter the stdout.
        pf = PrintFilterDefault(echo=False)
        self.stdout = pf.print(self.stdout)


class BuildMode(Enum):
    DEBUG = "DEBUG"
    QUICK = "QUICK"
    RELEASE = "RELEASE"

    @classmethod
    def from_string(cls, mode_str: str) -> "BuildMode":
        try:
            return cls[mode_str.upper()]
        except KeyError:
            valid_modes = [mode.name for mode in cls]
            raise ValueError(f"BUILD_MODE must be one of {valid_modes}, got {mode_str}")

    @staticmethod
    def from_args(args: _BuildModeArgs) -> "BuildMode":
        if args.debug:
            return BuildMode.DEBUG
        elif args.release:
            return BuildMode.RELEASE
        else:
            return BuildMode.QUICK
