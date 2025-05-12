"""
Unit test file.
"""

import os
import subprocess
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
        # rtn = os.system(COMMAND)
        cp: subprocess.CompletedProcess = subprocess.run(
            COMMAND,
            shell=True,
            capture_output=True,
            text=True,
            check=False,
        )
        ok = cp.returncode == 0
        if not ok:
            error_msg = "stdout:\n" + cp.stdout + "\nstderr:\n" + cp.stderr
            # self.assertEqual(0, cp.returncode, "Command failed: " + error_msg)
            self.fail(f"Command failed with return code {cp.returncode}:\n{error_msg}")


if __name__ == "__main__":
    unittest.main()
