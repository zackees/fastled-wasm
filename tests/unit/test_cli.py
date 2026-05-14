"""
Unit test file for native compilation CLI.
"""

import os
import subprocess
import unittest
from pathlib import Path

import pytest  # type: ignore[reportMissingImports]

from fastled._rust_cli import find_rust_fastled_cli
from fastled.interrupts import handle_keyboard_interrupt

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
            cli = find_rust_fastled_cli()
            if cli is None:
                self.skipTest("Rust fastled-rs CLI binary not found")
            cp: subprocess.CompletedProcess[bytes] = subprocess.run(
                [str(cli), "--just-compile"],
                check=False,
                timeout=300,
            )
            ok = cp.returncode == 0
            if not ok:
                self.fail(f"Command failed with return code {cp.returncode}")
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
            raise
        finally:
            os.chdir(original_dir)


if __name__ == "__main__":
    unittest.main()
