from dataclasses import dataclass
from enum import Enum
from typing import Any

from fastled.args import Args
from fastled.print_filter import PrintFilterDefault


@dataclass
class CompileResult:
    success: bool
    stdout: str
    hash_value: str | None
    zip_bytes: bytes

    def __bool__(self) -> bool:
        return self.success

    def to_dict(self) -> dict[str, Any]:
        return self.__dict__.copy()

    def __post_init__(self):
        # Filter the stdout.
        pf = PrintFilterDefault(echo=False)
        self.stdout = pf.print(self.stdout)


class CompileServerError(Exception):
    """Error class for failing to instantiate CompileServer."""

    pass


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
    def from_args(args: Args) -> "BuildMode":
        if args.debug:
            return BuildMode.DEBUG
        elif args.release:
            return BuildMode.RELEASE
        else:
            return BuildMode.QUICK


class Platform(Enum):
    WASM = "WASM"

    @classmethod
    def from_string(cls, platform_str: str) -> "Platform":
        try:
            return cls[platform_str.upper()]
        except KeyError:
            valid_modes = [mode.name for mode in cls]
            raise ValueError(
                f"Platform must be one of {valid_modes}, got {platform_str}"
            )


@dataclass
class FileResponse:
    """File response from the server."""

    filename: str
    content: str
    mimetype: str
