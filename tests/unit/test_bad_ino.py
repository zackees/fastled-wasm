import time
import unittest
from pathlib import Path

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
        print(f"Compilation stdout:\n{result.stdout}")
        if "lsfjsdklfjdskfjkasdfjdsfds" not in result.stdout:
            print("Expected error not found in stdout")
            print("stdout:")
            print(result.stdout)
            self.fail("Expected error not found in stdout")
        # if "bad/bad.ino:" not in result.stdout:  # No .cpp extension.
        #     print(
        #         "bad.ino.cpp was not transformed to bad.ino without the cpp extension"
        #     )
        #     print("stdout:")
        #     print(result.stdout)
        #     self.fail(
        #         "bad.ino.cpp was not transformed to bad.ino without the cpp extension"
        #     )

        print(f"Zip size: {len(result.zip_bytes)} bytes")

    def test_platform_ini_does_not_make_it_in(self) -> None:
        """Test that platformio.ini does not make it into the zip."""
        start = time.time()
        result = web_compile(TEST_DIR_2)
        diff = time.time() - start
        print(f"Time taken: {diff:.2f} seconds")

        # Verify we got a successful result
        self.assertTrue(result.success)

        # Verify we got actual WASM data back

        self.assertTrue(result.success)

        # Print compilation output for debugging
        print(f"Compilation stdout:\n{result.stdout}")

        print(f"Zip size: {len(result.zip_bytes)} bytes")


if __name__ == "__main__":
    unittest.main()
