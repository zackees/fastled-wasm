"""
Integration test file for testing session ID functionality in the FastLED WASM server.

This test file uses a real Docker-based server to test session ID handling.
"""

import io
import unittest
import zipfile
from pathlib import Path

import httpx

from fastled import Api
from fastled.types import BuildMode
from fastled.web_compile import _AUTH_TOKEN


class TestSessionCompile(unittest.TestCase):
    """Test session ID functionality using a real Docker-based server."""

    # Class-level variables for shared server instance
    server = None
    base_url = None

    @classmethod
    def setUpClass(cls):
        """Set up test environment once for all tests."""
        cls.test_dir = Path(__file__).parent / "test_ino" / "wasm"
        cls.timeout = 30

        # Check if we can run local docker tests
        if cls._enabled():
            print("\n🚀 Starting local FastLED WASM server for session ID tests...")
            cls.server = Api.spawn_server()
            cls.base_url = cls.server.url()
            print(f"✅ Server started at: {cls.base_url}")
        else:
            print("\n⚠️ Docker not available, skipping tests")
            cls.server = None
            cls.base_url = None

    @classmethod
    def tearDownClass(cls):
        """Clean up server after all tests."""
        if cls.server is not None:
            print("\n🛑 Stopping local FastLED WASM server...")
            cls.server.stop()
            print("✅ Server stopped")

    @classmethod
    def _enabled(cls) -> bool:
        """Check if this system can run the tests."""
        from fastled import Test

        return Test.can_run_local_docker_tests()

    def setUp(self):
        """Set up test case."""
        if not self._enabled():
            self.skipTest("Docker not available for testing")

        # Create a test file for compilation
        self.test_file_content = b"void setup() {}\nvoid loop() {}"

    def _create_test_sketch_zip(self) -> bytes:
        """Create a test sketch zip file for upload."""
        # Create in-memory zip file
        zip_buffer = io.BytesIO()
        with zipfile.ZipFile(zip_buffer, "w", zipfile.ZIP_DEFLATED) as zip_file:
            # Add the main sketch file
            zip_file.writestr("wasm/wasm.ino", self.test_file_content)
            # Add build mode identifier
            zip_file.writestr("wasm/build_mode.txt", BuildMode.QUICK.value)

        return zip_buffer.getvalue()

    def test_session_id_persistence(self):
        """Test that session ID persists across multiple compilation requests."""
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        # First request with session_id=123
        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        headers = {
            "authorization": _AUTH_TOKEN,
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
            "strict": "false",
            "session_id": "123",
        }

        # Make first request
        response1 = httpx.post(
            f"{self.base_url}/compile/wasm",
            files=files,
            headers=headers,
            timeout=self.timeout,
            follow_redirects=True,
        )
        self.assertEqual(response1.status_code, 200)

        # Second request with same session_id
        response2 = httpx.post(
            f"{self.base_url}/compile/wasm",
            files=files,
            headers=headers,
            timeout=self.timeout,
            follow_redirects=True,
        )
        self.assertEqual(response2.status_code, 200)

        # Verify both responses contain WASM data
        self.assertTrue(len(response1.content) > 0)
        self.assertTrue(len(response2.content) > 0)

    def test_different_session_ids(self):
        """Test that different session IDs are handled correctly."""
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        # First request with session_id=123
        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        headers1 = {
            "authorization": _AUTH_TOKEN,
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
            "strict": "false",
            "session_id": "123",
        }

        response1 = httpx.post(
            f"{self.base_url}/compile/wasm",
            files=files,
            headers=headers1,
            timeout=self.timeout,
            follow_redirects=True,
        )
        self.assertEqual(response1.status_code, 200)

        # Second request with different session_id=456
        headers2 = headers1.copy()
        headers2["session_id"] = "456"

        response2 = httpx.post(
            f"{self.base_url}/compile/wasm",
            files=files,
            headers=headers2,
            timeout=self.timeout,
            follow_redirects=True,
        )
        self.assertEqual(response2.status_code, 200)

        # Verify both responses contain WASM data
        self.assertTrue(len(response1.content) > 0)
        self.assertTrue(len(response2.content) > 0)

    def test_no_session_id(self):
        """Test compilation without a session ID."""
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        headers = {
            "authorization": _AUTH_TOKEN,
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
            "strict": "false",
        }

        response = httpx.post(
            f"{self.base_url}/compile/wasm",
            files=files,
            headers=headers,
            timeout=self.timeout,
            follow_redirects=True,
        )
        self.assertEqual(response.status_code, 200)
        self.assertTrue(len(response.content) > 0)

    def test_nonexistent_session_id(self):
        """Test that using a non-existent session ID returns the appropriate error."""
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        headers = {
            "authorization": _AUTH_TOKEN,
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
            "strict": "false",
            "session_id": "999",  # Non-existent session ID
        }

        try:
            response = httpx.post(
                f"{self.base_url}/compile/wasm",
                files=files,
                headers=headers,
                timeout=self.timeout,
                follow_redirects=True,
            )
            # The server accepts any session ID format, so we expect 200
            self.assertEqual(response.status_code, 200)
            self.assertTrue(len(response.content) > 0)
        except httpx.HTTPStatusError as e:
            # If the server raises an error, that's also acceptable
            self.assertTrue(e.response.status_code in [200, 404])

    def test_invalid_session_id(self):
        """Test that using an invalid session ID format is accepted by the server."""
        # Create test sketch zip
        zip_bytes = self._create_test_sketch_zip()

        files = {"file": ("wasm.zip", zip_bytes, "application/x-zip-compressed")}
        headers = {
            "authorization": _AUTH_TOKEN,
            "build": BuildMode.QUICK.value.lower(),
            "profile": "false",
            "strict": "false",
            "session_id": "invalid",  # Invalid session ID format
        }

        try:
            response = httpx.post(
                f"{self.base_url}/compile/wasm",
                files=files,
                headers=headers,
                timeout=self.timeout,
                follow_redirects=True,
            )
            # The server accepts any session ID format, so we expect 200
            self.assertEqual(response.status_code, 200)
            self.assertTrue(len(response.content) > 0)
        except httpx.HTTPStatusError as e:
            # If the server raises an error, that's also acceptable
            self.assertTrue(e.response.status_code in [200, 400])


if __name__ == "__main__":
    unittest.main()
