import time
import unittest
from pathlib import Path

from fastled.web_compile import web_compile

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "bad"


class WebCompileTester(unittest.TestCase):
    """Main tester class."""

    def test_bad_compile_and_ensure_error_is_in_stdout(self) -> None:
        """Test web compilation functionality with real server."""
        # Test the web_compile function with actual server call
        start = time.time()
        result = web_compile(TEST_DIR)
        diff = time.time() - start
        print(f"Time taken: {diff:.2f} seconds")

        # Verify we got a successful result
        self.assertFalse(result.success)

        # Verify we got actual WASM data back

        self.assertEqual(0, len(result.zip_bytes))

        # Print compilation output for debugging
        print(f"Compilation stdout:\n{result.stdout}")
        self.assertIn("lsfjsdklfjdskfjkasdfjdsfds", result.stdout)

        print(f"Zip size: {len(result.zip_bytes)} bytes")


if __name__ == "__main__":
    unittest.main()
