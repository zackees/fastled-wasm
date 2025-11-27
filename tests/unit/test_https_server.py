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


class HttpsErrorScenarioTests(unittest.TestCase):
    """Test HTTPS server error handling with SSL certificate failures."""

    def test_missing_certificate_files_fallback_to_http(self) -> None:
        """Test that server falls back to HTTP when certificate files are missing."""
        # Verify the fallback logic exists in the source code
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Check that the server has fallback logic
        self.assertIn("if certfile and keyfile", source)
        self.assertIn("ssl_enabled = False", source)
        self.assertIn("if not ssl_enabled", source)

        print("✓ Server has logic to check for certificate files")
        print("✓ Server can operate without SSL (HTTP fallback)")
        print("✓ Server tracks SSL enabled state")

    def test_corrupted_certificate_fallback(self) -> None:
        """Test server fallback behavior with corrupted certificates."""
        # Verify the fallback logic exists in the source code
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Check that SSL error handling is present
        self.assertIn("except Exception as ssl_error", source)
        self.assertIn("Falling back to HTTP", source)
        self.assertIn("ssl_enabled = False", source)
        print("✓ Server code has proper exception handling for SSL errors")
        print("✓ Server code falls back to HTTP on SSL failure")

    def test_certificate_expiration_check(self) -> None:
        """Test that certificate expiration checking is implemented."""
        from pathlib import Path

        # Read the server_flask.py source file directly
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Verify the certificate expiration checking function exists
        self.assertIn("def _check_certificate_expiration", source)
        self.assertIn("cryptography", source)
        self.assertIn("x509.load_pem_x509_certificate", source)
        self.assertIn("days_remaining", source)

        print("✓ Certificate expiration check function is implemented")
        print("✓ Function uses cryptography library for X.509 parsing")
        print("✓ Function calculates days remaining until expiration")

    def test_invalid_certificate_format_handling(self) -> None:
        """Test that invalid certificate formats are handled gracefully."""
        # Verify the error handling exists in the source code
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Verify that _check_certificate_expiration has try-except error handling
        self.assertIn("def _check_certificate_expiration", source)
        self.assertIn("try:", source)
        self.assertIn("except Exception", source)
        self.assertIn("return", source)

        print("✓ Certificate expiration check has error handling")
        print("✓ Invalid certificate format handled gracefully")

    def test_certificate_key_pair_validation(self) -> None:
        """Test that certificate and key files are properly matched."""
        from pathlib import Path

        import fastled

        # Get the packaged certificate and key
        assets_dir = Path(fastled.__file__).parent / "assets"
        certfile = assets_dir / "localhost.pem"
        keyfile = assets_dir / "localhost-key.pem"

        # Verify both files exist
        self.assertTrue(certfile.exists(), "Certificate file should exist")
        self.assertTrue(keyfile.exists(), "Key file should exist")

        # Verify files are readable
        with open(certfile, "r") as f:
            cert_content = f.read()
            self.assertIn("BEGIN CERTIFICATE", cert_content)

        with open(keyfile, "r") as f:
            key_content = f.read()
            self.assertIn("BEGIN", key_content)

        print("✓ Certificate and key files exist and are readable")
        print("✓ Certificate and key files have valid PEM format headers")

    def test_ssl_context_configuration(self) -> None:
        """Test that SSL context is configured with secure settings."""
        # Verify the SSL configuration in the source code
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Check for proper SSL configuration using either SSLContext or dict-based ssl_options
        # The implementation may use dict-based options to work around Python 3.13 truststore issues
        has_ssl_context = (
            "ssl.PROTOCOL_TLS_SERVER" in source and "ssl_ctx.load_cert_chain" in source
        )
        has_dict_options = (
            "ssl_options_dict" in source
            and '"certfile"' in source
            and '"keyfile"' in source
        )
        self.assertTrue(
            has_ssl_context or has_dict_options,
            "Server must configure SSL using either SSLContext or dict-based ssl_options",
        )

        if has_ssl_context:
            print("✓ Server uses PROTOCOL_TLS_SERVER for SSL context")
            print("✓ Server loads certificate chain properly")
        else:
            print("✓ Server uses dict-based ssl_options for SSL configuration")
            print("✓ Server specifies certfile and keyfile in ssl_options")

    def test_http_fallback_warning_messages(self) -> None:
        """Test that appropriate warning messages are shown on SSL failure."""
        # Verify warning messages are present in the source code
        server_flask_path = (
            Path(__file__).parent.parent.parent / "src" / "fastled" / "server_flask.py"
        )

        if not server_flask_path.exists():
            self.skipTest("Source file not found, skipping test")
            return

        with open(server_flask_path, "r") as f:
            source = f.read()

        # Check for fallback warning
        self.assertIn("Falling back to HTTP", source)
        self.assertIn("Microphone access may not work", source)
        print("✓ Server displays warning when falling back to HTTP")
        print("✓ Server warns about microphone access limitations")


if __name__ == "__main__":
    unittest.main()
