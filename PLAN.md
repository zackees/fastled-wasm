# Migration Plan: Remove Docker/Server/Web, Native-Only

## Goal

Remove all Docker, server, and web compilation infrastructure from fastled-wasm. The sole compilation path becomes native Emscripten via the `clang-tool-chain` package. Output remains WASM. Browser preview with file watching is preserved.

## Features Lost (acceptable)

- Dwarf file stack decoding (can add back later)
- Remote web compilation (fastled.onrender.com)
- Docker-based compile server (`--server` mode)
- Interactive Docker shell (`--interactive`)
- Docker image management (`--update`, `--purge`)

---

## Phase 1: Delete Dead Code (~8 files)

| File | Reason |
|------|--------|
| `src/fastled/docker_manager.py` | Docker API wrapper (~1,000 lines) |
| `src/fastled/compile_server.py` | Docker server facade (~112 lines) |
| `src/fastled/compile_server_impl.py` | Docker server implementation (~406 lines) |
| `src/fastled/server_flask.py` | Flask server (in-container) |
| `src/fastled/web_compile.py` | Remote web compilation client (~469 lines) |
| `src/fastled/client_server.py` | Client-server communication loop |
| `src/fastled/server_start.py` | Flask thread wrapper |
| `src/fastled/live_client.py` | Uses client_server.py |

---

## Phase 2: Modify Core Files (~6 files)

### `app.py`
- Remove `run_server()` function
- Remove Docker/web/server/interactive mode branches
- Keep native compile path as sole flow
- Keep `--install` handling
- Keep browser preview spawning

### `parse_args.py`
- Remove argument definitions for: `--docker`, `--web`, `--localhost`, `--server`, `--interactive`, `--update`, `--purge`, `--background-update`, `--no-platformio`, `--build`, `--no-auto-updates`, `--ram-disk-size`
- Remove Docker detection/fallback logic
- Remove `_try_start_server_or_get_url()` logic
- Keep: `--init`, `--debug`, `--release`, `--quick`, `--just-compile`, `--no-https`, `--app`, `--profile`, `--native`, `--fastled-path`

### `args.py`
- Remove fields: `web`, `interactive`, `server`, `localhost`, `update`, `background_update`, `build`, `purge`, `auto_update`, `no_platformio`
- Keep fields: `directory`, `native`, `debug`, `quick`, `release`, `profile`, `fastled_path`, `enable_https`, `app`, `just_compile`

### `settings.py`
- Remove: `CONTAINER_NAME`, `DEFAULT_CONTAINER_NAME`, `IMAGE_NAME`, `AUTH_TOKEN`, `DOCKER_FILE`, `DEFAULT_URL`
- Keep: `SERVER_PORT` (if still used for local preview)

### `__init__.py`
- Remove: `CompileServer` references, `Docker` class, `Api.spawn_server()`, `Api.server()`, `Api.web_compile()`
- Keep: `Api.project_init()`, basic exports

### `open_browser.py`
- Verify if Flask is used for local preview HTTP server
- If so, replace with `http.server` or similar lightweight alternative
- Keep HTTPS certificate handling if still needed

---

## Phase 3: Dependencies (`pyproject.toml`)

### Remove
- `docker>=7.1.0`
- `filelock>=3.16.1`
- `appdirs>=1.4.4`
- `rapidfuzz>=3.10.1`
- `progress>=1.6`
- `Flask>=3.0.0`
- `flask-cors>=4.0.0`
- `livereload`
- `websockify>=0.13.0`

### Add
- `clang-tool-chain` (hard dependency)

### Keep
- `httpx>=0.28.1`
- `watchdog>=6.0.0`
- `watchfiles>=1.0.5`
- `playwright>=1.40.0`
- `fasteners>=0.20`
- `cryptography>=41.0.0`
- `disklru>=2.0.4`

---

## Phase 4: Tests

### Delete (~12 Docker-dependent test files)
- `tests/unit/test_compile_server.py`
- `tests/unit/test_server_and_client_seperatly.py`
- `tests/unit/test_no_platformio_compile.py`
- `tests/unit/test_cli_no_platformio.py`
- `tests/unit/test_docker_linux_on_windows.py`
- `tests/unit/test_flask_headers.py`
- `tests/unit/test_https_server.py`
- `tests/unit/test_http_server.py`
- `tests/unit/test_session_compile.py`
- `tests/integration/test_libcompile.py`
- `tests/integration/test_build_examples.py`
- `tests/integration/test_examples.py`

### Modify (~3-4 files)
- `tests/unit/test_api.py` — Remove Docker/server API tests, keep `project_init` tests
- `tests/unit/test_manual_api_invocation.py` — Remove web/docker API tests
- `tests/unit/test_cli.py` — Remove tests for deleted flags

### Keep as-is
- `test_banner_string.py`, `test_bad_ino.py`, `test_version.py`, `test_filechanger.py`
- `test_string_diff.py`, `test_string_diff_comprehensive.py`
- `test_sketch_partial_match.py`, `test_select_sketch_directory.py`
- `test_emscripten_platform_neutral.py`, `test_header_dump.py`

### Cleanup
- `src/fastled/test/` subpackage — Remove `can_run_local_docker_tests()` and Docker helper utilities

---

## Phase 5: Cleanup

- Remove any dangling imports referencing deleted files
- Verify `compile_native.py` and `toolchain/emscripten.py` still work standalone
- Run `bash lint` and `bash test` to confirm clean state
- Bump version

---

## Open Items

1. **`open_browser.py` Flask dependency** — Needs investigation. If Flask is used for local preview, replace with lightweight alternative.
2. **`--native` flag** — Now the only mode. Keep as no-op for backwards compat or remove?
3. **`--emsdk-headers` flag** — Still useful for native compilation or Docker-only?
4. **`src/fastled/test/` subpackage** — Has Docker helper utilities that need cleanup.
