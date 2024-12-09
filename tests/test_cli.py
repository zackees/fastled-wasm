"""
Unit test file.
"""

import os
import unittest
from pathlib import Path

COMMAND = "fastled --just-compile"

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class MainTester(unittest.TestCase):
    """Main tester class."""

    def test_command(self) -> None:
        """Test command line interface (CLI)."""
        os.chdir(str(TEST_DIR))
        rtn = os.system(COMMAND)
        self.assertEqual(0, rtn)


if __name__ == "__main__":
    unittest.main()
