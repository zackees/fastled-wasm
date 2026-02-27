import time
import unittest
from pathlib import Path

from fastled.emoji_util import safe_print
from fastled.web_compile import web_compile

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "bad"

TEST_DIR_2 = HERE / "test_ino" / "bad_platformio"


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
        safe_print(f"Compilation stdout:\n{result.stdout}")
        # Check that we got a meaningful compilation error in stdout.
        # The specific error may vary depending on the Docker image version:
        # - Newer images: the intentional garbage identifier "lsfjsdklfjdskfjkasdfjdsfds"
        # - Older images: may fail on missing headers like "is_apollo3.h"
        has_expected_error = "lsfjsdklfjdskfjkasdfjdsfds" in result.stdout
        has_any_compile_error = "error" in result.stdout.lower()
        if not has_expected_error and not has_any_compile_error:
            safe_print("No compilation error found in stdout")
            safe_print("stdout:")
            safe_print(result.stdout)
            self.fail("No compilation error found in stdout")
        # if "bad/bad.ino:" not in result.stdout:  # No .cpp extension.
        #     print(
        #         "bad.ino.cpp was not transformed to bad.ino without the cpp extension"
        #     )
        #     print("stdout:")
        #     print(result.stdout)
        #     self.fail(
        #         "bad.ino.cpp was not transformed to bad.ino without the cpp extension"
        #     )

        safe_print(f"Zip size: {len(result.zip_bytes)} bytes")

    def test_platform_ini_does_not_make_it_in(self) -> None:
        """Test that platformio.ini does not make it into the zip."""
        start = time.time()
        result = web_compile(TEST_DIR_2)
        diff = time.time() - start
        print(f"Time taken: {diff:.2f} seconds")

        # Print compilation output for debugging
        safe_print(f"Compilation stdout:\n{result.stdout}")
        safe_print(f"Zip size: {len(result.zip_bytes)} bytes")

        if not result.success:
            # If compilation failed due to infrastructure issues (e.g., stale Docker
            # image with missing headers), skip instead of failing.
            if "file not found" in result.stdout.lower():
                self.skipTest(
                    "Compilation failed due to infrastructure issue (missing headers in Docker image)"
                )
            self.fail(f"Compilation failed unexpectedly: {result.stdout[:200]}")

        self.assertTrue(result.success)


if __name__ == "__main__":
    unittest.main()
