"""
Unit test file.
"""

import os
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).parent
PROJECT_ROOT = HERE.parent


class BuildExeTester(unittest.TestCase):
    """Main tester class."""

    def test_builder(self) -> None:
        os.chdir(PROJECT_ROOT)
        python_exe = sys.executable
        cmd = f"{python_exe} build_exe.py"
        # Running command
        # rtn = os.system(cmd)
        rtn = subprocess.run(cmd, shell=True, check=True).returncode
        self.assertEqual(0, rtn)


if __name__ == "__main__":
    unittest.main()
