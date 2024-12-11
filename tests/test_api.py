"""
Unit test file.
"""

import unittest
from tempfile import TemporaryDirectory

from fastled import Api, LiveClient, Test

ENABLED = False


class ApiTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(ENABLED, "This test takes a long time.")
    def test_examples(self) -> None:
        """Test command line interface (CLI)."""

        with Api.server() as server:
            out = Test.test_examples(host=server)
            self.assertEqual(0, len(out), f"Failed tests: {out}")

    def test_live_client(self) -> None:
        """Test command line interface (CLI)."""

        with TemporaryDirectory() as tmpdir:
            with Api.server() as server:
                sketch_directory = Api.project_init(example="Blink", outputdir=tmpdir)
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
