"""
Unit tests for Playwright integration.
"""

import unittest
from unittest.mock import MagicMock, patch

from fastled.playwright_browser import is_playwright_available, open_with_playwright


class PlaywrightIntegrationTester(unittest.TestCase):
    """Test Playwright integration functionality."""

    def test_playwright_availability_check(self):
        """Test that the Playwright availability check works correctly."""
        # The availability check should return a boolean
        result = is_playwright_available()
        self.assertIsInstance(result, bool)

    def test_open_with_playwright_fallback(self):
        """Test that open_with_playwright falls back to webbrowser when Playwright is not available."""
        test_url = "http://localhost:8080"

        # Mock the webbrowser module and PLAYWRIGHT_AVAILABLE
        with patch("fastled.playwright_browser.PLAYWRIGHT_AVAILABLE", False):
            with patch("webbrowser.open") as mock_webbrowser_open:
                # Call the function
                proxy = open_with_playwright(test_url)

                # Verify that webbrowser.open was called as fallback
                mock_webbrowser_open.assert_called_once_with(test_url)

                # Verify that a proxy object was returned
                self.assertIsNotNone(proxy)

    def test_open_with_playwright_when_available(self):
        """Test that open_with_playwright uses Playwright when available."""
        test_url = "http://localhost:8080"

        # Mock the multiprocessing.Process and PLAYWRIGHT_AVAILABLE
        with patch("fastled.playwright_browser.PLAYWRIGHT_AVAILABLE", True):
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

    def test_args_integration(self):
        """Test that the playwright argument is properly integrated into the Args class."""
        import argparse

        from fastled.args import Args

        # Create a mock namespace with the playwright argument
        namespace = argparse.Namespace()
        namespace.directory = None
        namespace.init = None
        namespace.just_compile = False
        namespace.web = None
        namespace.interactive = False
        namespace.profile = False
        namespace.force_compile = False
        namespace.no_platformio = False
        namespace.no_auto_updates = False
        namespace.update = False
        namespace.localhost = False
        namespace.build = False
        namespace.server = False
        namespace.purge = False
        namespace.debug = False
        namespace.quick = False
        namespace.release = False
        namespace.ram_disk_size = "0"
        namespace.playwright = True

        # Create Args from namespace
        args = Args.from_namespace(namespace)

        # Verify the playwright flag is set
        self.assertTrue(args.playwright)

    def test_parse_args_playwright_flag(self):
        """Test that the --playwright flag is properly parsed."""
        import sys

        from fastled.parse_args import parse_args

        # Save original argv
        original_argv = sys.argv

        try:
            # Test with --playwright flag
            sys.argv = ["fastled", "test_dir", "--playwright"]
            args = parse_args()
            self.assertTrue(args.playwright)

            # Test without --playwright flag
            sys.argv = ["fastled", "test_dir"]
            args = parse_args()
            self.assertFalse(args.playwright)

        finally:
            # Restore original argv
            sys.argv = original_argv


if __name__ == "__main__":
    unittest.main()
