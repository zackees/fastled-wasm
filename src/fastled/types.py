import argparse
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

from fastled.print_filter import PrintFilterFastled


@dataclass
class Args:
    directory: Path | None
    init: bool | str
    just_compile: bool
    web: str | None
    interactive: bool
    profile: bool
    force_compile: bool
    auto_update: bool | None
    update: bool
    localhost: bool
    build: bool
    server: bool
    purge: bool
    debug: bool
    quick: bool
    release: bool

    @staticmethod
    def from_namespace(args: argparse.Namespace) -> "Args":
        assert isinstance(
            args.directory, str | None
        ), f"expected str | None, got {type(args.directory)}"
        assert isinstance(
            args.init, bool | str | None
        ), f"expected bool, got {type(args.init)}"
        assert isinstance(
            args.just_compile, bool
        ), f"expected bool, got {type(args.just_compile)}"
        assert isinstance(
            args.web, str | None
        ), f"expected str | None, got {type(args.web)}"
        assert isinstance(
            args.interactive, bool
        ), f"expected bool, got {type(args.interactive)}"
        assert isinstance(
            args.profile, bool
        ), f"expected bool, got {type(args.profile)}"
        assert isinstance(
            args.force_compile, bool
        ), f"expected bool, got {type(args.force_compile)}"
        assert isinstance(
            args.no_auto_updates, bool | None
        ), f"expected bool | None, got {type(args.no_auto_updates)}"
        assert isinstance(args.update, bool), f"expected bool, got {type(args.update)}"
        assert isinstance(
            args.localhost, bool
        ), f"expected bool, got {type(args.localhost)}"
        assert isinstance(args.build, bool), f"expected bool, got {type(args.build)}"
        assert isinstance(args.server, bool), f"expected bool, got {type(args.server)}"
        assert isinstance(args.purge, bool), f"expected bool, got {type(args.purge)}"
        assert isinstance(args.debug, bool), f"expected bool, got {type(args.debug)}"
        assert isinstance(args.quick, bool), f"expected bool, got {type(args.quick)}"
        assert isinstance(
            args.release, bool
        ), f"expected bool, got {type(args.release)}"
        init: bool | str = False
        if args.init is None:
            init = False
        elif isinstance(args.init, bool):
            init = args.init
        elif isinstance(args.init, str):
            init = args.init
        return Args(
            directory=Path(args.directory) if args.directory else None,
            init=init,
            just_compile=args.just_compile,
            web=args.web,
            interactive=args.interactive,
            profile=args.profile,
            force_compile=args.force_compile,
            auto_update=not args.no_auto_updates,
            update=args.update,
            localhost=args.localhost,
            build=args.build,
            server=args.server,
            purge=args.purge,
            debug=args.debug,
            quick=args.quick,
            release=args.release,
        )


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
        pf = PrintFilterFastled(echo=False)
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
