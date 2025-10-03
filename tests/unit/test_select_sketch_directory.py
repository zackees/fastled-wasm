"""
Unit tests for select_sketch_directory to prevent regressions.
"""

import unittest
from pathlib import Path
from unittest.mock import patch

from fastled.select_sketch_directory import select_sketch_directory


class TestSelectSketchDirectory(unittest.TestCase):
    """Test select_sketch_directory behavior."""

    def test_single_directory_auto_selects(self):
        """Single directory should auto-select without prompting."""
        sketch_dirs = [Path("sketch1")]
        result = select_sketch_directory(sketch_dirs, cwd_is_fastled=False)
        self.assertEqual(str(sketch_dirs[0]), result)

    def test_first_scan_with_more_than_4_returns_none(self):
        """First scan with >4 directories should return None (no auto-select)."""
        sketch_dirs = [Path(f"sketch{i}") for i in range(5)]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=False
        )
        self.assertIsNone(result)

    @patch("builtins.input", return_value="")
    def test_first_scan_with_4_or_less_prompts_with_default(self, mock_input):
        """First scan with ≤4 directories should prompt with default selection."""
        sketch_dirs = [Path(f"sketch{i}") for i in range(4)]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=False
        )
        # User pressed return (empty input), should get default (first option)
        self.assertEqual(str(sketch_dirs[0]), result)
        mock_input.assert_called_once()

    @patch("builtins.input", return_value="2")
    def test_first_scan_user_selects_by_number(self, mock_input):
        """User can select by number on first scan with ≤4 directories."""
        sketch_dirs = [Path(f"sketch{i}") for i in range(4)]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=False
        )
        self.assertEqual(str(sketch_dirs[1]), result)
        mock_input.assert_called_once()

    @patch("builtins.input", return_value="")
    def test_followup_always_prompts_even_with_many_dirs(self, mock_input):
        """Follow-up calls should always prompt, even with >4 directories."""
        sketch_dirs = [Path(f"sketch{i}") for i in range(10)]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=True
        )
        # Should prompt and return default (first option)
        self.assertEqual(str(sketch_dirs[0]), result)
        mock_input.assert_called_once()

    @patch("builtins.input", return_value="5")
    def test_followup_user_can_select_from_many(self, mock_input):
        """Follow-up calls allow user to select from many directories."""
        sketch_dirs = [Path(f"sketch{i}") for i in range(10)]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=True
        )
        self.assertEqual(str(sketch_dirs[4]), result)
        mock_input.assert_called_once()

    def test_fastled_repo_excludes_src_dev_tests(self):
        """When in FastLED repo, should exclude src, dev, tests directories."""
        sketch_dirs = [Path("src"), Path("dev"), Path("tests"), Path("examples")]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=True, is_followup=False
        )
        # Only "examples" remains, should auto-select
        self.assertEqual("examples", result)

    @patch("builtins.input", return_value="sketch3")
    def test_user_can_select_by_name(self, mock_input):
        """User can select by name instead of number."""
        sketch_dirs = [Path("sketch1"), Path("sketch2"), Path("sketch3")]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=False
        )
        self.assertEqual("sketch3", result)
        mock_input.assert_called_once()

    @patch("builtins.input", side_effect=["invalid", "1"])
    def test_invalid_input_retry(self, mock_input):
        """Invalid input should retry prompt."""
        sketch_dirs = [Path("sketch1"), Path("sketch2")]
        result = select_sketch_directory(
            sketch_dirs, cwd_is_fastled=False, is_followup=False
        )
        self.assertEqual(str(sketch_dirs[0]), result)
        # Should be called twice: once for invalid, once for valid
        self.assertEqual(mock_input.call_count, 2)


if __name__ == "__main__":
    unittest.main()
