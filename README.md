# FastLED WASM Compiler

| | Build | Lint | Unit Test | Integration Test |
|---|---|---|---|---|
| **Linux x86** | [![Linux x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-build.yml) | [![Linux x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-lint.yml) | [![Linux x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-unit-test.yml) | [![Linux x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-integration-test.yml) |
| **Linux ARM** | [![Linux ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-build.yml) | [![Linux ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-lint.yml) | [![Linux ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-unit-test.yml) | [![Linux ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-integration-test.yml) |
| **Windows x86** | [![Windows x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-build.yml) | [![Windows x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-lint.yml) | [![Windows x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-unit-test.yml) | [![Windows x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-integration-test.yml) |
| **Windows ARM** | [![Windows ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-build.yml) | [![Windows ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-lint.yml) | [![Windows ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-unit-test.yml) | [![Windows ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-integration-test.yml) |
| **macOS x86** | [![macOS x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-build.yml) | [![macOS x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-lint.yml) | [![macOS x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-unit-test.yml) | [![macOS x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-integration-test.yml) |
| **macOS ARM** | [![macOS ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-build.yml) | [![macOS ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-lint.yml) | [![macOS ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-unit-test.yml) | [![macOS ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-integration-test.yml) |

[![Build Webpage](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml)
[![Publish Release](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml)

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
