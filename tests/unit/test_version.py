"""
Unit test file.
"""

import subprocess
import sys
import unittest

from fastled import __version__

COMMAND = [sys.executable, "-m", "fastled", "--version"]


class MainTester(unittest.TestCase):
    """Main tester class."""

    def test_command(self) -> None:
        """Test command line interface (CLI).

        The Rust CLI emits clap's standard "<bin> <version>" format
        (e.g. "fastled 2.0.7"); assert the package __version__ is present
        in that output rather than requiring an exact match.
        """
        stdout = subprocess.check_output(COMMAND, stdin=subprocess.DEVNULL)
        version_stdout = stdout.decode("utf-8").strip()
        self.assertIn(
            __version__,
            version_stdout,
            f"FastLED Version mismatch: {version_stdout} (tool output) does not contain {__version__} (package version)",
        )


if __name__ == "__main__":
    unittest.main()
