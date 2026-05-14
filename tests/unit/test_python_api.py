from pathlib import Path

from fastled import (
    BuildArtifacts,
    BuildMode,
    BuildRequest,
    BuildService,
    CompileResult,
    __version__,
    is_in_order_match,
    string_diff,
)


def test_python_package_exports_native_api() -> None:
    assert __version__
    assert BuildMode.QUICK.value == "QUICK"
    assert BuildMode.from_string("release").value == "RELEASE"


def test_build_dtos_are_native_objects(tmp_path: Path) -> None:
    request = BuildRequest(tmp_path, BuildMode.QUICK)
    assert request.sketch_dir == tmp_path
    assert request.output_dir == tmp_path / "fastled_js"
    assert BuildService().detect_strategy(request) == "cold"

    result = CompileResult(True, "ok", zip_bytes=b"abc", sketch_time=1.25)
    assert bool(result)
    assert result.to_dict()["zip_bytes"] == b"abc"
    assert result.sketch_time == 1.25

    artifacts = BuildArtifacts(js=tmp_path / "fastled.js")
    assert artifacts["js"] == tmp_path / "fastled.js"
    assert artifacts.as_dict() == {"js": tmp_path / "fastled.js"}


def test_matching_helpers_are_native() -> None:
    assert is_in_order_match("wave 2d", "Wave2d")
    assert string_diff("FxWave", ["Wave2d", "FxWave2d"])[0][1] == "FxWave2d"
