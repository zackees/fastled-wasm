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
        self.assertEqual(
            stdout.decode("utf-8").strip(),
            __version__,
            f"Version mismatch: {stdout.decode('utf-8').strip()} != {__version__}",
        )


if __name__ == "__main__":
    unittest.main()
