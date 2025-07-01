"""
Test Flask server HTTP headers including CORS and cross-origin isolation headers.
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
PORT = random.randint(8500, 8600)
COMPILE_SERVER_PORT = PORT + 1


class FlaskHeadersTester(unittest.TestCase):
    """Test Flask server HTTP headers."""

    def test_cors_headers(self) -> None:
        """Test that CORS headers are properly set."""
        port = PORT
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT,
            open_browser=False,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Test with OPTIONS request (preflight request)
            options_response = httpx.options(
                f"http://localhost:{port}", timeout=TIMEOUT
            )

            # Check CORS headers
            self.assertIn("Access-Control-Allow-Origin", options_response.headers)
            self.assertEqual(
                options_response.headers["Access-Control-Allow-Origin"], "*"
            )

            # Test with regular GET request
            get_response = httpx.get(f"http://localhost:{port}", timeout=TIMEOUT)
            self.assertEqual(get_response.status_code, 200)

            # Check CORS headers are present in GET response too
            self.assertIn("Access-Control-Allow-Origin", get_response.headers)
            self.assertEqual(get_response.headers["Access-Control-Allow-Origin"], "*")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_cross_origin_isolation_headers(self) -> None:
        """Test that cross-origin isolation headers are properly set for audio worklets."""
        port = PORT + 2
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 2,
            open_browser=False,
        )

        try:
            # Give server time to start
            time.sleep(2)

            response = httpx.get(f"http://localhost:{port}", timeout=TIMEOUT)
            self.assertEqual(response.status_code, 200)

            # Check cross-origin isolation headers
            self.assertIn("Cross-Origin-Embedder-Policy", response.headers)
            self.assertIn("Cross-Origin-Opener-Policy", response.headers)

            # Verify the values
            coep = response.headers["Cross-Origin-Embedder-Policy"]
            coop = response.headers["Cross-Origin-Opener-Policy"]

            # Should be either 'credentialless' or 'require-corp'
            self.assertIn(coep, ["credentialless", "require-corp"])
            self.assertEqual(coop, "same-origin")

        finally:
            proc.terminate()
            time.sleep(1)

    def test_cors_preflight_request(self) -> None:
        """Test CORS preflight request with custom headers."""
        port = PORT + 3
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 3,
            open_browser=False,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Simulate a preflight request with custom headers
            headers = {
                "Origin": "https://example.com",
                "Access-Control-Request-Method": "POST",
                "Access-Control-Request-Headers": "Content-Type,Authorization",
            }

            response = httpx.options(
                f"http://localhost:{port}", headers=headers, timeout=TIMEOUT
            )

            # Check CORS response headers
            self.assertIn("Access-Control-Allow-Origin", response.headers)
            self.assertIn("Access-Control-Allow-Methods", response.headers)
            self.assertIn("Access-Control-Allow-Headers", response.headers)

            # Verify values - Flask-CORS echoes back the specific origin when provided
            # This is correct security behavior
            self.assertEqual(
                response.headers["Access-Control-Allow-Origin"], "https://example.com"
            )

            allowed_methods = response.headers["Access-Control-Allow-Methods"]
            self.assertIn("POST", allowed_methods)
            self.assertIn("GET", allowed_methods)

            allowed_headers = response.headers["Access-Control-Allow-Headers"]
            self.assertIn("Content-Type", allowed_headers)

        finally:
            proc.terminate()
            time.sleep(1)

    def test_all_headers_on_file_request(self) -> None:
        """Test that both CORS and cross-origin isolation headers are present on file requests."""
        port = PORT + 4
        proc = Test.spawn_http_server(
            INDEX_HTML.parent,
            port=port,
            compile_server_port=COMPILE_SERVER_PORT + 4,
            open_browser=False,
        )

        try:
            # Give server time to start
            time.sleep(2)

            # Request the index.html file
            response = httpx.get(f"http://localhost:{port}/index.html", timeout=TIMEOUT)
            self.assertEqual(response.status_code, 200)

            # Check all important headers are present
            headers = response.headers

            # CORS headers
            self.assertIn("Access-Control-Allow-Origin", headers)
            self.assertEqual(headers["Access-Control-Allow-Origin"], "*")

            # Cross-origin isolation headers
            self.assertIn("Cross-Origin-Embedder-Policy", headers)
            self.assertIn("Cross-Origin-Opener-Policy", headers)

            # Cache control headers (should be no-cache for development)
            self.assertIn("Cache-Control", headers)
            self.assertIn("no-cache", headers["Cache-Control"])

        finally:
            proc.terminate()
            time.sleep(1)


if __name__ == "__main__":
    unittest.main()
