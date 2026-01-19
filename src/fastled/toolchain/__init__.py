"""
FastLED Toolchain Module

Provides toolchain implementations for compiling FastLED sketches:
- EmscriptenToolchain: Compile to WebAssembly using Emscripten
"""

from fastled.toolchain.emscripten import EmscriptenToolchain

__all__ = ["EmscriptenToolchain"]
