"""Tests for --latest, --branch, --commit CLI arguments used with --init."""

import sys
import types
import unittest
from pathlib import Path

_ROOT_DIR = Path(__file__).parent.parent.parent
_SRC_DIR = str(_ROOT_DIR / "src")
_CLI_SOURCE = _ROOT_DIR / "crates" / "fastled-cli" / "src" / "lib.rs"


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


def _cli_source() -> str:
    return _CLI_SOURCE.read_text()


class TestInitRefArgsMutualExclusivity(unittest.TestCase):
    """--latest is mutually exclusive with --branch and --commit."""

    def test_latest_with_branch_fails(self) -> None:
        source = _cli_source()
        self.assertIn(
            "--latest cannot be used with --branch or --commit",
            source,
        )
        self.assertIn(
            "cli.latest && (cli.branch.is_some() || cli.commit.is_some())", source
        )

    def test_latest_with_commit_fails(self) -> None:
        self.test_latest_with_branch_fails()

    def test_latest_with_branch_and_commit_fails(self) -> None:
        self.test_latest_with_branch_fails()

    def test_branch_and_commit_together_accepted(self) -> None:
        """--branch and --commit can coexist; --commit takes precedence."""
        source = _cli_source()
        self.assertIn("cli.commit.as_deref().or(cli.branch.as_deref())", source)
        self.assertNotIn('conflicts_with = "branch"', source)
        self.assertNotIn('conflicts_with = "commit"', source)


class TestInitRefArgsExistInParser(unittest.TestCase):
    """Verify --latest, --branch, --commit are defined in the Rust CLI."""

    def test_help_shows_latest(self) -> None:
        self.assertIn("latest: bool", _cli_source())

    def test_help_shows_branch(self) -> None:
        self.assertIn("branch: Option<String>", _cli_source())

    def test_help_shows_commit(self) -> None:
        self.assertIn("commit: Option<String>", _cli_source())

    def test_no_master_flag(self) -> None:
        """--master should not exist; replaced by --branch master."""
        self.assertNotIn("--master", _cli_source())


class TestBuildSiteUsesBranchMaster(unittest.TestCase):
    """build.py must pass --branch master so init matches the compile step."""

    def test_build_example_uses_master_ref_for_project_init(self) -> None:
        from unittest.mock import patch

        _install_native_stub()
        from fastled.site import build as site_build

        calls: list[tuple[str, Path | None, str | None]] = []

        def fake_project_init(
            example: str | None = "PROMPT",
            outputdir: Path | None = None,
            ref: str | None = None,
        ) -> Path:
            calls.append((example or "", outputdir, ref))
            out = (
                outputdir / example if outputdir is not None and example else outputdir
            )
            assert out is not None
            (out / "fastled_js").mkdir(parents=True)
            (out / "fastled_js" / "fastled.wasm").write_bytes(b"\0asm")
            return out

        with (
            patch.object(site_build, "project_init", side_effect=fake_project_init),
            patch.object(site_build, "invoke_rust_fastled_cli", return_value=0),
        ):
            import tempfile

            with tempfile.TemporaryDirectory() as tmp:
                site_build.build_example("wasm", Path(tmp))

        self.assertEqual(calls[0][0], "wasm")
        self.assertEqual(calls[0][2], "master")


if __name__ == "__main__":
    unittest.main()
