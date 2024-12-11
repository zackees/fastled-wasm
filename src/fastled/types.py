import argparse
from dataclasses import dataclass
from enum import Enum
from typing import Any


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
    def from_args(args: argparse.Namespace) -> "BuildMode":
        if args.debug:
            return BuildMode.DEBUG
        elif args.release:
            return BuildMode.RELEASE
        else:
            return BuildMode.QUICK
