"""Compatibility shim. Frontend bundling is implemented in Rust.

See ``crates/fastled-cli/src/frontend.rs`` for the implementation; this module
exists so existing callers (``fastled.toolchain.emscripten``,
``fastled.toolchain.internal_wasm_build``) keep working without churn.
"""

from __future__ import annotations

from pathlib import Path

from fastled._native import copy_frontend_to_output as _native_copy_frontend_to_output

FRONTEND_DIR = Path(__file__).resolve().parent / "frontend"


def copy_frontend_to_output(output_dir: Path, source_dir: Path | None = None) -> None:
    source = source_dir if source_dir is not None else FRONTEND_DIR
    _native_copy_frontend_to_output(str(output_dir), str(source))
