import time
import unittest
from pathlib import Path

from fastled.web_compile import web_compile

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
        print(f"Compilation stdout:\n{result.stdout}")

        print(f"Zip size: {len(result.zip_bytes)} bytes")

    def test_invalid_directory(self) -> None:
        """Test handling of invalid directory."""
        with self.assertRaises(FileNotFoundError):
            web_compile(Path("nonexistent_directory"))


if __name__ == "__main__":
    unittest.main()
