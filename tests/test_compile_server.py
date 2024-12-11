import os
import platform
import unittest
from pathlib import Path

from fastled.compile_server import CompileServer
from fastled.web_compile import WebCompileResult

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        return True
    # this only works in ubuntu at the moment
    return platform.system() == "Linux"


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer(auto_start=True)
        result: WebCompileResult = server.web_compile(TEST_DIR)
        # Stop the server
        server.stop()
        # Verify server stopped
        self.assertFalse(server.running, "Server did not stop")
        self.assertTrue(result.success, f"Compilation failed: {result.stdout}")


if __name__ == "__main__":
    unittest.main()
