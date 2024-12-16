import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api
from fastled.project_init import get_examples, project_init

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


def _local_server_enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


class ProjectInitTester(unittest.TestCase):
    """Main tester class."""

    def test_get_examples(self) -> None:
        """Test get_examples function."""
        if _local_server_enabled():
            with Api.server(auto_updates=True) as server:
                examples = get_examples(server.url())
        else:
            examples = get_examples()
        self.assertTrue(len(examples) > 0)
        self.assertTrue("wasm" in examples)

    @unittest.skipUnless(_local_server_enabled(), "This is not a fast test")
    def test_compile(self) -> None:
        """Test web compilation functionality with real server."""
        # Test the web_compile function with actual server call
        with TemporaryDirectory() as tmpdir:
            out = Path(tmpdir)
            with Api.server() as server:
                project_init(example="wasm", outputdir=out, host=server.url())
                # print out everything in the out dir
                for f in out.iterdir():
                    print(f)
                self.assertTrue((out / "wasm" / "wasm.ino").exists())


if __name__ == "__main__":
    unittest.main()
