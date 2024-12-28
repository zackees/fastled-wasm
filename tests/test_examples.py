import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled.compile_server import CompileServer
from fastled.project_init import project_init
from fastled.web_compile import CompileResult

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


EXAMPLES = ["Blink", "wasm", "Chromancer", "FxSdCard", "FxNoiseRing"]


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


class WebCompileTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_server(self) -> None:
        """Test basic server start/stop functionality."""
        server = CompileServer(auto_start=True)
        try:
            with TemporaryDirectory() as tmpdir:
                for example in EXAMPLES:
                    out = Path(tmpdir)
                    project_init(example=example, outputdir=out)
                    # print out everything in the out dir
                    for f in out.iterdir():
                        print(f)
                    name = Path(example).name
                    self.assertTrue((out / example / f"{name}.ino").exists())
                    # Test the web_compile function with actual server call
                    result: CompileResult = server.web_compile(out / example)
                    self.assertTrue(
                        result.success, f"Compilation failed: {result.stdout}"
                    )
        finally:
            server.stop()


if __name__ == "__main__":
    unittest.main()
