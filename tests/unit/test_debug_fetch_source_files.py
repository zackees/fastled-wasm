import os
import time
import unittest
import warnings
from pathlib import Path
from urllib.parse import urlparse

import httpx

os.environ["FLASK_SERVER_LOGGING"] = "1"

from fastled import Api, LiveClient

HERE = Path(__file__).parent
TEST_INO_WASM = HERE / "test_ino" / "wasm"

# New refactor has broken this test. Good news, we got the sketch to output debug symbols!!!!!!
_ENABLED = True

_DWARF_SRC_EXAMPLE1 = "http://localhost:{http_port}/dwarfsource/fastledsource/js/src/fastledsource/git/fastled/src/FastLED.h"
_DWARF_SRC_EXAMPLE2 = (
    "http://localhost:{http_port}/dwarfsource/js/dwarfsource/git/fastled/src/chipsets.h"
)

_DWARF_SRC_EXAMPLES = [
    # _DWARF_SRC_EXAMPLE1,
    _DWARF_SRC_EXAMPLE2
]


def _enabled() -> bool:
    """Check if this system can run the tests."""
    from fastled import Test

    return _ENABLED and Test.can_run_local_docker_tests()


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
    def test_http_server_for_fetch_redirect(self) -> None:
        """Tests that we can convert the file paths from emscripten debugging (dwarf) to the actual file paths."""
        http_port = 8932
        client: LiveClient = Api.live_client(
            sketch_directory=TEST_INO_WASM,
            auto_updates=False,
            open_web_browser=False,
            http_port=http_port,
        )
        with client:
            wait_for_server(f"http://localhost:{http_port}", timeout=100)
            backend_host = client.url()

            # This url should proxy back to the server at /dwarfsource/fastledsource/git/fastled/src/FastLED.h
            url = (
                f"http://localhost:{http_port}/fastledsource/git/fastled/src/FastLED.h"
            )

            resp = httpx.get(
                # This type of request will come from the server during debug mode to
                # enable debugging.
                url,
                timeout=100,
            )
            if resp.status_code != 200:
                raise Exception(f"Failed to fetch source file: {resp.status_code}")
            content_length = int(resp.headers["Content-Length"])
            if content_length == 0:
                raise Exception("Content-Length is 0")

            backend_url = backend_host + "/dwarfsource"

            body: dict[str, str] = {
                "path": "fastledsource/git/fastled/src/FastLED.h",
            }

            resp = httpx.post(
                backend_url,
                json=body,
                timeout=100,
            )
            if resp.status_code != 200:
                raise Exception(
                    f"Failed to fetch source file from the backend server: {resp.status_code}"
                )

            for ds in _DWARF_SRC_EXAMPLES:
                # now get something similar at static/js/fastled/src/platforms/wasm/js.cpp
                url = ds.format(http_port=http_port)
                resp = httpx.get(
                    url,
                    timeout=100,
                )
                self.assertTrue(resp.status_code == 200, resp.status_code)
                content_length = int(resp.headers["Content-Length"])
                self.assertTrue(content_length > 0, "Content-Length is 0")

                parsed_url = urlparse(url)
                # Extract the path and reconstruct the backend URL
                # print(parsed_url)

                body: dict[str, str] = {
                    "path": parsed_url.path.lstrip("/"),  # Remove leading slash
                }

                backend_url = backend_host + "/dwarfsource"
                resp = httpx.post(
                    backend_url,
                    json=body,
                    timeout=100,
                )
                if resp.status_code != 200:
                    raise Exception(
                        f"Failed to fetch source file '{backend_url}' from the backend server: {resp.status_code}"
                    )

            print("Done")

            # Work in progress: get system include files.
            # if resp.status_code != 200:
            #     raise Exception(f"Failed to fetch source file: {resp.status_code}")
            # # error dwarfsource/js/drawfsour
            # url = f"http://localhost:{http_port}/dwarfsource/js/dwarfsource/emsdk/upstream/emscripten/cache/sysroot/include/ctype.h"
            # resp = httpx.get(
            #     url,
            #     timeout=100,
            # )
            # if resp.status_code != 200:
            #     raise Exception(f"Failed to fetch source file: {resp.status_code}")


if __name__ == "__main__":
    unittest.main()
