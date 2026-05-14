"""
End-to-end compile test for the Animartrix example.

This test compiles ~/dev/fastled/examples/Animartrix using the native toolchain
and verifies the expected browser-ready output files are emitted.
"""

import os
import subprocess
import unittest
from pathlib import Path

import pytest  # type: ignore[reportMissingImports]

from fastled._rust_cli import find_rust_fastled_cli
from fastled.interrupts import handle_keyboard_interrupt

ANIMARTRIX_DIR = Path.home() / "dev" / "fastled" / "examples" / "Animartrix"
FASTLED_JS_DIR = ANIMARTRIX_DIR / "fastled_js"


class AnimartrixE2ETest(unittest.TestCase):
    """End-to-end test: compile Animartrix and check browser-ready output."""

    @pytest.mark.timeout(600)
    def test_animartrix_compiles_and_emits_browser_output(self) -> None:
        """Compile Animartrix and verify expected output artifacts exist."""
        if not ANIMARTRIX_DIR.exists():
            self.skipTest(f"Animartrix example not found at {ANIMARTRIX_DIR}")

        original_dir = os.getcwd()
        try:
            os.chdir(str(ANIMARTRIX_DIR))
            cli = find_rust_fastled_cli()
            if cli is None:
                self.skipTest("Rust fastled-rs CLI binary not found")
            cp = subprocess.run(
                [str(cli), "--just-compile"],
                capture_output=True,
                check=False,
                timeout=300,
            )
            if cp.returncode != 0:
                stdout = cp.stdout.decode("utf-8", errors="replace")
                stderr = cp.stderr.decode("utf-8", errors="replace")
                self.fail(
                    f"Compilation failed (rc={cp.returncode}):\n"
                    f"stdout:\n{stdout}\nstderr:\n{stderr}"
                )
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
            raise
        finally:
            os.chdir(original_dir)

        self.assertTrue(
            FASTLED_JS_DIR.exists(), f"Expected output dir {FASTLED_JS_DIR} to exist"
        )
        self.assertTrue(
            (FASTLED_JS_DIR / "index.html").exists(), "Expected index.html in output"
        )
        self.assertTrue(
            (FASTLED_JS_DIR / "fastled.wasm").exists(),
            "Expected fastled.wasm in output",
        )

        print(f"Animartrix compile test passed: {FASTLED_JS_DIR}")


if __name__ == "__main__":
    unittest.main()
