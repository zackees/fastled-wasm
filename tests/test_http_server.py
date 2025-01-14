"""
Unit test file.
"""

import unittest
from pathlib import Path

import httpx

from fastled import Test

HERE = Path(__file__).parent
INDEX_HTML = HERE / "html" / "index.html"
TIMEOUT = 120

assert INDEX_HTML.exists()


class HttpServerTester(unittest.TestCase):
    """Main tester class."""

    def test_http_server(self) -> None:
        """Test the http server."""
        proc = Test.spawn_http_server(INDEX_HTML.parent, port=8021, open_browser=False)
        response = httpx.get("http://localhost:8021", timeout=1)
        self.assertEqual(response.status_code, 200)
        proc.terminate()


if __name__ == "__main__":
    unittest.main()
