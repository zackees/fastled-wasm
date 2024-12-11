"""
Unit test file.
"""

import os
import unittest

from fastled.paths import PROJECT_ROOT


class BuildExeTester(unittest.TestCase):
    """Main tester class."""

    def test_builder(self) -> None:
        os.chdir(str(PROJECT_ROOT))
        rtn = os.system("python build_exe.py")
        self.assertEqual(0, rtn)


if __name__ == "__main__":
    unittest.main()
