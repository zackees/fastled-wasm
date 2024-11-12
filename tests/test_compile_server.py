import unittest
from pathlib import Path

from fastled_wasm.compile_server import CompileServer
from fastled_wasm.web_compile import WebCompileResult, web_compile

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer()
        server.wait_for_startup()
        url = server.url()
        result: WebCompileResult = web_compile(TEST_DIR, host=url)

        # Stop the server
        server.stop()

        # Verify server stopped
        self.assertFalse(server.running, "Server did not stop")
        self.assertIsNone(server.docker_process, "Server process not cleared")
        self.assertTrue(result.success, "Compilation failed")


if __name__ == "__main__":
    unittest.main()
