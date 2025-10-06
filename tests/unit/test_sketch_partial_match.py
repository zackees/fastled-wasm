"""
Unit tests for find_sketch_by_partial_name to ensure correct partial matching behavior.
"""

import unittest
from pathlib import Path
from unittest.mock import patch

from fastled.sketch import find_sketch_by_partial_name


class TestFindSketchByPartialName(unittest.TestCase):
    """Test find_sketch_by_partial_name behavior."""

    @patch("fastled.sketch.find_sketch_directories")
    def test_single_match_returns_path(self, mock_find):
        """Single unique match should return the matching path."""
        mock_find.return_value = [
            Path("examples/sketch1"),
            Path("examples/sketch2"),
        ]
        result = find_sketch_by_partial_name("sketch2")
        self.assertEqual(Path("examples/sketch2"), result)

    @patch("fastled.sketch.find_sketch_directories")
    def test_no_match_raises_error(self, mock_find):
        """No matches should raise ValueError."""
        mock_find.return_value = [
            Path("examples/sketch1"),
            Path("examples/sketch2"),
        ]
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("nonexistent")
        self.assertIn("No sketch directory found", str(context.exception))

    @patch("fastled.sketch.find_sketch_directories")
    def test_multiple_matches_raises_error(self, mock_find):
        """Multiple matches should raise ValueError with list of matches."""
        mock_find.return_value = [
            Path("examples/sketch1"),
            Path("examples/sketch2"),
            Path("more/sketch3"),
        ]
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("sketch")
        error_msg = str(context.exception)
        self.assertIn("Multiple sketch directories found", error_msg)
        # Check using normalized paths (cross-platform)
        self.assertIn("sketch1", error_msg)
        self.assertIn("sketch2", error_msg)

    @patch("fastled.sketch.find_sketch_directories")
    def test_case_insensitive_match(self, mock_find):
        """Match should be case insensitive."""
        mock_find.return_value = [
            Path("examples/MySketch"),
            Path("examples/OtherSketch"),
        ]
        result = find_sketch_by_partial_name("mysketch")
        self.assertEqual(Path("examples/MySketch"), result)

    @patch("fastled.sketch.find_sketch_directories")
    def test_partial_path_match(self, mock_find):
        """Should match partial paths, not just directory names."""
        mock_find.return_value = [
            Path("examples/path/sketch1"),
            Path("examples/other/sketch2"),
        ]
        result = find_sketch_by_partial_name("path/sketch1")
        self.assertEqual(Path("examples/path/sketch1"), result)

    @patch("fastled.sketch.find_sketch_directories")
    def test_best_match_exact_name(self, mock_find):
        """When multiple partial matches exist, exact directory name match wins."""
        mock_find.return_value = [
            Path("examples/sketch"),
            Path("examples/sketch1"),
            Path("examples/sketch2"),
        ]
        result = find_sketch_by_partial_name("sketch")
        self.assertEqual(Path("examples/sketch"), result)

    @patch("fastled.sketch.find_sketch_directories")
    def test_no_exact_match_with_multiple_partials_errors(self, mock_find):
        """Multiple partial matches with no exact match should error."""
        mock_find.return_value = [
            Path("examples/sketch1"),
            Path("examples/sketch2"),
        ]
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("sketch")
        self.assertIn("Multiple sketch directories found", str(context.exception))

    @patch("fastled.sketch.find_sketch_directories")
    def test_multiple_exact_matches_errors(self, mock_find):
        """Multiple exact matches should error."""
        mock_find.return_value = [
            Path("examples/sketch"),
            Path("more/sketch"),
        ]
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("sketch")
        self.assertIn("Multiple sketch directories found", str(context.exception))

    @patch("fastled.sketch.find_sketch_directories")
    def test_low_character_similarity_shows_available_sketches(self, mock_find):
        """When search term has low character similarity, show available sketches."""
        mock_find.return_value = [
            Path("path/to/sketch"),
            Path("examples/MyProject"),
        ]
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("blah")
        error_msg = str(context.exception)
        self.assertIn("does not look like any of the available sketches", error_msg)
        self.assertIn("Available sketches:", error_msg)
        self.assertIn("sketch", error_msg)
        self.assertIn("MyProject", error_msg)

    @patch("fastled.sketch.find_sketch_directories")
    def test_substring_match_with_low_similarity_rejected(self, mock_find):
        """Substring match with low character similarity should be rejected."""
        mock_find.return_value = [
            Path("qrs/tuv"),
        ]
        # "ab" has chars {a, b}, "qrs/tuv" has chars {q, r, s, /, t, u, v}
        # No common characters, so similarity = 0/2 = 0% < 50%
        with self.assertRaises(ValueError) as context:
            find_sketch_by_partial_name("ab")
        error_msg = str(context.exception)
        self.assertIn("does not look like any of the available sketches", error_msg)

    @patch("fastled.sketch.find_sketch_directories")
    def test_sufficient_similarity_allows_match(self, mock_find):
        """Match with sufficient character similarity should succeed."""
        mock_find.return_value = [
            Path("examples/sketch123"),
        ]
        # "sketch" has all chars in "examples/sketch123" (100% similarity)
        result = find_sketch_by_partial_name("sketch")
        self.assertEqual(Path("examples/sketch123"), result)


if __name__ == "__main__":
    unittest.main()
