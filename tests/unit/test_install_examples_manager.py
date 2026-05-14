"""Tests for install examples compatibility wrapper delegation."""

import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

_SRC_DIR = str(Path(__file__).parent.parent.parent / "src")


def _install_native_stub() -> None:
    if _SRC_DIR not in sys.path:
        sys.path.insert(0, _SRC_DIR)
    native = types.ModuleType("fastled._native")
    native.__dict__.update(
        {
            "version": lambda: "test",
            "NativeBuildService": object,
            "collect_examples": lambda examples_dir: [],
            "ensure_fastled_repo": lambda ref: "",
            "find_fastled_repo_upwards": lambda start, max_depth: None,
            "init_example_from_repo": lambda repo, example, outputdir, ref: outputdir,
            "read_fastled_json_ref": lambda directory: None,
        }
    )
    sys.modules["fastled._native"] = native


class TestInstallExamplesManager(unittest.TestCase):
    def test_force_delegates_project_init_to_native_cli_helper(self) -> None:
        _install_native_stub()
        from fastled.install.examples_manager import (
            install_fastled_examples_via_project_init,
        )

        with patch(
            "fastled.install.examples_manager.invoke_rust_fastled_cli", return_value=0
        ) as invoke:
            self.assertTrue(install_fastled_examples_via_project_init(force=True))

        invoke.assert_called_once_with(["--project-init"])

    def test_native_cli_helper_nonzero_exit_returns_false(self) -> None:
        _install_native_stub()
        from fastled.install.examples_manager import (
            install_fastled_examples_via_project_init,
        )

        with patch(
            "fastled.install.examples_manager.invoke_rust_fastled_cli", return_value=2
        ) as invoke:
            self.assertFalse(install_fastled_examples_via_project_init(force=True))

        invoke.assert_called_once_with(["--project-init"])


if __name__ == "__main__":
    unittest.main()
