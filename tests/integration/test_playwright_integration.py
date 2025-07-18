"""
Unit tests for Playwright integration.
"""

import unittest
from unittest.mock import MagicMock, patch

from fastled.playwright.playwright_browser import open_with_playwright


class PlaywrightIntegrationTester(unittest.TestCase):
    """Test Playwright integration functionality."""

    def test_open_with_playwright_fallback(self):
        """Test that open_with_playwright falls back to webbrowser when Playwright is not available."""
        test_url = "http://localhost:8089"

        # Mock multiprocessing.Process to raise an exception, simulating Playwright not being available
        with patch(
            "multiprocessing.Process", side_effect=Exception("Playwright not available")
        ):
            with patch("webbrowser.open") as mock_webbrowser_open:
                # Call the function
                proxy = open_with_playwright(test_url)

                # Verify that webbrowser.open was called as fallback
                mock_webbrowser_open.assert_called_once_with(test_url)

                # Verify that a proxy object was returned
                self.assertIsNotNone(proxy)

    def test_open_with_playwright_when_available(self):
        """Test that open_with_playwright uses Playwright when available."""
        test_url = "http://localhost:8089"

        # Mock the multiprocessing.Process to work normally (Playwright available)
        with patch("multiprocessing.Process") as mock_process_class:
            mock_process = MagicMock()
            mock_process_class.return_value = mock_process

            # Call the function
            proxy = open_with_playwright(test_url)

            # Verify that a new process was created and started
            mock_process_class.assert_called_once()
            mock_process.start.assert_called_once()

            # Verify that a proxy object was returned
            self.assertIsNotNone(proxy)

            # Test cleanup
            proxy.close()  # Should not raise an exception


if __name__ == "__main__":
    unittest.main()
