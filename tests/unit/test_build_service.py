from pathlib import Path

from fastled.build_service import BuildService
from fastled.build_types import BuildRequest
from fastled.types import BuildMode


class FakeToolchain:
    def __init__(self) -> None:
        self.calls: list[tuple[Path, Path, BuildMode, bool]] = []

    def compile(
        self,
        sketch_dir: Path,
        output_dir: Path,
        build_mode: BuildMode,
        profile: bool,
    ) -> Path:
        self.calls.append((sketch_dir, output_dir, build_mode, profile))
        output_dir.mkdir(parents=True, exist_ok=True)
        (output_dir / "fastled.js").write_text("console.log('ok');")
        (output_dir / "fastled.wasm").write_bytes(b"\0asm")
        if build_mode == BuildMode.DEBUG:
            (output_dir / "fastled.wasm.dwarf").write_text("debug")
        assets_dir = output_dir / "assets"
        assets_dir.mkdir(exist_ok=True)
        (assets_dir / "main.js").write_text("asset")
        return output_dir / "fastled.js"


def test_build_service_starts_cold_then_turns_incremental(tmp_path: Path) -> None:
    service = BuildService()
    toolchain = FakeToolchain()
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    request = BuildRequest(sketch_dir=sketch_dir, build_mode=BuildMode.QUICK)

    service.register_toolchain(None, toolchain)

    assert service.detect_strategy(request) == "cold"
    first = service.build(request)
    assert first.success is True
    assert first.strategy == "cold"
    assert first.artifacts["js"] == sketch_dir / "fastled_js" / "fastled.js"
    assert first.artifacts["wasm"] == sketch_dir / "fastled_js" / "fastled.wasm"
    assert first.artifacts["frontend_assets"] == sketch_dir / "fastled_js" / "assets"

    second = service.build(request)
    assert second.success is True
    assert second.strategy == "incremental"
    assert len(toolchain.calls) == 2


def test_build_service_force_clean_resets_strategy(tmp_path: Path) -> None:
    service = BuildService()
    toolchain = FakeToolchain()
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()

    service.register_toolchain(None, toolchain)
    service.build(BuildRequest(sketch_dir=sketch_dir, build_mode=BuildMode.QUICK))

    request = BuildRequest(
        sketch_dir=sketch_dir,
        build_mode=BuildMode.DEBUG,
        force_clean=True,
    )
    result = service.build(request)

    assert result.strategy == "cold"
    assert result.artifacts["dwarf"] == sketch_dir / "fastled_js" / "fastled.wasm.dwarf"


def test_build_service_detects_incremental_across_service_instances(
    tmp_path: Path,
) -> None:
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    request = BuildRequest(sketch_dir=sketch_dir, build_mode=BuildMode.QUICK)

    first_service = BuildService()
    first_toolchain = FakeToolchain()
    first_service.register_toolchain(None, first_toolchain)
    first_result = first_service.build(request)

    assert first_result.strategy == "cold"

    second_service = BuildService()
    second_toolchain = FakeToolchain()
    second_service.register_toolchain(None, second_toolchain)

    assert second_service.detect_strategy(request) == "incremental"
    second_result = second_service.build(request)
    assert second_result.strategy == "incremental"
