import unittest

from fastled import Api

_DISABLE = False


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests() and not _DISABLE


class WebCompilerTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(
        _enabled(),
        "Skipping test because either this is on non-Linux system on github or embedded data is disabled",
    )
    def test_server_big_data_roundtrip(self) -> None:
        """Tests that embedded data is round tripped correctly."""
        with Api.server() as server:
            resp = server.fetch_source_file("platforms/wasm/js.cpp")
            if isinstance(resp, Exception):
                raise resp
            print("Done")


if __name__ == "__main__":
    unittest.main()
