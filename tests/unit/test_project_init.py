import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from fastled.project_init import (
    _ensure_repo_via_rust,
    _init_example_from_repo,
    get_examples,
)

HERE = Path(__file__).parent
TEST_DIR = HERE / "test_ino" / "wasm"


class ProjectInitTester(unittest.TestCase):
    """Main tester class."""

    def test_get_examples(self) -> None:
        """Test get_examples function."""
        examples = get_examples()
        self.assertTrue(len(examples) > 0)
        self.assertTrue("wasm" in examples)

    @patch("fastled.project_init._native_ensure_fastled_repo")
    def test_ensure_repo_via_rust_prefers_native_extension(self, mock_native) -> None:
        """Native Rust extension should be used before CLI subprocess fallback."""
        with TemporaryDirectory() as tmpdir:
            mock_native.return_value = tmpdir
            repo = _ensure_repo_via_rust("master")
            self.assertEqual(Path(tmpdir), repo)
            mock_native.assert_called_once_with("master")

    @patch("fastled.project_init._native_init_example_from_repo")
    def test_init_example_from_repo_prefers_native_extension(self, mock_native) -> None:
        """Example copy should stay in Rust when the extension is available."""
        with TemporaryDirectory() as tmpdir:
            out = Path(tmpdir) / "Blink"
            out.mkdir()
            mock_native.return_value = str(out)
            result = _init_example_from_repo(
                repo_root=Path("repo"),
                example="Blink",
                outputdir=Path(tmpdir),
                resolved_ref="master",
            )
            self.assertEqual(out, result)
            mock_native.assert_called_once_with("repo", "Blink", tmpdir, "master")


if __name__ == "__main__":
    unittest.main()
