# FastLED WASM Compiler

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml)
[![Test Build Executables](https://github.com/zackees/fastled-wasm/actions/workflows/test_build_exe.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_build_exe.yml)
[![Publish Release](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml)
[![Build Webpage](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml)

Compile FastLED sketches to browser-ready `html/js/wasm` output using a native Emscripten toolchain.

## Install

```bash
pip install fastled
```

Alternative installers:

- `uv pip install fastled --system`
- `pipx install fastled`

Executable downloads are published on the latest GitHub release for Windows, macOS, and Linux.

## Run

Change into a sketch directory and run:

```bash
fastled
```

Useful flags:

- `--just-compile` compiles and exits without opening a browser or watching files.
- `--debug` builds debug artifacts for browser DWARF debugging.
- `--quick` is the default build mode.
- `--release` builds an optimized binary.
- `--app` launches the local preview in the Playwright-based app-like browser.
- `--no-https` disables HTTPS for the local preview server.
- `--fastled-path <path>` points the build at a local FastLED checkout.
- `--purge` clears cached FastLED downloads and stale WASM build artifacts.

## Native Build Flow

The CLI now routes through a native-only build service that:

- selects cold versus incremental builds automatically
- reuses toolchain state across watch-mode rebuilds
- delegates to FastLED's upstream `ci/wasm_build.py` when available
- indexes the produced JS, WASM, DWARF, and frontend assets

## Debugging

`fastled --debug --app` serves the local preview over HTTPS and exposes local `/dwarfsource` resolution so browser devtools can map DWARF paths back to sketch, FastLED, and EMSDK source files.

For debugger setup details see [DEBUGGER.md](DEBUGGER.md).

## HTTPS

The local preview server supports HTTPS using the bundled localhost certificate pair. HTTPS is required for browser features such as microphone access.

## Python API

```python
from pathlib import Path

from fastled import Api, BuildMode, BuildRequest, BuildService

service = BuildService()
request = BuildRequest(
    sketch_dir=Path("path/to/sketch"),
    build_mode=BuildMode.QUICK,
)
result = service.build(request)
print(result.success, result.strategy, result.output_dir)

examples = Api.get_examples()
print(examples)
```

## Development

```bash
./install
./test
./lint
```

The local preview server used by tests is implemented in this repo and serves static assets plus debug-source routes.
