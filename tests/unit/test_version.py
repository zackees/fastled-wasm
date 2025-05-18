"""
Unit test file.
"""

import subprocess
import unittest

from fastled import __version__

COMMAND = "fastled --version"


class MainTester(unittest.TestCase):
    """Main tester class."""

    def test_command(self) -> None:
        """Test command line interface (CLI)."""
        stdout = subprocess.check_output(COMMAND, shell=True)
        version_stdout = stdout.decode("utf-8").strip()
        self.assertEqual(
            version_stdout,
            __version__,
            f"FastLED Version mismatch: {version_stdout} (tool output) != {__version__} (package version)",
        )


if __name__ == "__main__":
    unittest.main()
