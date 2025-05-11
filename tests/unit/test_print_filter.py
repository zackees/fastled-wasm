"""
Unit test file.
"""

import unittest

from fastled.print_filter import PrintFilter


class PrintFitlerTester(unittest.TestCase):
    """Main tester class."""

    def test_live_client(self) -> None:
        """Tests that a project can be filtered"""
        # Test the PrintFilter class
        pf = PrintFilter(echo=False)
        pf.print("# WASM is building")  # This should trigger the filter.
        result = pf.print("src/XYPath.ino.cpp")  # This should now be transformed.
        self.assertNotIn(".ino.cpp", result, "Expected .ino.cpp to be filtered out")
        self.assertIn(
            "examples/XYPath/XYPath.ino", result, "Expected path to be transformed"
        )


if __name__ == "__main__":
    unittest.main()
