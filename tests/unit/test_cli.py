"""
Unit test file for native compilation CLI.
"""

import os
import subprocess
import unittest
from pathlib import Path

import pytest  # type: ignore[reportMissingImports]

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


@pytest.mark.skipif(
    os.environ.get("CI") == "true",
    reason="Requires full WASM toolchain (emscripten + esbuild)",
)
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
                check=False,
            )
            ok = cp.returncode == 0
            if not ok:
                self.fail(f"Command failed with return code {cp.returncode}")
        finally:
            os.chdir(original_dir)


if __name__ == "__main__":
    unittest.main()
