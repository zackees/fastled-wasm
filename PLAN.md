# Plan: Remove Docker and Network Backends, Native-Only Toolchain

## Goal

Finish the migration to a single native Emscripten build path inside this repo.

After this refactor:

- There is no Docker build/runtime concept in `fastled-wasm`.
- There is no network or remote compile backend in `fastled-wasm`.
- Build invocation goes through a native toolchain API that can reuse hot state and automatically choose cold vs incremental behavior.
- Debug symbol source resolution is hosted by this repo for the local Flask frontend server.
- Tests are fast again and cover the native-only architecture.

## Current State

The repo is only partially migrated.

What is already true:

- CLI compilation already routes through native code in [src/fastled/app.py](./src/fastled/app.py) and [src/fastled/compile_native.py](./src/fastled/compile_native.py).
- Native compilation already delegates to FastLED's `ci/wasm_build.py` when available in [src/fastled/toolchain/emscripten.py](./src/fastled/toolchain/emscripten.py).
- Docker/server API exports are already gone from [src/fastled/__init__.py](./src/fastled/__init__.py).

What is still stale or incomplete:

- Docker assets and scripts still exist: [Dockerfile](./Dockerfile), [docker-compose.yml](./docker-compose.yml), [build_local_docker.py](./build_local_docker.py), [requirements.docker.txt](./requirements.docker.txt).
- The test runner still assumes Docker in [test](./test).
- CLI still carries deprecated `--local` in [src/fastled/parse_args.py](./src/fastled/parse_args.py).
- Packaging still depends on Flask/livereload stack in [pyproject.toml](./pyproject.toml), and the local preview server is still implemented in [src/fastled/server_flask.py](./src/fastled/server_flask.py).
- Docs and CI still advertise Docker/web compiler behavior in [README.md](./README.md) and `.github/workflows`.
- Debug symbol resolution does not live in this repo yet. The logic exists today in:
  - `~/dev/fastled-wasm-server/src/fastled_wasm_server/dwarf_utils.py`
  - `~/dev/fastled-wasm-compiler/src/fastled_wasm_compiler/dwarf_path_to_file_path.py`

## Target Architecture

### 1. Native-only build service

Introduce a stable build interface, separate from CLI concerns.

Proposed module shape:

- `src/fastled/build_service.py`
- `src/fastled/build_types.py`

Proposed API:

- `BuildRequest`
  - `sketch_dir: Path`
  - `build_mode: BuildMode`
  - `profile: bool`
  - `fastled_path: Path | None`
  - `force_clean: bool = False`
- `BuildResult`
  - wraps current compile result plus metadata
  - `strategy: Literal["cold", "incremental"]`
  - `output_dir: Path`
  - `artifacts: dict[str, Path]`
- `BuildService`
  - `build(request: BuildRequest) -> BuildResult`
  - `detect_strategy(request: BuildRequest) -> Literal["cold", "incremental"]`
  - `purge(...)`

Rules:

- The service decides cold vs incremental automatically.
- CLI and tests do not know the detection details.
- The service owns toolchain reuse and artifact discovery.
- Watch mode uses the same service instead of directly calling `compile_native()`.

### 2. Toolchain layering

Keep `EmscriptenToolchain` focused on compilation mechanics.

Responsibilities after refactor:

- `BuildService`: orchestration, strategy detection, toolchain instance reuse, artifact indexing.
- `EmscriptenToolchain`: execute compile/link/build-system delegation.
- CLI: parse args, call service, manage browser/watch loop.

### 3. Local debug symbol resolution service

Keep local Flask preview, but move debug source resolution into this repo so the browser debugger can resolve source files without the old server package.

Proposed module shape:

- `src/fastled/debug_symbols.py`
- `src/fastled/debug_routes.py`

Minimum responsibilities:

- Resolve DWARF-mapped request paths back to real source files.
- Read config from FastLED's `build_flags.toml` when available.
- Handle Windows Git Bash path normalization.
- Reject traversal and invalid paths.
- Serve source contents for browser debugging through the local Flask server.

