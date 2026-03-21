"""Tests for --latest, --branch, --commit CLI arguments used with --init."""

import subprocess
import sys
import unittest
from pathlib import Path

_SRC_DIR = str(Path(__file__).parent.parent.parent / "src")


def _run_fastled(*args: str) -> subprocess.CompletedProcess[str]:
    """Run fastled CLI with the given arguments, capturing output.

    Uses PYTHONPATH to ensure we run against the local source tree rather than
    a potentially stale system-installed package.
    """
    import os

    env = os.environ.copy()
    # Prepend src/ so `python -m fastled` resolves to the local source
    env["PYTHONPATH"] = _SRC_DIR + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [sys.executable, "-m", "fastled", *args],
        capture_output=True,
        text=True,
        timeout=60,
        stdin=subprocess.DEVNULL,
        env=env,
    )


class TestInitRefArgsMutualExclusivity(unittest.TestCase):
    """--latest is mutually exclusive with --branch and --commit."""

    def test_latest_with_branch_fails(self) -> None:
        result = _run_fastled("--init=wasm", "--latest", "--branch", "master")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "--latest cannot be used with --branch or --commit", result.stdout
        )

    def test_latest_with_commit_fails(self) -> None:
        result = _run_fastled("--init=wasm", "--latest", "--commit", "abc1234")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "--latest cannot be used with --branch or --commit", result.stdout
        )

    def test_latest_with_branch_and_commit_fails(self) -> None:
        result = _run_fastled(
            "--init=wasm", "--latest", "--branch", "master", "--commit", "abc1234"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "--latest cannot be used with --branch or --commit", result.stdout
        )

    def test_branch_and_commit_together_accepted(self) -> None:
        """--branch and --commit can coexist; --commit takes precedence."""
        # Use an invalid commit so init fails, but argparse should NOT reject the combo
        result = _run_fastled(
            "--init=wasm", "--branch", "master", "--commit", "0000000"
        )
        # Should NOT fail with a mutual-exclusivity error
        self.assertNotIn("--latest cannot be used with", result.stdout + result.stderr)


class TestInitRefArgsExistInParser(unittest.TestCase):
    """Verify --latest, --branch, --commit are defined in parse_args.py."""

    def test_help_shows_latest(self) -> None:
        result = _run_fastled("--help")
        self.assertIn("--latest", result.stdout)

    def test_help_shows_branch(self) -> None:
        result = _run_fastled("--help")
        self.assertIn("--branch", result.stdout)

    def test_help_shows_commit(self) -> None:
        result = _run_fastled("--help")
        self.assertIn("--commit", result.stdout)

    def test_no_master_flag(self) -> None:
        """--master should not exist; replaced by --branch master."""
        result = _run_fastled("--help")
        self.assertNotIn("--master", result.stdout)


class TestBuildSiteUsesBranchMaster(unittest.TestCase):
    """build.py must pass --branch master so init matches the compile step."""

    def test_build_example_uses_branch_master(self) -> None:
        from pathlib import Path

        source = (
            Path(__file__).parent.parent.parent
            / "src"
            / "fastled"
            / "site"
            / "build.py"
        ).read_text()
        self.assertIn(
            "--branch master",
            source,
            "build_example must pass --branch master to fastled --init",
        )


if __name__ == "__main__":
    unittest.main()
