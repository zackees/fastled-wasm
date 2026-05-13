"""Sketch detection helpers.

This module is a thin Python wrapper around the native Rust implementation in
``fastled._native``. The wrappers exist only to:

* normalize ``Path``-vs-``str`` inputs for callers, and
* return ``pathlib.Path`` instances rather than strings.

There is no longer a Python fallback — the native extension is the
authoritative implementation. Stream A of the Rust orchestration migration
deliberately removed all Python-side fallback code paths.
"""

from pathlib import Path

from fastled._native import (
    find_sketch_by_partial_name as _native_find_sketch_by_partial_name,
)
from fastled._native import find_sketch_directories as _native_find_sketch_directories
from fastled._native import looks_like_fastled_repo as _native_looks_like_fastled_repo
from fastled._native import (
    looks_like_sketch_directory as _native_looks_like_sketch_directory,
)

__all__ = [
    "find_sketch_by_partial_name",
    "find_sketch_directories",
    "looks_like_fastled_repo",
    "looks_like_sketch_directory",
]


def find_sketch_directories(directory: Path | str | None = None) -> list[Path]:
    if directory is None:
        directory = Path(".")
    return [Path(p) for p in _native_find_sketch_directories(str(directory))]


def find_sketch_by_partial_name(
    partial_name: str, search_dir: Path | str | None = None
) -> Path:
    """Find a sketch directory by partial name match (delegates to Rust).

    Raises ``ValueError`` if no unique match is found.
    """
    if search_dir is None:
        search_dir = Path(".")
    return Path(_native_find_sketch_by_partial_name(partial_name, str(search_dir)))


def looks_like_fastled_repo(directory: Path | str = Path(".")) -> bool:
    return bool(_native_looks_like_fastled_repo(str(directory)))


def looks_like_sketch_directory(
    directory: Path | str | None, quick: bool = False
) -> bool:
    if directory is None:
        return False
    return bool(_native_looks_like_sketch_directory(str(directory), quick))
