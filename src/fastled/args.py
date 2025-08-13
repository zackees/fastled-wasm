import argparse
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Args:
    directory: Path | None
    init: bool | str
    just_compile: bool
    web: str | None
    interactive: bool
    profile: bool
    force_compile: bool
    no_platformio: bool
    app: bool  # New flag to trigger Playwright browser with browser download if needed
    auto_update: bool | None
    update: bool
    background_update: bool
    localhost: bool
    build: bool
    server: bool
    purge: bool
    debug: bool
    quick: bool
    release: bool
    ram_disk_size: str  # suffixed liked "25mb" or "1gb"
    clear = False  # Force the last running container to be removed. Useful for benchmarking.
    install: bool = False  # Install FastLED development environment
    dry_run: bool = False  # Dry run mode for testing
    no_interactive: bool = False  # Non-interactive mode
    emsdk_headers: str | None = None  # Path to export EMSDK headers ZIP

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
            args.no_platformio, bool
        ), f"expected bool, got {type(args.no_platformio)}"
        assert isinstance(args.app, bool), f"expected bool, got {type(args.app)}"
        assert isinstance(
            args.no_auto_updates, bool | None
        ), f"expected bool | None, got {type(args.no_auto_updates)}"
        assert isinstance(args.update, bool), f"expected bool, got {type(args.update)}"
        assert isinstance(
            args.background_update, bool
        ), f"expected bool, got {type(args.background_update)}"
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
            args.emsdk_headers, str | None
        ), f"expected str | None, got {type(args.emsdk_headers)}"

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
            no_platformio=args.no_platformio,
            app=args.app,
            auto_update=not args.no_auto_updates,
            update=args.update,
            background_update=args.background_update,
            localhost=args.localhost,
            build=args.build,
            server=args.server,
            purge=args.purge,
            debug=args.debug,
            quick=args.quick,
            release=args.release,
            ram_disk_size=args.ram_disk_size,
            install=args.install,
            dry_run=args.dry_run,
            emsdk_headers=args.emsdk_headers,
            no_interactive=args.no_interactive,
        )
