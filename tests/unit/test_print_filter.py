"""
Unit test file.
"""

import unittest

from fastled.print_filter import PrintFilterFastled


class PrintFitlerTester(unittest.TestCase):
    """Main tester class."""

    def test_print_filter(self) -> None:
        """Tests that a project can be filtered"""
        # Test the PrintFilter class
        pf = PrintFilterFastled(echo=False)
        pf.print("# WASM is building")  # This should trigger the filter.
        result = pf.print(
            "5.36 src/XYPath.ino.cpp:4:1: error: unknown type name 'kdsjfsdkfjsd'"
        )  # This should now be transformed.
        self.assertNotIn(".ino.cpp", result, "Expected .ino.cpp to be filtered out")
        self.assertIn(
            "examples/XYPath/XYPath.ino", result, "Expected path to be transformed"
        )


if __name__ == "__main__":
    unittest.main()
