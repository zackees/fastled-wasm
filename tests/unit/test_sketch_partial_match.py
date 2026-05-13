"""
Unit tests for find_sketch_by_partial_name to ensure correct partial matching behavior.

These tests exercise the native Rust implementation re-exported through
``fastled.sketch``. The Python-side fallback no longer exists (Stream A: Rust
is authoritative), so we use real on-disk fixtures rather than mocking the
search.
"""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled.sketch import find_sketch_by_partial_name


def _make_sketch(root: Path, relative: str) -> Path:
    """Create a minimal sketch directory containing an .ino file."""
    sketch_dir = root / relative
    sketch_dir.mkdir(parents=True, exist_ok=True)
    (sketch_dir / f"{sketch_dir.name}.ino").write_text("void setup(){}void loop(){}\n")
    return sketch_dir


class TestFindSketchByPartialName(unittest.TestCase):
    """Test find_sketch_by_partial_name behavior against the native resolver."""

    def test_single_match_returns_path(self) -> None:
        """A unique partial match should return the matching sketch."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/sketch1")
            _make_sketch(root, "examples/sketch2")
            result = find_sketch_by_partial_name("sketch2", root)
            self.assertTrue(result.match("**/sketch2"))

    def test_no_match_raises_error(self) -> None:
        """No matches should raise ValueError."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/sketch1")
            _make_sketch(root, "examples/sketch2")
            with self.assertRaises(ValueError):
                find_sketch_by_partial_name("nonexistent", root)

    def test_multiple_matches_raises_error(self) -> None:
        """An ambiguous partial match should raise ValueError listing matches."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/sketch1")
            _make_sketch(root, "examples/sketch2")
            _make_sketch(root, "more/sketch3")
            with self.assertRaises(ValueError) as context:
                find_sketch_by_partial_name("sketch", root)
            error_msg = str(context.exception)
            self.assertIn("sketch1", error_msg)
            self.assertIn("sketch2", error_msg)

    def test_case_insensitive_match(self) -> None:
        """Match should be case insensitive."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/MySketch")
            _make_sketch(root, "examples/OtherSketch")
            result = find_sketch_by_partial_name("mysketch", root)
            self.assertTrue(result.match("**/MySketch"))

    def test_partial_path_match(self) -> None:
        """Partial path components should match."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/path/sketch1")
            _make_sketch(root, "examples/other/sketch2")
            result = find_sketch_by_partial_name("path/sketch1", root)
            self.assertTrue(result.match("**/path/sketch1"))

    def test_best_match_exact_name(self) -> None:
        """When multiple partial matches exist, an exact directory name wins."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/sketch")
            _make_sketch(root, "examples/sketch1")
            _make_sketch(root, "examples/sketch2")
            result = find_sketch_by_partial_name("sketch", root)
            self.assertEqual("sketch", result.name)

    def test_multiple_exact_matches_errors(self) -> None:
        """Multiple exact directory-name matches should error."""
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            _make_sketch(root, "examples/group1/sketch")
            _make_sketch(root, "examples/group2/sketch")
            with self.assertRaises(ValueError):
                find_sketch_by_partial_name("sketch", root)


if __name__ == "__main__":
    unittest.main()
