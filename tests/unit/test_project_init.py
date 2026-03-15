import unittest
from pathlib import Path

from fastled.project_init import get_examples

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class ProjectInitTester(unittest.TestCase):
    """Main tester class."""

    def test_get_examples(self) -> None:
        """Test get_examples function."""
        examples = get_examples()
        self.assertTrue(len(examples) > 0)
        self.assertTrue("wasm" in examples)


if __name__ == "__main__":
    unittest.main()
