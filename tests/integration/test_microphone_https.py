"""
Validation tests for the microphone demo HTML file.

These tests verify that micdemo.html exists and has the expected content
(getUserMedia API usage, error handling, etc.).

Note: HTTPS-specific browser integration tests were removed after the
HTTPS server was dropped in favour of HTTP-only (PR #48).  Localhost is
a secure context in all modern browsers, so the microphone API works
over plain HTTP on localhost.
"""

import unittest
from pathlib import Path


class MicrophoneDemoValidationTest(unittest.TestCase):
    """Tests for the microphone demo HTML file."""

    def test_micdemo_html_exists(self):
        """Verify that the microphone demo HTML file exists."""
        demo_path = Path(__file__).parent.parent.parent / "demo" / "micdemo.html"
        self.assertTrue(
            demo_path.exists(), f"Microphone demo file should exist at {demo_path}"
        )

    def test_micdemo_uses_getusermedia(self):
        """Verify that micdemo.html uses the getUserMedia API."""
        demo_path = Path(__file__).parent.parent.parent / "demo" / "micdemo.html"

        if not demo_path.exists():
            self.skipTest("micdemo.html not found")

        content = demo_path.read_text()

        # Check for getUserMedia API usage
        self.assertIn("getUserMedia", content, "Demo should use getUserMedia API")
        self.assertIn(
            "navigator.mediaDevices", content, "Demo should use navigator.mediaDevices"
        )

        # Check for audio access
        self.assertIn("audio", content, "Demo should request audio access")

    def test_micdemo_has_error_handling(self):
        """Verify that micdemo.html has proper error handling."""
        demo_path = Path(__file__).parent.parent.parent / "demo" / "micdemo.html"

        if not demo_path.exists():
            self.skipTest("micdemo.html not found")

        content = demo_path.read_text()

        # Check for error handling
        self.assertIn("catch", content, "Demo should have error handling")
        self.assertIn("Error", content, "Demo should handle errors")


if __name__ == "__main__":
    unittest.main()
