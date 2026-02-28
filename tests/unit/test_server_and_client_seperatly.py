import os
import unittest
from pathlib import Path

from fastled.compile_server import CompileServer

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


class ServerLocalClientTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer(auto_start=True, port=0)
        url = server.url()
        client_cmd = f"fastled --just-compile --web {url} {TEST_DIR}"
        rtn = os.system(client_cmd)

        # Stop the server
        server.stop()

        # Verify server stopped
        self.assertEqual(0, rtn, "Client did compile successfully")


if __name__ == "__main__":
    unittest.main()
