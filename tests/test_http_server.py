"""
Unit test file.
"""

import time
import unittest
from pathlib import Path

import httpx

from fastled import Test

HERE = Path(__file__).parent
INDEX_HTML = HERE / "html" / "index.html"

assert INDEX_HTML.exists()

# def override(url) -> None:
#     """Override the server url."""
#     assert isinstance(url, str) and "localhost" in url

# client_server.TEST_BEFORE_COMPILE = override


class HttpServerTester(unittest.TestCase):
    """Main tester class."""

    def test_http_server(self) -> None:
        """Test the http server."""
        proc = Test.spawn_http_server(INDEX_HTML.parent, port=8081, open_browser=False)
        time.sleep(1)
        # test get request
        response = httpx.get("http://localhost:8081")
        self.assertEqual(response.status_code, 200)
        proc.kill()
        proc.wait()


if __name__ == "__main__":
    unittest.main()
