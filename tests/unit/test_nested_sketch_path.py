# -*- coding: utf-8 -*-
"""
Regression test for https://github.com/zackees/fastled-wasm/issues/12

When a sketch is nested like examples/Fx/FxCylon, the --example argument
passed to wasm_build.py must be "Fx/FxCylon", not just "FxCylon".
"""

import tempfile
from pathlib import Path

from fastled.toolchain.emscripten import _resolve_example_name


def test_nested_sketch_preserves_intermediate_directory():
    """examples/Fx/FxCylon -> example name should be 'Fx/FxCylon'."""
    with tempfile.TemporaryDirectory() as tmpdir:
        fastled_dir = Path(tmpdir) / "FastLED"
        nested_sketch = fastled_dir / "examples" / "Fx" / "FxCylon"
        nested_sketch.mkdir(parents=True)
        (nested_sketch / "FxCylon.ino").touch()

        name, example_dir, is_in_tree = _resolve_example_name(
            nested_sketch, fastled_dir
        )
        assert name == "Fx/FxCylon", f"Expected 'Fx/FxCylon', got '{name}'"
        assert example_dir == nested_sketch
        assert is_in_tree is True


def test_flat_sketch_still_works():
    """examples/FxCylon -> example name should be 'FxCylon'."""
    with tempfile.TemporaryDirectory() as tmpdir:
        fastled_dir = Path(tmpdir) / "FastLED"
        flat_sketch = fastled_dir / "examples" / "FxCylon"
        flat_sketch.mkdir(parents=True)
        (flat_sketch / "FxCylon.ino").touch()

        name, example_dir, is_in_tree = _resolve_example_name(flat_sketch, fastled_dir)
        assert name == "FxCylon", f"Expected 'FxCylon', got '{name}'"
        assert example_dir == flat_sketch
        assert is_in_tree is True


def test_external_sketch_uses_leaf_name():
    """Sketches outside the FastLED tree use leaf directory name."""
    with tempfile.TemporaryDirectory() as tmpdir:
        fastled_dir = Path(tmpdir) / "FastLED"
        (fastled_dir / "examples").mkdir(parents=True)

        ext_sketch = Path(tmpdir) / "MySketch"
        ext_sketch.mkdir()
        (ext_sketch / "MySketch.ino").touch()

        name, example_dir, is_in_tree = _resolve_example_name(ext_sketch, fastled_dir)
        assert name == "MySketch"
        assert example_dir == fastled_dir / "examples" / "MySketch"
        assert is_in_tree is False