This replaces the dependency on:

- `fastled-wasm-server` for `/dwarfsource`
- `fastled-wasm-compiler` for path resolution logic

## Workstreams

### Workstream 1: Formalize native build API

Files to change:

- [src/fastled/compile_native.py](./src/fastled/compile_native.py)
- [src/fastled/toolchain/emscripten.py](./src/fastled/toolchain/emscripten.py)
- [src/fastled/app.py](./src/fastled/app.py)

Tasks:

- Extract orchestration out of `compile_native.py` into `BuildService`.
- Make incremental behavior explicit through result metadata instead of hidden toolchain reuse.
- Add artifact discovery for:
  - `fastled.js`
  - `fastled.wasm`
  - `fastled.wasm.dwarf` when debug
  - symbol map if present
  - frontend assets directory
- Preserve current FastLED repo delegation to `ci/wasm_build.py`.
- Add a clean detection path:
  - cold build when build outputs or toolchain state are absent
  - incremental when prior build state exists and mode/toolchain match
  - forced cold when `purge` or explicit clean requested

Implementation note:

- The actual incremental compilation remains owned by upstream FastLED build caches.
- Our service should detect and report strategy, not try to reimplement Meson/Ninja invalidation.

### Workstream 2: Migrate debug symbol resolution into this repo

Source material to port:

- `~/dev/fastled-wasm-server/src/fastled_wasm_server/dwarf_utils.py`
- `~/dev/fastled-wasm-compiler/src/fastled_wasm_compiler/dwarf_path_to_file_path.py`

Tasks:

- Create a single canonical resolver in this repo.
- Prefer the compiler version's environment-aware behavior, but remove stale package coupling and debug prints.
- Support:
  - `fastledsource`
  - `sketchsource`
  - `dwarfsource`
  - EMSDK source mapping when debug info points there
- Add Flask routes for:
  - `POST /dwarfsource`
  - any future metadata endpoint needed by the frontend debugger
- Ensure the local server can serve source text from:
  - the sketch directory
  - the FastLED repo in use
  - EMSDK headers/sources when present

Acceptance criteria:

- `fastled --debug --app` still supports browser DWARF debugging.
- No dependency on `fastled-wasm-server` or `fastled-wasm-compiler` remains for source resolution.

### Workstream 3: Remove Docker and network leftovers

Delete:

- [Dockerfile](./Dockerfile)
- [docker-compose.yml](./docker-compose.yml)
- [build_local_docker.py](./build_local_docker.py)
- [requirements.docker.txt](./requirements.docker.txt)

Update:

- [install](./install)
- [test](./test)
- [README.md](./README.md)
- [.github/workflows/build_multi_docker_image.yml](./.github/workflows/build_multi_docker_image.yml)
- [.github/workflows/template_build_docker_image.yml](./.github/workflows/template_build_docker_image.yml)

Likely additional cleanup:

- remove old Docker badges and release references from docs
- remove dead `DEFAULT_URL`/remote wording where no longer used
- remove comments mentioning `--server`, `--web`, `--local`, or public compiler fallback

### Workstream 4: Tighten CLI and public API

Files to change:

- [src/fastled/parse_args.py](./src/fastled/parse_args.py)
- [src/fastled/args.py](./src/fastled/args.py)
- [src/fastled/__init__.py](./src/fastled/__init__.py)

Tasks:

- Remove deprecated `--local`.
- Keep the user-facing behavior simple: native build is the only build mode.
- Consider whether `--just-compile`, `--debug`, `--quick`, `--release`, `--purge`, `--app`, `--no-https`, `--fastled-path` stay as-is.
- Export the new build service from the package if public API support is desired.

### Workstream 5: Keep or simplify Flask server

Files to change:

- [src/fastled/server_flask.py](./src/fastled/server_flask.py)
- [src/fastled/open_browser.py](./src/fastled/open_browser.py)
- [pyproject.toml](./pyproject.toml)

Decision:

- Keep Flask if it is the easiest place to host the local debugger endpoints and HTTPS behavior.
- Remove `livereload`; it is no longer needed because rebuild/watch happens in Python already.

