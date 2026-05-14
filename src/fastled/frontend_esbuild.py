"""Compatibility shim. Frontend bundling is implemented in Rust.

See ``crates/fastled-cli/src/frontend.rs`` for the implementation; this module
exists so existing Python callers can keep importing the old helper.
"""

from __future__ import annotations

from pathlib import Path

from fastled._native import copy_frontend_to_output as _native_copy_frontend_to_output

FRONTEND_DIR = Path(__file__).resolve().parent / "frontend"


def copy_frontend_to_output(output_dir: Path, source_dir: Path | None = None) -> None:
    source = source_dir if source_dir is not None else FRONTEND_DIR
    _native_copy_frontend_to_output(str(output_dir), str(source))
