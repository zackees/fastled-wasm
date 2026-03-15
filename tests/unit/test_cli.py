"""
Unit test file for native compilation CLI.
"""

import os
import subprocess
import unittest
from pathlib import Path

import pytest

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class MainTester(unittest.TestCase):
    """Main tester class."""

    @pytest.mark.timeout(300)
    def test_command(self) -> None:
        """Test command line interface (CLI) with native compilation."""
        original_dir = os.getcwd()
        try:
            os.chdir(str(TEST_DIR))
            cp: subprocess.CompletedProcess = subprocess.run(
                "fastled --just-compile",
                shell=True,
                capture_output=True,
                check=False,
            )
            ok = cp.returncode == 0
            if not ok:
                stdout = cp.stdout.decode("utf-8", errors="replace")
                stderr = cp.stderr.decode("utf-8", errors="replace")
                error_msg = "stdout:\n" + stdout + "\nstderr:\n" + stderr
                self.fail(
                    f"Command failed with return code {cp.returncode}:\n{error_msg}"
                )
        finally:
            os.chdir(original_dir)


if __name__ == "__main__":
    unittest.main()
