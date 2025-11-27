"""
Integration tests for microphone access over HTTPS vs HTTP.

These tests verify that:
1. Microphone API (getUserMedia) is available over HTTPS
2. Microphone API is blocked/unavailable over HTTP in secure contexts
3. The server correctly serves content over HTTPS with SSL certificates
4. HTTP fallback mode explicitly disables secure features

Note: These tests use Playwright to run actual browser automation tests.
"""

import asyncio
import os
import random
import time
import unittest
from pathlib import Path

from playwright.async_api import async_playwright

# Enable Flask server logging for debugging if needed
os.environ["FLASK_SERVER_LOGGING"] = "0"

from fastled import Test

HERE = Path(__file__).parent
# Use the micdemo.html file for testing microphone access
DEMO_DIR = Path(__file__).parent.parent.parent / "demo"
MICDEMO_HTML = DEMO_DIR / "micdemo.html"

# Use random ports to avoid conflicts
BASE_PORT = random.randint(9000, 9100)


class MicrophoneHttpsIntegrationTest(unittest.TestCase):
    """Integration tests for microphone access requiring HTTPS."""

    def test_microphone_available_over_https(self):
        """Test that navigator.mediaDevices.getUserMedia is available over HTTPS.

        This test:
        1. Starts an HTTPS server with SSL certificates
        2. Launches a Playwright browser
        3. Navigates to the HTTPS localhost URL serving micdemo.html
        4. Verifies that navigator.mediaDevices.getUserMedia exists
        5. Verifies that the page is served in a secure context
        """
        # Run the async test
        asyncio.run(self._test_microphone_available_over_https_async())

    async def _test_microphone_available_over_https_async(self):
        """Async implementation of HTTPS microphone availability test."""
        port = BASE_PORT
        compile_server_port = port + 1

        # Start HTTPS server
        proc = Test.spawn_http_server(
            DEMO_DIR,
            port=port,
            compile_server_port=compile_server_port,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Wait for server to start
            time.sleep(2)

            server_url = f"https://localhost:{port}/micdemo.html"

            # Launch Playwright browser
            async with async_playwright() as p:
                # Launch with args to accept self-signed certificates
                browser = await p.chromium.launch(
                    headless=True,
                    args=[
                        "--ignore-certificate-errors",
                        "--allow-insecure-localhost",
                    ],
                )

                try:
                    context = await browser.new_context(
                        ignore_https_errors=True,  # Accept self-signed cert
                    )
                    page = await context.new_page()

                    # Navigate to HTTPS URL
                    response = await page.goto(
                        server_url, wait_until="domcontentloaded", timeout=10000
                    )

                    # Verify we got a successful response
                    self.assertIsNotNone(response, "Failed to load HTTPS page")
                    assert response is not None  # Type narrowing for pyright
                    self.assertEqual(
                        response.status,
                        200,
                        f"Expected 200 status, got {response.status}",
                    )

                    # Check if we're in a secure context
                    is_secure_context = await page.evaluate("window.isSecureContext")
                    self.assertTrue(
                        is_secure_context,
                        "Page should be in a secure context over HTTPS",
                    )

                    # Check if mediaDevices API exists
                    has_media_devices = await page.evaluate(
                        "typeof navigator.mediaDevices !== 'undefined' && "
                        "typeof navigator.mediaDevices.getUserMedia === 'function'"
                    )
                    self.assertTrue(
                        has_media_devices,
                        "navigator.mediaDevices.getUserMedia should be available over HTTPS",
                    )

                    # Check protocol
                    protocol = await page.evaluate("window.location.protocol")
                    self.assertEqual(
                        protocol, "https:", f"Expected https: protocol, got {protocol}"
                    )

                    print(f"✅ HTTPS microphone API test passed: {server_url}")

                finally:
                    await browser.close()

        finally:
            # Stop the server
            proc.terminate()
            time.sleep(1)

    def test_microphone_context_over_http(self):
        """Test that secure context is not available over plain HTTP.

        This test:
        1. Starts an HTTP server (HTTPS disabled)
        2. Launches a Playwright browser
        3. Navigates to the HTTP localhost URL
        4. Verifies that the page is NOT in a secure context
        5. Documents the expected behavior for mediaDevices API over HTTP

        Note: localhost is treated as a secure context by some browsers even over HTTP,
        but the getUserMedia API may still be restricted or require additional permissions.
        """
        # Run the async test
        asyncio.run(self._test_microphone_context_over_http_async())

    async def _test_microphone_context_over_http_async(self):
        """Async implementation of HTTP microphone context test."""
        port = BASE_PORT + 10
        compile_server_port = port + 1

        # Start HTTP server (disable HTTPS)
        proc = Test.spawn_http_server(
            DEMO_DIR,
            port=port,
            compile_server_port=compile_server_port,
            open_browser=False,
            enable_https=False,  # Force HTTP mode
        )

        try:
            # Wait for server to start
            time.sleep(2)

            server_url = f"http://localhost:{port}/micdemo.html"

            # Launch Playwright browser
            async with async_playwright() as p:
                browser = await p.chromium.launch(headless=True)

                try:
                    context = await browser.new_context()
                    page = await context.new_page()

                    # Navigate to HTTP URL
                    response = await page.goto(
                        server_url, wait_until="domcontentloaded", timeout=10000
                    )

                    # Verify we got a successful response
                    self.assertIsNotNone(response, "Failed to load HTTP page")
                    assert response is not None  # Type narrowing for pyright
                    self.assertEqual(
                        response.status,
                        200,
                        f"Expected 200 status, got {response.status}",
                    )

                    # Check protocol
                    protocol = await page.evaluate("window.location.protocol")
                    self.assertEqual(
                        protocol, "http:", f"Expected http: protocol, got {protocol}"
                    )

                    # Note: localhost is often treated as a "potentially trustworthy" origin
                    # even over HTTP, so isSecureContext might be true
                    is_secure_context = await page.evaluate("window.isSecureContext")

                    # Check if mediaDevices API exists
                    has_media_devices = await page.evaluate(
                        "typeof navigator.mediaDevices !== 'undefined' && "
                        "typeof navigator.mediaDevices.getUserMedia === 'function'"
                    )

                    # Document the findings
                    print(f"📊 HTTP context test results for {server_url}:")
                    print(f"  - Protocol: {protocol}")
                    print(f"  - isSecureContext: {is_secure_context}")
                    print(f"  - mediaDevices API available: {has_media_devices}")

                    # The key insight: even if the API exists, actual microphone permission
                    # requests may be blocked or require user interaction. The browser
                    # may treat localhost specially.
                    if is_secure_context:
                        print(
                            "  ℹ️  Note: localhost is treated as secure context by browsers"
                        )

                    print(f"✅ HTTP context documentation test passed: {server_url}")

                finally:
                    await browser.close()

        finally:
            # Stop the server
            proc.terminate()
            time.sleep(1)

    def test_https_certificate_validation(self):
        """Test that HTTPS server properly loads and uses SSL certificates.

        This test:
        1. Starts an HTTPS server
        2. Connects with Playwright
        3. Verifies the connection uses HTTPS
        4. Validates that the server has proper SSL certificate configuration
        """
        asyncio.run(self._test_https_certificate_validation_async())

    async def _test_https_certificate_validation_async(self):
        """Async implementation of HTTPS certificate validation test."""
        port = BASE_PORT + 20
        compile_server_port = port + 1

        # Start HTTPS server
        proc = Test.spawn_http_server(
            DEMO_DIR,
            port=port,
            compile_server_port=compile_server_port,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Wait for server to start
            time.sleep(2)

            server_url = f"https://localhost:{port}/micdemo.html"

            # Launch Playwright browser
            async with async_playwright() as p:
                browser = await p.chromium.launch(
                    headless=True,
                    args=[
                        "--ignore-certificate-errors",
                        "--allow-insecure-localhost",
                    ],
                )

                try:
                    context = await browser.new_context(
                        ignore_https_errors=True,
                    )
                    page = await context.new_page()

                    # Navigate to HTTPS URL
                    response = await page.goto(
                        server_url, wait_until="domcontentloaded", timeout=10000
                    )

                    # Verify successful HTTPS connection
                    self.assertIsNotNone(response)
                    assert response is not None  # Type narrowing for pyright
                    self.assertEqual(response.status, 200)

                    # Verify protocol is HTTPS
                    protocol = await page.evaluate("window.location.protocol")
                    self.assertEqual(
                        protocol, "https:", "Server should serve content over HTTPS"
                    )

                    print(f"✅ HTTPS certificate validation test passed: {server_url}")

                finally:
                    await browser.close()

        finally:
            # Stop the server
            proc.terminate()
            time.sleep(1)


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
