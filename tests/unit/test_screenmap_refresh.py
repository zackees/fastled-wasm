"""Guard the late screen-map recovery path for sketches with no setup() map.

Sketches that never call ``setScreenMap()`` in ``setup()`` used to leave the
viewer with an empty canvas: the frontend only asked the wasm module for
screen-map data once, right after setup, and a strip added later (or a strip
whose layout arrives after that first read) never got picked up. These tests
pin the source-level contract for the fix: a shared strip-layout helper
module, a worker-side refresh that runs before each frame is rendered, and a
fixture wired into the wheel smoke test so a regression is caught in CI.
"""

from pathlib import Path

root = Path(__file__).resolve().parents[2]

FRONTEND = root / "src/fastled/frontend/modules/core"
WORKER = FRONTEND / "fastled_background_worker.ts"
SYNC = FRONTEND / "screenmap_sync.ts"
SMOKE = root / "ci/smoke_installed_wheel.sh"
FIXTURE = root / "tests/fixtures/wasm_no_screenmap/wasm_no_screenmap.ino"


def test_screenmap_sync_module_exports_layout_helpers() -> None:
    assert SYNC.is_file(), f"missing {SYNC}"
    text = SYNC.read_text(encoding="utf-8")
    assert "export function stripHasLayout(" in text
    assert "export function findStripsMissingLayout(" in text
    assert "export function frameNeedsScreenMapRefresh(" in text
    assert "strip_id" in text, "helper must key off the frame's strip_id field"


def test_worker_refetches_screenmaps_for_strips_without_layout() -> None:
    text = WORKER.read_text(encoding="utf-8")
    assert "import { findStripsMissingLayout } from './screenmap_sync.ts';" in text
    assert "function refreshScreenMapsIfIncomplete(" in text
    assert "refreshScreenMapsIfIncomplete(frameData);" in text
    assert (
        "MAX_SCREENMAP_REFRESH_ATTEMPTS" in text
    ), "per-strip retry cap is required to stop the polling"


def test_worker_refresh_runs_before_the_frame_is_rendered() -> None:
    worker_text = WORKER.read_text(encoding="utf-8")
    start = worker_text.index("async function executeFrameLoop(")
    end = worker_text.index("function extractFrameData(")
    slice_ = worker_text[start:end]

    frame_data_index = slice_.index("const frameData = extractFrameData();")
    refresh_index = slice_.index("refreshScreenMapsIfIncomplete(frameData);")
    post_index = slice_.index("postFrameToMainThread(frameData)")
    render_index = slice_.index("graphicsManager.updateCanvas(frameData)")

    assert (
        frame_data_index < refresh_index
    ), "refresh must run after extractFrameData() builds the lazy layout"
    assert (
        refresh_index < post_index
    ), "refresh must complete before the frame is posted to the main thread"
    assert (
        refresh_index < render_index
    ), "refresh must complete before the frame is rendered onto the canvas"


def test_worker_keeps_the_push_based_screenmap_update_path() -> None:
    text = WORKER.read_text(encoding="utf-8")
    assert "case 'screenmap_update':" in text
    assert "function handleScreenMapUpdate(payload)" in text
    assert (
        "Object.assign({}, workerState.screenMaps, screenMapData)" in text
    ), "late refresh must merge, not replace, pushed screenmap layouts"


def test_worker_does_not_reintroduce_a_single_shot_screenmap_read() -> None:
    worker_text = WORKER.read_text(encoding="utf-8")
    assert "function fetchScreenMapsFromWasm(" in worker_text
    assert (
        worker_text.count("getScreenMapData(screenMapSizePtr)") == 1
    ), "the post-setup read and the late refresh must share one helper"


def test_no_screenmap_render_check_is_wired_into_the_wheel_smoke() -> None:
    assert FIXTURE.is_file(), f"missing {FIXTURE}"
    fixture_text = FIXTURE.read_text(encoding="utf-8")
    # Strip `//` comments: the fixture's header explains why it must not call
    # setScreenMap(), so only the executable lines are checked.
    fixture_code = "\n".join(
        line.split("//", 1)[0] for line in fixture_text.splitlines()
    )
    assert (
        "setScreenMap" not in fixture_code
    ), "fixture must be the un-mapped case with no setup() screen map"

    smoke_text = SMOKE.read_text(encoding="utf-8")
    assert "wasm_no_screenmap" in smoke_text

    marker = "[fastled] late screenmap recovered"
    worker_text = WORKER.read_text(encoding="utf-8")
    assert marker in smoke_text, "wheel smoke must assert the recovery marker"
    assert marker in worker_text, "emitter and CI assertion must share one marker"
