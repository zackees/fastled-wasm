import time
import unittest
from pathlib import Path

from fastled_wasm.compile_server import CompileServer

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"

_USE_LOCALHOST = False
_HOST = "http://localhost"


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer()

        # Wait for server to initialize
        time.sleep(1)

        # Stop the server
        server.stop()

        # Verify server stopped
        self.assertFalse(server.running)
        self.assertIsNone(server.docker_process)

    # def test_compile(self) -> None:
    #     """Test web compilation functionality with real server."""
    #     # Test the web_compile function with actual server call
    #     start = time.time()
    #     result = web_compile(TEST_DIR, host=_HOST)
    #     diff = time.time() - start
    #     print(f"Time taken: {diff:.2f} seconds")

    #     # Verify we got a successful result
    #     self.assertTrue(result.success)

    #     # Verify we got actual WASM data back
    #     self.assertTrue(len(result.zip_bytes) > 0)

    #     # Print compilation output for debugging
    #     print(f"Compilation stdout:\n{result.stdout}")

    #     print(f"Zip size: {len(result.zip_bytes)} bytes")

    # def test_invalid_directory(self) -> None:
    #     """Test handling of invalid directory."""
    #     with self.assertRaises(FileNotFoundError):
    #         web_compile(Path("nonexistent_directory"))


if __name__ == "__main__":
    unittest.main()
