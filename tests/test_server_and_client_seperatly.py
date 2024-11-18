import os
import platform
import unittest
from pathlib import Path

from fastled.compile_server import CompileServer

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"
CLIENT_CMD = f"fastled --just-compile --localhost {TEST_DIR}"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        return True
    # this only works in ubuntu at the moment
    return platform.system() == "Linux"


class ServerLocalClientTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer()
        server.wait_for_startup()

        rtn = os.system(CLIENT_CMD)

        # Stop the server
        server.stop()

        # Verify server stopped
        # self.assertTrue(rtn == 0, "Client did not stop")
        self.assertEqual(0, rtn, "Client did compile successfully")


if __name__ == "__main__":
    unittest.main()
