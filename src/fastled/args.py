import argparse
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Args:
    directory: Path | None
    init: bool | str
    just_compile: bool
    profile: bool
    app: bool  # New flag to trigger Playwright browser with browser download if needed
    debug: bool
    quick: bool
    release: bool
    install: bool = False  # Install FastLED development environment
    dry_run: bool = False  # Dry run mode for testing
    no_interactive: bool = False  # Non-interactive mode
    enable_https: bool = True  # Enable HTTPS for local server (default: True)
    fastled_path: Path | str | None = (
        None  # Path to FastLED library for native compilation
    )

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
            args.profile, bool
        ), f"expected bool, got {type(args.profile)}"
        assert isinstance(args.app, bool), f"expected bool, got {type(args.app)}"
        assert isinstance(args.debug, bool), f"expected bool, got {type(args.debug)}"
        assert isinstance(args.quick, bool), f"expected bool, got {type(args.quick)}"
        assert isinstance(
            args.release, bool
        ), f"expected bool, got {type(args.release)}"
        assert isinstance(
            args.install, bool
        ), f"expected bool, got {type(args.install)}"
        assert isinstance(
            args.dry_run, bool
        ), f"expected bool, got {type(args.dry_run)}"
        assert isinstance(
            args.no_interactive, bool
        ), f"expected bool, got {type(args.no_interactive)}"
        assert isinstance(
            args.no_https, bool
        ), f"expected bool, got {type(args.no_https)}"
        assert isinstance(
            args.fastled_path, str | None
        ), f"expected str | None, got {type(args.fastled_path)}"

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
            profile=args.profile,
            app=args.app,
            debug=args.debug,
            quick=args.quick,
            release=args.release,
            install=args.install,
            dry_run=args.dry_run,
            no_interactive=args.no_interactive,
            enable_https=not args.no_https,  # Invert no_https to enable_https
            fastled_path=Path(args.fastled_path) if args.fastled_path else None,
        )
