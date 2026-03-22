"""Unit tests for the --purge feature."""

import argparse
from pathlib import Path

import pytest

from fastled.app import purge_cache
from fastled.args import Args


class TestPurgeArgs:
    """Test that the purge flag is correctly parsed into Args."""

    def test_args_purge_default_false(self) -> None:
        """purge defaults to False."""
        args = Args(
            directory=None,
            init=False,
            just_compile=False,
            profile=False,
            app=False,
            debug=False,
            quick=True,
            release=False,
        )
        assert args.purge is False

    def test_args_purge_true(self) -> None:
        """purge can be set to True."""
        args = Args(
            directory=None,
            init=False,
            just_compile=False,
            profile=False,
            app=False,
            debug=False,
            quick=True,
            release=False,
            purge=True,
        )
        assert args.purge is True

    def test_args_from_namespace_purge(self) -> None:
        """Args.from_namespace correctly passes through the purge flag."""
        ns = argparse.Namespace(
            directory=None,
            init=None,
            just_compile=False,
            profile=False,
            app=False,
            debug=False,
            quick=True,
            release=False,
            install=False,
            dry_run=False,
            no_interactive=False,
            no_https=False,
            fastled_path=None,
            purge=True,
        )
        args = Args.from_namespace(ns)
        assert args.purge is True


class TestPurgeCache:
    """Test the purge_cache function."""

    def test_purge_removes_cache_dir(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """purge_cache removes the cache directory when it exists."""
        cache_dir = tmp_path / ".fastled" / "cache"
        cache_dir.mkdir(parents=True)
        # Put some files in it
        (cache_dir / "some_repo.zip").write_text("data")
        (cache_dir / "subdir").mkdir()
        (cache_dir / "subdir" / "file.txt").write_text("nested")

        purge_cache(cache_dir)

        assert not cache_dir.exists()
        assert "Purged FastLED cache:" in capsys.readouterr().out

    def test_purge_no_cache_dir(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """purge_cache prints a message when cache directory doesn't exist."""
        cache_dir = tmp_path / ".fastled" / "cache"
        assert not cache_dir.exists()

        purge_cache(cache_dir)

        assert "No FastLED cache to purge." in capsys.readouterr().out

    def test_purge_wasm_build_artifacts(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """purge_cache removes stale WASM build artifacts when fastled_path is provided."""
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        fastled_path = tmp_path / "FastLED"
        wasm_dir = fastled_path / ".build" / "meson-wasm-debug"
        wasm_dir.mkdir(parents=True)

        # Create the stale artifacts
        stale_files = ["wasm_ld_args.json", "wasm_ld_args.key", "fastled_glue.js"]
        for name in stale_files:
            (wasm_dir / name).write_text("stale")

        # Also create a non-stale file that should NOT be removed
        (wasm_dir / "keep_me.txt").write_text("important")

        purge_cache(cache_dir, fastled_path=fastled_path)

        for name in stale_files:
            assert not (wasm_dir / name).exists(), f"{name} should have been purged"
        assert (
            wasm_dir / "keep_me.txt"
        ).exists(), "Non-stale files should be preserved"

        out = capsys.readouterr().out
        for name in stale_files:
            assert f"Purged: {wasm_dir / name}" in out

    def test_purge_wasm_multiple_meson_dirs(self, tmp_path: Path) -> None:
        """purge_cache handles multiple meson-wasm-* directories."""
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        fastled_path = tmp_path / "FastLED"

        dirs = ["meson-wasm-debug", "meson-wasm-release", "meson-wasm-quick"]
        for d in dirs:
            wasm_dir = fastled_path / ".build" / d
            wasm_dir.mkdir(parents=True)
            (wasm_dir / "wasm_ld_args.json").write_text("stale")

        purge_cache(cache_dir, fastled_path=fastled_path)

        for d in dirs:
            assert not (fastled_path / ".build" / d / "wasm_ld_args.json").exists()

    def test_purge_no_fastled_path_skips_wasm(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """purge_cache only removes cache dir when fastled_path is None."""
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()

        purge_cache(cache_dir, fastled_path=None)

        assert not cache_dir.exists()
        out = capsys.readouterr().out
        assert "Purged FastLED cache:" in out
        # No WASM artifact messages
        assert "wasm_ld_args" not in out

    def test_purge_wasm_no_build_dir(self, tmp_path: Path) -> None:
        """purge_cache handles missing .build directory gracefully."""
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        fastled_path = tmp_path / "FastLED"
        # Don't create .build dir

        # Should not raise
        purge_cache(cache_dir, fastled_path=fastled_path)

    def test_purge_wasm_partial_artifacts(
        self, tmp_path: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        """purge_cache handles directories with only some stale artifacts."""
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        fastled_path = tmp_path / "FastLED"
        wasm_dir = fastled_path / ".build" / "meson-wasm-quick"
        wasm_dir.mkdir(parents=True)

        # Only create one of the three stale files
        (wasm_dir / "fastled_glue.js").write_text("stale")

        purge_cache(cache_dir, fastled_path=fastled_path)

        assert not (wasm_dir / "fastled_glue.js").exists()
        out = capsys.readouterr().out
        assert "fastled_glue.js" in out
        assert "wasm_ld_args.json" not in out
