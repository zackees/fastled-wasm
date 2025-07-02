import argparse
import os
import subprocess
import sys
import warnings
from pathlib import Path
from typing import Tuple

# ----------------------------------------------------------------------------
# The fastled-wasm Docker image sometimes ships with an older version of the
# fastled-wasm-compiler PyPI package that predates the introduction of the
# `Compiler` class. In that case importing `Compiler` raises an ImportError
# which causes the whole server to crash very early during start-up:
#
#   ImportError: cannot import name 'Compiler' from 'fastled_wasm_compiler.compiler'
#
# To make the container self-healing we try the import, and if it fails we
# perform an in-place upgrade of the fastled-wasm-compiler package then retry
# the import once more.  If it still fails we re-raise the original error.
# ----------------------------------------------------------------------------

# NOTE: This code intentionally uses the standard `python -m pip` invocation
# instead of `uv` because `uv` is not guaranteed to be present in the Docker
# image at runtime. The rule to always use `uv run` applies to development on
# the host; inside the container we fall back to the ubiquitous `pip` tool.

import importlib
import subprocess
import sys


def _import_compiler():
    """Attempt to import the Compiler class, returning it on success."""
    from fastled_wasm_compiler import Compiler  # type: ignore

    return Compiler


try:
    # First attempt: the vast majority of cases should work here.
    Compiler = _import_compiler()
except ImportError as _original_exc:  # pragma: no cover – only runs in broken images
    # Perform a best-effort, in-place upgrade of the package and retry.
    try:
        print("fastled_wasm_compiler missing `Compiler` symbol – attempting self-upgrade …")
        subprocess.check_call(
            [
                sys.executable,
                "-m",
                "pip",
                "install",
                "--upgrade",
                "fastled-wasm-compiler",
            ]
        )

        # Invalidate import caches so that Python sees the freshly installed version.
        importlib.invalidate_caches()

        # Retry the import now that we've potentially upgraded the package.
        Compiler = _import_compiler()
        print("Successfully upgraded fastled_wasm_compiler – proceeding with startup.")
    except Exception as _upgrade_exc:  # pragma: no cover
        # If anything goes wrong we fall back to re-raising the original
        # ImportError so that the stack trace clearly shows the root cause.
        print(
            "Failed to upgrade fastled_wasm_compiler automatically. "
            "Original error follows:")
        raise _original_exc from _upgrade_exc

from fastled_wasm_compiler.paths import VOLUME_MAPPED_SRC

_PORT = os.environ.get("PORT", 80)

_CHOICES = ["compile", "server"]

HERE = Path(__file__).parent


def _parse_args() -> Tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser(
        description="Run compile.py with additional arguments"
    )
    parser.add_argument(
        "mode", help="Which mode does this script run in", choices=_CHOICES
    )
    return parser.parse_known_args()


def _run_server(unknown_args: list[str]) -> int:
    env = os.environ.copy()
    if "--disable-auto-clean" in unknown_args:
        env["DISABLE_AUTO_CLEAN"] = "1"
        unknown_args.remove("--disable-auto-clean")
    if "--allow-shutdown" in unknown_args:
        env["ALLOW_SHUTDOWN"] = "1"
        unknown_args.remove("--allow-shutdown")
    if "--no-auto-update" in unknown_args:
        env["NO_AUTO_UPDATE"] = "1"
        unknown_args.remove("--no-auto-update")
    if "--no-sketch-cache" in unknown_args:
        env["NO_SKETCH_CACHE"] = "1"
        unknown_args.remove("--no-sketch-cache")
    if unknown_args:
        warnings.warn(f"Unknown arguments: {unknown_args}")
        unknown_args = []
    cmd_list = [
        "uvicorn",
        "fastled_wasm_server.server:app",
        "--host",
        "0.0.0.0",
        "--workers",
        "1",
        "--port",
        f"{_PORT}",
    ]
    cp: subprocess.CompletedProcess = subprocess.run(cmd_list, cwd=str(HERE), env=env)
    return cp.returncode


def _run_compile(unknown_args: list[str]) -> int:

    # Construct the command to call compile.py with unknown arguments
    command = [sys.executable, "compile.py"] + unknown_args

    # Call compile.py with the unknown arguments
    result = subprocess.run(command, text=True, cwd=str(HERE))

    # Print the output from compile.py
    # print(result.stdout)
    # if result.stderr:
    #    print(result.stderr, file=sys.stderr)
    return result.returncode


def main() -> int:
    print("Running...")
    args, unknown_args = _parse_args()
    compiler = Compiler(
        volume_mapped_src=VOLUME_MAPPED_SRC,
    )
    compiler.update_src()

    try:
        if args.mode == "compile":
            warnings.warn(
                "The compile mode is deprecated and may fail. Use server mode instead."
            )
            rtn = _run_compile(unknown_args)
            return rtn
        elif args.mode == "server":
            rtn = _run_server(unknown_args)
            return rtn
        raise ValueError(f"Unknown mode: {args.mode}")
    except KeyboardInterrupt:
        print("Exiting...")
        return 1


if __name__ == "__main__":
    sys.exit(main())
