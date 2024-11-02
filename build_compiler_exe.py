
import argparse

from fastled_wasm.build_compiler_exe import setup_docker2exe

ARCH_CHOICES = [
    "amd64",
    "arm64",
]

def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build compilers script.")
    parser.add_argument("--arch", type=str, help="Docker password", required=True, choices=ARCH_CHOICES)
    return parser.parse_args()

def main() -> None:
    args = _parse_args()
    setup_docker2exe(args.arch)


if __name__ == "__main__":
    main()