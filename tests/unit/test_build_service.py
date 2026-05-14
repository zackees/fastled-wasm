from pathlib import Path

from fastled.build_types import BuildRequest
from fastled.types import BuildMode


class FakeNativeBuildService:
    states: dict[Path, tuple[str, bool, str | None]] = {}

    def __init__(self) -> None:
        self.registered: list[object] = []

    def register_toolchain(
        self, toolchain: object, fastled_path: str | None = None
    ) -> None:
        self.registered.append((toolchain, fastled_path))

    def detect_strategy(
        self,
        sketch_dir: str,
        build_mode: str,
        profile: bool = False,
        fastled_path: str | None = None,
        force_clean: bool = False,
    ) -> str:
        output_dir = Path(sketch_dir) / "fastled_js"
        state = self.states.get(Path(sketch_dir).resolve())
        if (
            force_clean
            or not (output_dir / "fastled.js").exists()
            or not (output_dir / "fastled.wasm").exists()
            or state != (build_mode, profile, fastled_path)
        ):
            return "cold"
        return "incremental"

    def build(
        self,
        sketch_dir: str,
        build_mode: str,
        build_mode_obj: BuildMode,
        profile: bool = False,
        fastled_path: str | None = None,
        force_clean: bool = False,
    ) -> dict[str, object]:
        del build_mode_obj
        strategy = self.detect_strategy(
            sketch_dir, build_mode, profile, fastled_path, force_clean
        )
        output_dir = Path(sketch_dir) / "fastled_js"
        output_dir.mkdir(parents=True, exist_ok=True)
        (output_dir / "fastled.js").write_text("console.log('ok');")
        (output_dir / "fastled.wasm").write_bytes(b"\0asm")
        if build_mode == "DEBUG":
            (output_dir / "fastled.wasm.dwarf").write_text("debug")
        assets_dir = output_dir / "assets"
        assets_dir.mkdir(exist_ok=True)
        (assets_dir / "main.js").write_text("asset")
        self.states[Path(sketch_dir).resolve()] = (build_mode, profile, fastled_path)
        return {
            "success": True,
            "stdout": "Native Rust WASM build successful",
            "hash_value": None,
            "zip_bytes": b"zip",
            "zip_time": 0.01,
            "libfastled_time": 0.0,
            "sketch_time": 0.02,
            "response_processing_time": 0.0,
            "strategy": strategy,
            "output_dir": str(output_dir),
            "artifacts": {
                "js": str(output_dir / "fastled.js"),
                "wasm": str(output_dir / "fastled.wasm"),
                "frontend_assets": str(assets_dir),
                **(
                    {"dwarf": str(output_dir / "fastled.wasm.dwarf")}
                    if build_mode == "DEBUG"
                    else {}
                ),
            },
        }

    def purge(self, sketch_dir: str) -> None:
        self.states.pop(Path(sketch_dir).resolve(), None)


def _install_fake_native(monkeypatch) -> None:
    FakeNativeBuildService.states = {}
    monkeypatch.setattr(
        "fastled.build_service._NativeBuildService", FakeNativeBuildService
    )


def test_build_service_starts_cold_then_turns_incremental(
    tmp_path: Path, monkeypatch
) -> None:
    _install_fake_native(monkeypatch)
    from fastled.build_service import BuildService

    service = BuildService()
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    request = BuildRequest(sketch_dir=sketch_dir, build_mode=BuildMode.QUICK)

    service.register_toolchain(None, object())

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


def test_build_service_force_clean_resets_strategy(tmp_path: Path, monkeypatch) -> None:
    _install_fake_native(monkeypatch)
    from fastled.build_service import BuildService

    service = BuildService()
    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()

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
    tmp_path: Path, monkeypatch
) -> None:
    _install_fake_native(monkeypatch)
    from fastled.build_service import BuildService

    sketch_dir = tmp_path / "sketch"
    sketch_dir.mkdir()
    request = BuildRequest(sketch_dir=sketch_dir, build_mode=BuildMode.QUICK)

    first_service = BuildService()
    first_result = first_service.build(request)

    assert first_result.strategy == "cold"

    second_service = BuildService()

    assert second_service.detect_strategy(request) == "incremental"
    second_result = second_service.build(request)
    assert second_result.strategy == "incremental"
