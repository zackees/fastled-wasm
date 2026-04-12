from pathlib import Path

import pytest  # type: ignore[reportMissingImports]

import fastled.debug_symbols as debug_symbols
from fastled.debug_symbols import (
    DebugSymbolResolver,
    load_debug_symbol_config,
    normalize_windows_path,
)


def test_normalize_windows_git_bash_path_from_zcmds_package(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr("platform.system", lambda: "Windows")
    git_bash_bin = tmp_path / "site-packages" / "zcmds_win32" / "git-bash-bin"
    git_bash_bin.mkdir(parents=True)

    class FakePackagePath:
        def __init__(self, path: Path) -> None:
            self._path = path

        def joinpath(self, child: str) -> Path:
            return self._path / child

    monkeypatch.setattr(
        debug_symbols.resources,
        "files",
        lambda package: FakePackagePath(tmp_path / "site-packages" / package),
    )
    assert (
        normalize_windows_path(f"{git_bash_bin.as_posix()}/js/src/main.cpp")
        == "/js/src/main.cpp"
    )


def test_debug_symbol_resolver_maps_sketch_fastled_and_emsdk(tmp_path: Path) -> None:
    sketch_dir = tmp_path / "sketch"
    fastled_dir = tmp_path / "FastLED"
    emsdk_dir = tmp_path / "emsdk"
    (sketch_dir / "src").mkdir(parents=True)
    (fastled_dir / "src").mkdir(parents=True)
    (emsdk_dir / "upstream" / "emscripten").mkdir(parents=True)
    sketch_file = sketch_dir / "src" / "demo.ino"
    fastled_file = fastled_dir / "src" / "FastLED.h"
    emsdk_file = emsdk_dir / "upstream" / "emscripten" / "cache.h"
    sketch_file.write_text("sketch")
    fastled_file.write_text("fastled")
    emsdk_file.write_text("emsdk")

    resolver = DebugSymbolResolver(
        load_debug_symbol_config(
            sketch_dir=sketch_dir,
            fastled_dir=fastled_dir,
            emsdk_path=emsdk_dir,
        )
    )

    assert resolver.resolve("sketchsource/src/demo.ino") == sketch_file
    assert resolver.resolve("fastledsource/FastLED.h") == fastled_file
    assert (
        resolver.resolve("dwarfsource/emsdk/upstream/emscripten/cache.h") == emsdk_file
    )


def test_debug_symbol_resolver_rejects_traversal(tmp_path: Path) -> None:
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    resolver = DebugSymbolResolver(load_debug_symbol_config(sketch_dir=sketch_dir))

    with pytest.raises(ValueError):
        resolver.resolve("dwarfsource/../../secret.txt")


def test_debug_symbol_resolver_missing_file_errors(tmp_path: Path) -> None:
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    resolver = DebugSymbolResolver(load_debug_symbol_config(sketch_dir=sketch_dir))

    with pytest.raises(FileNotFoundError):
        resolver.resolve("sketchsource/missing.cpp")