Tasks:

- Strip Flask server down to static file serving plus debug endpoints.
- Preserve headers needed for browser isolation and microphone tests.
- Keep HTTPS support for local secure contexts.
- Rename internals if needed so this is clearly a local preview/debug server, not a compile server.

## Test Plan

### Remove slow or stale backend assumptions

Delete or rewrite anything that encodes Docker as a prerequisite.

Immediate changes:

- Rewrite [test](./test) so it no longer serializes around shared Docker state and no longer rebuilds images.
- Remove workflow steps that invoke deleted Docker flags.
- Update Windows CI currently invoking `--web` in [.github/workflows/test_win.yml](./.github/workflows/test_win.yml).
- Update executable test currently invoking `--local` in [.github/workflows/test_build_exe.yml](./.github/workflows/test_build_exe.yml).

### Add native-only unit coverage

New tests to add:

- `tests/unit/test_build_service.py`
  - cold build detection
  - incremental detection
  - toolchain reuse behavior
  - artifact discovery for debug/quick/release
- `tests/unit/test_debug_symbols.py`
  - prefix pruning
  - Windows path normalization
  - traversal rejection
  - nonexistent path handling
  - FastLED/sketch/EMSDK path mapping
- `tests/unit/test_debug_routes.py`
  - `POST /dwarfsource` returns file contents
  - invalid requests return correct errors
- `tests/unit/test_parse_args.py`
  - no `--local`
  - native-only defaults remain correct

### Update existing tests

Files to update:

- [tests/unit/test_cli.py](./tests/unit/test_cli.py)
- [tests/unit/test_api.py](./tests/unit/test_api.py)
- [tests/unit/test_post_migration.py](./tests/unit/test_post_migration.py)
- [tests/integration/test_microphone_https.py](./tests/integration/test_microphone_https.py)
- [tests/integration/test_animartrix_e2e.py](./tests/integration/test_animartrix_e2e.py)
- [tests/integration/test_playwright_integration.py](./tests/integration/test_playwright_integration.py)

Specific additions:

- Assert the local server still serves correct MIME/security headers.
- Add an integration test that debug mode produces and exposes the expected debug artifacts.
- Add a local `/dwarfsource` integration test against the Flask server.

### Coverage target for migrated debug logic

Before deleting external dependencies, port the equivalent cases from:

- `~/dev/fastled-wasm-compiler/tests/unit/test_source_resolver.py`
- `~/dev/fastled-wasm-server/tests/test_api_client.py`

## Proposed Execution Order

1. Add the new build service abstraction without changing user-facing behavior.
2. Migrate DWARF/source-resolution logic into this repo.
3. Add Flask debug endpoints and tests.
4. Switch `compile_native.py` and watch mode to the new build service.
5. Remove Docker/network flags, scripts, docs, and CI references.
6. Simplify dependencies and remove `livereload`.
7. Run fast unit suite, then targeted integration suite, then full CI.

## Acceptance Criteria

- `fastled` compiles only through native Emscripten.
- No code, tests, docs, or workflows refer to Docker/web/server compile backends.
- Local preview/debug server still supports HTTPS and microphone-related browser requirements.
- `fastled --debug` still supports browser source resolution through local endpoints.
- Unit tests run without Docker and without network backends.
- CI no longer builds Docker images or exercises removed flags.

## Risks

- The biggest functional risk is debug symbol/source resolution, not compilation.
- The upstream FastLED build system controls the real incremental cache behavior, so our service must not drift from its artifact layout.
- Windows path handling for DWARF sources is easy to break and must be explicitly tested.
- If Flask is removed too aggressively, we can regress HTTPS headers or debugger endpoints; keep it until a smaller replacement fully covers those needs.

## Notes for Implementation

- Existing `PLAN.md` content assumes more removal has already happened than is actually true; this rewrite should be treated as the current source of truth.
- The old "compile server" is already gone from the main package; the remaining work is mostly local server cleanup, new native API surfacing, debug service migration, and repo-wide stale-reference removal.
