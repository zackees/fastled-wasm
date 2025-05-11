import time
import unittest
import warnings
from pathlib import Path

import httpx

from fastled import Api, LiveClient

HERE = Path(__file__).parent
TEST_INO_WASM = HERE / "test_ino" / "wasm"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return Test.can_run_local_docker_tests()


def wait_for_server(url: str, timeout: int = 10) -> bool:
    """Wait for the server to be live."""
    expire_time = time.time() + timeout
    while expire_time > time.time():
        try:
            response = httpx.get(url, timeout=1)
            if response.status_code == 200:
                return True
        except httpx.RequestError:
            print(f"Waiting for server to start at {url}")
            pass
    warnings.warn(f"Server at {url} did not start in {timeout} seconds")
    return False


class FetchSourceFileTester(unittest.TestCase):
    """Main tester class."""

    @unittest.skipUnless(
        _enabled(),
        "Skipping test because either this is on non-Linux system on github or embedded data is disabled",
    )
    def test_backend_server_for_src_file_fetch(self) -> None:
        """Tests that embedded data is round tripped correctly."""
        with Api.server() as server:
            resp = server.fetch_source_file("FastLED.h")
            if isinstance(resp, Exception):
                raise resp
            print("Done")

    @unittest.skipUnless(
        _enabled(),
        "Skipping test because either this is on non-Linux system on github or embedded data is disabled",
    )
    def test_http_server_for_fetch_redirect(self) -> None:
        """Tests that embedded data is round tripped correctly."""
        http_port = 8932
        client: LiveClient = Api.live_client(
            sketch_directory=TEST_INO_WASM,
            auto_updates=False,
            open_web_browser=False,
            http_port=http_port,
        )
        with client:
            # TODO Make faster.
            wait_for_server(f"http://localhost:{http_port}", timeout=100)

            resp = httpx.get(
                f"http://localhost:{http_port}/sourcefiles/FastLED.h",
                timeout=10,
            )
            if resp.status_code != 200:
                raise Exception(f"Failed to fetch source file: {resp.status_code}")


if __name__ == "__main__":
    unittest.main()
