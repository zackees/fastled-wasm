"""
Unit test file.
"""

import os
import platform
import unittest
from tempfile import TemporaryDirectory

from fastled import Api, CompileServer, LiveClient

# def override(url) -> None:
#     """Override the server url."""
#     assert isinstance(url, str) and "localhost" in url

# client_server.TEST_BEFORE_COMPILE = override


def _enabled() -> bool:
    """Check if this system can run the tests."""
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        return True
    # this only works in ubuntu at the moment
    return platform.system() == "Linux"


class ApiTester(unittest.TestCase):
    """Main tester class."""

    # @unittest.skipUnless(_enabled(), "This test takes a long time.")
    # def test_examples(self) -> None:
    #     """Test command line interface (CLI)."""

    #     with Api.server() as server:
    #         out = Test.test_examples(host=server)
    #         self.assertEqual(0, len(out), f"Failed tests: {out}")

    @unittest.skipUnless(_enabled(), "Can only happen with a local server.")
    def test_live_client(self) -> None:
        """Tests that a project can be init'd, then compiled using a local server."""

        with TemporaryDirectory() as tmpdir:
            with Api.server() as server:
                assert isinstance(server, CompileServer)
                sketch_directory = Api.project_init(
                    example="Blink", outputdir=tmpdir, host=server
                )
                client = LiveClient(
                    sketch_directory=sketch_directory,
                    open_web_browser=False,
                    host=server,
                    keep_running=False,
                )
                client.stop()
            expected_output_dir = sketch_directory / "fastled_js"
            # now test that fastled_js is in the sketch directory
            self.assertTrue(expected_output_dir.exists())


if __name__ == "__main__":
    unittest.main()
