import argparse
from enum import Enum


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


def get_build_mode(args: argparse.Namespace) -> BuildMode:
    if args.debug:
        return BuildMode.DEBUG
    elif args.release:
        return BuildMode.RELEASE
    else:
        return BuildMode.QUICK
