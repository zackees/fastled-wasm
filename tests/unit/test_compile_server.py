import unittest
from pathlib import Path

from fastled import Test
from fastled.compile_server import CompileServer
from fastled.web_compile import CompileResult

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(
        Test.can_run_local_docker_tests(), "Skipping test on non-Linux system on github"
    )
    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer(auto_start=True)
        result: CompileResult = server.web_compile(TEST_DIR)
        # Stop the server
        server.stop()
        # Verify server stopped
        self.assertFalse(server.running, "Server did not stop")
        self.assertTrue(result.success, f"Compilation failed: {result.stdout}")


if __name__ == "__main__":
    unittest.main()
