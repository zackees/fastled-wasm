"""Compatibility exports for legacy FastLED toolchain imports.

WASM compilation is owned by the Rust backend. ``EmscriptenToolchain`` remains
as a thin facade for callers that still import it directly.
"""

from fastled.toolchain.emscripten import EmscriptenToolchain

__all__ = ["EmscriptenToolchain"]
