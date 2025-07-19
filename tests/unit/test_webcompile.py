import time
import unittest
from pathlib import Path

from fastled.emoji_util import safe_print
from fastled.web_compile import _check_embedded_http_status, web_compile

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"

_USE_LOCALHOST = False
_HOST = "http://localhost" if _USE_LOCALHOST else None


class WebCompileTester(unittest.TestCase):
    """Main tester class."""

    def test_compile(self) -> None:
        """Test web compilation functionality with real server."""
        # Test the web_compile function with actual server call
        start = time.time()
        result = web_compile(TEST_DIR, host=_HOST)
        diff = time.time() - start
        print(f"Time taken: {diff:.2f} seconds")

        # Verify we got a successful result
        self.assertTrue(result.success, f"Compilation failed: {result.stdout}")

        # Verify we got actual WASM data back
        self.assertTrue(len(result.zip_bytes) > 0)

        # Print compilation output for debugging
        safe_print(f"Compilation stdout:\n{result.stdout}")

        safe_print(f"Zip size: {len(result.zip_bytes)} bytes")

    def test_invalid_directory(self) -> None:
        """Test handling of invalid directory."""
        with self.assertRaises(FileNotFoundError):
            web_compile(Path("nonexistent_directory"))

    def test_embedded_http_status_parsing(self) -> None:
        """Test the _check_embedded_http_status function."""
        # Test successful case (no embedded status)
        content_success = b"Some output\nCompilation successful\n"
        has_status, status_code = _check_embedded_http_status(content_success)
        self.assertFalse(has_status)
        self.assertIsNone(status_code)

        # Test embedded 400 status
        content_400 = b"Some error output\nHTTP_STATUS: 400"
        has_status, status_code = _check_embedded_http_status(content_400)
        self.assertTrue(has_status)
        self.assertEqual(status_code, 400)

        # Test embedded 200 status
        content_200 = b"Some output\nHTTP_STATUS: 200"
        has_status, status_code = _check_embedded_http_status(content_200)
        self.assertTrue(has_status)
        self.assertEqual(status_code, 200)

        # Test malformed embedded status
        content_malformed = b"Some output\nHTTP_STATUS: invalid"
        has_status, status_code = _check_embedded_http_status(content_malformed)
        self.assertTrue(has_status)
        self.assertIsNone(status_code)

        # Test empty content
        content_empty = b""
        has_status, status_code = _check_embedded_http_status(content_empty)
        self.assertFalse(has_status)
        self.assertIsNone(status_code)

        # Test content with embedded status in middle (should not be detected)
        content_middle = b"Some output\nHTTP_STATUS: 400\nMore output"
        has_status, status_code = _check_embedded_http_status(content_middle)
        self.assertFalse(has_status)
        self.assertIsNone(status_code)


if __name__ == "__main__":
    unittest.main()
