import os
import platform
import unittest
import zipfile
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled.compile_server import CompileServer
from fastled.web_compile import ENABLE_EMBEDDED_DATA, WebCompileResult, web_compile

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "embedded"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    if not ENABLE_EMBEDDED_DATA:
        return False
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        return True
    # this only works in ubuntu at the moment
    return platform.system() == "Linux"


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(
        _enabled(),
        "Skipping test because either this is on non-Linux system on github or embedded data is disabled",
    )
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
        self.assertTrue(result.success, f"Compilation failed: {result.stdout}")

        zip_bytes = result.zip_bytes

        # dump the result into a temp directory
        with TemporaryDirectory() as temp_dir:
            temp_dir_path = Path(temp_dir)
            # unzip zip_bytes
            with open(temp_dir_path / "output.zip", "wb") as f:
                f.write(zip_bytes)
            with zipfile.ZipFile(temp_dir_path / "output.zip", "r") as zip_ref:
                zip_ref.extractall(temp_dir_path)
            # check that data/ dir exists
            self.assertTrue((temp_dir_path / "data").is_dir())
            # check that data/bigdata.dat exists
            self.assertTrue((temp_dir_path / "data" / "bigdata.dat").is_file())


if __name__ == "__main__":
    unittest.main()
