"""
Unit test file.
"""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api

HERE = Path(__file__).parent
INDEX_HTML = HERE / "html" / "index.html"

assert INDEX_HTML.exists()


class ApiTester(unittest.TestCase):
    """Main tester class."""

    def test_project_init(self) -> None:
        """Tests that a project can be initialized."""
        with TemporaryDirectory() as tmpdir:
            sketch_directory = Api.project_init(
                example="Blink",
                outputdir=tmpdir,
            )
            self.assertTrue(sketch_directory.exists())


if __name__ == "__main__":
    unittest.main()
