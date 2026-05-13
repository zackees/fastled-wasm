"""Compatibility entry point that forwards to the native Rust CLI."""

import sys

from fastled._rust_cli import invoke_rust_fastled_cli


def main() -> int:
    """Run the native Rust CLI with the current argv."""
    return invoke_rust_fastled_cli(sys.argv[1:])


if __name__ == "__main__":
    sys.exit(main())
