"""
Unit test for HTTPS server functionality.

This test verifies that the local Flask server is properly configured
to serve content over HTTPS with SSL certificates.
"""

import os
import random
import time
import unittest
from pathlib import Path

import httpx

# Enable Flask server logging for debugging if needed
os.environ["FLASK_SERVER_LOGGING"] = "0"

from fastled import Test

HERE = Path(__file__).parent
INDEX_HTML = HERE / "html" / "index.html"
TIMEOUT = 30

assert INDEX_HTML.exists()

# Use random port to avoid conflicts
PORT = random.randint(8700, 8800)
COMPILE_SERVER_PORT = PORT + 1


class HttpsServerTester(unittest.TestCase):
    """Test HTTPS server functionality."""

    def test_https_server_status(self) -> None:
        """Test that the server is running with HTTPS and returns 200 status."""
        port = PORT
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Test HTTPS connection (verify=False for self-signed certificates)
            https_response = httpx.get(
                f"https://localhost:{port}", timeout=TIMEOUT, verify=False
            )
            self.assertEqual(https_response.status_code, 200)
            print(f"✓ HTTPS server returned status code: {https_response.status_code}")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_https_connection_properties(self) -> None:
        """Test HTTPS connection properties and SSL certificate usage."""
        port = PORT + 2
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 2,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Create a client that accepts self-signed certificates
            with httpx.Client(verify=False) as client:
                response = client.get(f"https://localhost:{port}", timeout=TIMEOUT)

                # Verify response is successful
                self.assertEqual(response.status_code, 200)

                # Verify we're using HTTPS (not HTTP)
                self.assertTrue(response.url.scheme == "https")
                print(f"✓ Server is using HTTPS protocol: {response.url.scheme}")

                # Verify localhost
                self.assertIn("localhost", str(response.url))
                print("✓ Server is running on localhost")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_https_with_security_headers(self) -> None:
        """Test that HTTPS server includes security headers."""
        port = PORT + 3
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 3,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Give server time to start
            time.sleep(2)

            response = httpx.get(
                f"https://localhost:{port}", timeout=TIMEOUT, verify=False
            )
            self.assertEqual(response.status_code, 200)

            # Check that security headers are present
            headers = response.headers

            # Cross-origin isolation headers (required for SharedArrayBuffer)
            self.assertIn("Cross-Origin-Embedder-Policy", headers)
            self.assertIn("Cross-Origin-Opener-Policy", headers)
            print("✓ Security headers present on HTTPS connection")

            # CORS headers should also be present
            self.assertIn("Access-Control-Allow-Origin", headers)
            print("✓ CORS headers present on HTTPS connection")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_https_file_serving(self) -> None:
        """Test that files are properly served over HTTPS."""
        port = PORT + 4
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 4,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Request the index.html file over HTTPS
            response = httpx.get(
                f"https://localhost:{port}/index.html", timeout=TIMEOUT, verify=False
            )
            self.assertEqual(response.status_code, 200)

            # Verify content type is set
            self.assertIn("Content-Type", response.headers)
            self.assertIn("text/html", response.headers["Content-Type"])
            print("✓ Files are properly served over HTTPS with correct content type")

            # Verify we got actual content
            self.assertGreater(len(response.content), 0)
            print(f"✓ HTTPS response contains content ({len(response.content)} bytes)")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_https_404_response(self) -> None:
        """Test that HTTPS server returns 404 for non-existent files."""
        port = PORT + 5
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 5,
            open_browser=False,
            enable_https=True,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Request a non-existent file over HTTPS
            response = httpx.get(
                f"https://localhost:{port}/nonexistent.html",
                timeout=TIMEOUT,
                verify=False,
            )
            self.assertEqual(response.status_code, 404)
            print("✓ HTTPS server properly returns 404 for missing files")

        finally:
            proc.terminate()
            time.sleep(1)


if __name__ == "__main__":
    unittest.main()
