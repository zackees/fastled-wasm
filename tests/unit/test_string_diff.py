import unittest

from fastled.string_diff import string_diff

_HAYSTACK: list[str] = [
    "examples/Wave",
    "examples/Wave2d",
    "examples/FxWave",
]





class StringDiffTester(unittest.TestCase):
    """Main tester class."""

    def test_needle_in_hastack(self) -> None:
        """Test if the needle is in the haystack."""
        result = string_diff("FxWave", _HAYSTACK)
        self.assertGreater(len(result), 0)
        _, path = result[0]
        self.assertEqual(path, "examples/FxWave")


if __name__ == "__main__":
    unittest.main()
