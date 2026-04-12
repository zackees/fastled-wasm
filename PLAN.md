# Plan: Remove Docker and Network Backends, Native-Only Toolchain

## Status: Migration Complete

All workstreams below have been implemented. The project now compiles exclusively
through a native Emscripten toolchain. Docker, network/remote compile backends,
and the livereload dependency have been removed. The Rust CLI and PyO3 bridge are
built and verified end-to-end.

## Goal

Finish the migration to a single native Emscripten build path inside this repo.

After this refactor:

- There is no Docker build/runtime concept in `fastled-wasm`. ✅
- There is no network or remote compile backend in `fastled-wasm`. ✅
- Build invocation goes through a native toolchain API that can reuse hot state and automatically choose cold vs incremental behavior. ✅
- Debug symbol source resolution is hosted by this repo for the local Flask frontend server. ✅
- Tests are fast again and cover the native-only architecture. ✅

## Completed Workstreams

### Workstream 1: Formalize native build API ✅

- `src/fastled/build_service.py` — `BuildService`, `BuildRequest`, `BuildResult` implemented.
- `src/fastled/build_types.py` — `BuildMode`, supporting types.
- Incremental vs cold strategy is detected automatically and surfaced in `BuildResult.strategy`.
- Artifact discovery covers `fastled.js`, `fastled.wasm`, `fastled.wasm.dwarf`, and frontend assets.
- Forced cold path implemented via `BuildService.purge()`.
- Watch mode uses `BuildService` instead of calling `compile_native()` directly.

### Workstream 2: Migrate debug symbol resolution into this repo ✅

- `src/fastled/debug_symbols.py` — canonical resolver ported from `fastled-wasm-server` and `fastled-wasm-compiler`.
  - Supports `fastledsource`, `sketchsource`, `dwarfsource`, EMSDK source mapping.
  - Windows Git Bash path normalization handled.
  - Traversal and invalid path rejection.
- `src/fastled/debug_routes.py` — Flask routes for `POST /dwarfsource` and source serving.
- No dependency on `fastled-wasm-server` or `fastled-wasm-compiler` for source resolution.

### Workstream 3: Remove Docker and network leftovers ✅

Deleted:

- `Dockerfile`
- `docker-compose.yml`
- `build_local_docker.py`
- `requirements.docker.txt`

Updated:

- `install` — no Docker steps.
- `test` — runs native unit suite only.
- `README.md` — Docker badges and remote compiler references removed.
- `.github/workflows/build_multi_docker_image.yml` — deleted.
- `.github/workflows/template_build_docker_image.yml` — deleted.
- Dead `DEFAULT_URL`/remote wording removed throughout.

### Workstream 4: Tighten CLI and public API ✅

- `--local` deprecated flag removed from `parse_args.py`.
- `--server` and `--web` flags removed.
- `--serve-dir` added for static serving without recompile.
- Native build is the only build mode.
- `BuildService` exported from the package where needed.
- Rust CLI (`crates/fastled-cli`) mirrors every Python flag and delegates to `python -m fastled.app`.

### Workstream 5: Keep or simplify Flask server ✅

- Flask kept for static file serving and debug endpoints.
- `livereload` dependency removed; watch/rebuild is handled in Python.
- HTTPS and security headers preserved.
- Renamed internals to make it clearly a local preview/debug server.

## Rust Migration

### Overview

A Rust workspace has been introduced at the repo root with three crates:

- `crates/fastled-cli` — thin Rust front-end binary (`fastled.exe`) that mirrors all Python CLI flags and delegates to `python -m fastled.app`.
- `crates/fastled-py` — PyO3 extension module (`_native.pyd`) with availability probes used by the Python layer.
- `crates/fastled-tauri` — native Tauri viewer binary (`fastled-viewer.exe`) for rendering the compiled WASM without a browser.

### Verified ✅

- `_cargo build --workspace --release` — compiles cleanly.
- `_cargo test --workspace` — 47 Rust tests pass.
- `_cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `_cargo fmt --all --check` — clean.
- `fastled.exe --help` — correct output, all flags present.
- `fastled.exe --version` — reports `2.0.6`.
- PyO3 bridge: `version=2.0.6, watch=True, archive=True, project=True, build=True`.
- `bash lint` — zero errors.
- `bash test` — 121 Python tests pass.

### PyO3 Bridge API

```python
from fastled._native import (
    version,           # -> str: crate version
    watch_available,   # -> bool: native Rust watcher compiled in
    archive_available, # -> bool: native archive utilities compiled in
    project_available, # -> bool: native project init compiled in
    build_available,   # -> bool: native build orchestration compiled in
    viewer_available,  # -> bool: fastled-viewer binary reachable
)
```

## Test Coverage (Final)

### Unit tests added during migration

- `tests/unit/test_build_service.py` — cold/incremental detection, force-clean, cross-instance reuse.
- `tests/unit/test_debug_symbols.py` — prefix pruning, Windows path normalisation, traversal rejection, missing-file errors, FastLED/sketch/EMSDK mapping.
- `tests/unit/test_debug_routes.py` — `POST /dwarfsource` happy path and error cases.
- `tests/unit/test_parse_args.py` — no `--local`/`--server`/`--web`, `--serve-dir` present.
- `tests/unit/test_internal_wasm_build_cache.py` — incremental cache invalidation rules.
- `tests/unit/test_keyboard_interrupt_checker.py` — linting helper coverage.

### All tests run without Docker and without network backends.

## Acceptance Criteria — Final Status

| Criterion | Status |
|-----------|--------|
| `fastled` compiles only through native Emscripten | ✅ |
| No code/tests/docs/workflows refer to Docker/web/server backends | ✅ |
| Local preview/debug server still supports HTTPS and microphone requirements | ✅ |
| `fastled --debug` supports browser source resolution through local endpoints | ✅ |
| Unit tests run without Docker and without network backends | ✅ |
| CI no longer builds Docker images or exercises removed flags | ✅ |
| Rust CLI binary builds and passes `--help` / `--version` | ✅ |
| PyO3 bridge exports all availability probes | ✅ |

## Risks (Resolved)

- Debug symbol/source resolution — ported and tested. ✅
- Upstream FastLED build system artifact layout — `BuildService` stays aligned. ✅
- Windows path handling for DWARF sources — explicitly tested. ✅
- Flask HTTPS headers and debugger endpoints — preserved. ✅
