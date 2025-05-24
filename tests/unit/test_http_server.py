"""
Unit test file.
"""

import os
import random
import time
import unittest
from pathlib import Path

import httpx

os.environ["FLASK_SERVER_LOGGING"] = "1"

from fastled import Test

HERE = Path(__file__).parent
INDEX_HTML = HERE / "html" / "index.html"
TIMEOUT = 120

assert INDEX_HTML.exists()


# realistic range, 8021 - 8030
PORT = random.randint(8394, 8423)
PORT2 = PORT + 3


class HttpServerTester(unittest.TestCase):
    """Main tester class."""

    def test_http_server(self) -> None:
        """Test the http server."""
        port = PORT
        proc = Test.spawn_http_server(INDEX_HTML.parent, port=port, open_browser=False)
        response = httpx.get(f"http://localhost:{port}", timeout=1)
        self.assertEqual(response.status_code, 200)
        proc.terminate()
        time.sleep(1)

    def test_http_server_404(self) -> None:
        """Test the http server returns 404 for non-existent files."""
        port = PORT2
        proc = Test.spawn_http_server(INDEX_HTML.parent, port=port, open_browser=False)
        response = httpx.get(f"http://localhost:{port}/nonexistent.html", timeout=1)
        self.assertEqual(response.status_code, 404)
        proc.terminate()
        time.sleep(1)


if __name__ == "__main__":
    unittest.main()
