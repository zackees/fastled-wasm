# -*- coding: utf-8 -*-
"""
Regression test for https://github.com/zackees/fastled-wasm/issues/12

When a sketch is nested like examples/Fx/FxCylon, the --example argument
passed to wasm_build.py must be "Fx/FxCylon", not just "FxCylon".
"""

import tempfile
from pathlib import Path

from fastled.toolchain.emscripten import EmscriptenToolchain, _resolve_example_name
from fastled.types import BuildMode


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


def test_compile_via_wasm_build_preserves_sketch_cache(monkeypatch):
    """The internal builder must preserve the per-sketch cache directory."""
    with tempfile.TemporaryDirectory() as tmpdir:
        fastled_dir = Path(tmpdir) / "FastLED"
        sketch_dir = fastled_dir / "examples" / "LuminescentGrand"
        output_dir = sketch_dir / "fastled_js"
        cache_dir = sketch_dir / ".build" / "wasm"

        (fastled_dir / "ci").mkdir(parents=True)
        sketch_dir.mkdir(parents=True)
        output_dir.mkdir(parents=True)
        cache_dir.mkdir(parents=True)
        (sketch_dir / "LuminescentGrand.ino").write_text("// sketch\n")
        cache_marker = cache_dir / "cache-marker.txt"
        cache_marker.write_text("keep me\n")

        calls: list[tuple[str, object]] = []

        def fake_configure(project_root):
            calls.append(("configure", project_root))

        def fake_build(*, example, output, mode, verbose=False, force=False):
            calls.append(("build", (example, output, mode, verbose, force)))
            return 0

        monkeypatch.setattr(
            "fastled.toolchain.internal_wasm_build.configure_project_root",
            fake_configure,
        )
        monkeypatch.setattr(
            "fastled.toolchain.internal_wasm_build.build",
            fake_build,
        )

        toolchain = EmscriptenToolchain(fastled_path=fastled_dir)
        output_js = toolchain._compile_via_wasm_build(
            sketch_dir=sketch_dir,
            output_dir=output_dir,
            fastled_dir=fastled_dir,
            build_mode=BuildMode.QUICK,
        )

        assert output_js == output_dir / "fastled.js"
        assert cache_marker.exists()
        assert calls, "expected internal wasm builder to be invoked"
