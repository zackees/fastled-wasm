# FastLED WASM Compiler

| | Build | Lint | Unit Test | Integration Test |
|---|---|---|---|---|
| **Linux x86** | [![Linux x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-build.yml) | [![Linux x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-lint.yml) | [![Linux x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-unit-test.yml) | [![Linux x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-x86-integration-test.yml) |
| **Linux ARM** | [![Linux ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-build.yml) | [![Linux ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-lint.yml) | [![Linux ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-unit-test.yml) | [![Linux ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/linux-arm-integration-test.yml) |
| **Windows x86** | [![Windows x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-build.yml) | [![Windows x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-lint.yml) | [![Windows x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-unit-test.yml) | [![Windows x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-x86-integration-test.yml) |
| **Windows ARM** | [![Windows ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-build.yml) | [![Windows ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-lint.yml) | [![Windows ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-unit-test.yml) | [![Windows ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/windows-arm-integration-test.yml) |
| **macOS x86** | [![macOS x86 Build](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-build.yml) | [![macOS x86 Lint](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-lint.yml) | [![macOS x86 Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-unit-test.yml) | [![macOS x86 Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-x86-integration-test.yml) |
| **macOS ARM** | [![macOS ARM Build](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-build.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-build.yml) | [![macOS ARM Lint](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-lint.yml) | [![macOS ARM Unit Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-unit-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-unit-test.yml) | [![macOS ARM Integration Test](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-integration-test.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/macos-arm-integration-test.yml) |

[![Publish Release](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml)

## Compile your FastLED sketch and run it on the browser!

![image](https://github.com/user-attachments/assets/243aeb4d-e42f-4cc3-9c31-0af51271f3e0)

## Demo

https://zackees.github.io/fastled-wasm/

## About

Compile FastLED sketches to browser-ready `html/js/wasm` output using a native Emscripten toolchain.

Compile times are extremely fast thanks to aggressive object caching and sketch fingerprinting. Recompilation of sketch files with minimal changes completes in under a second.

## Tutorial Video

> **Note:** This video predates the Rust CLI migration. Install with `pip install fastled` and run with `fastled mysketchfolder`.

https://github.com/user-attachments/assets/64ae0e6c-5f8b-4830-ab87-dcc25bc61218

## Install

```bash
pip install fastled
```

Alternative installers:

- `uv pip install fastled --system`
- `pipx install fastled`

### Executables

Pre-built binaries are published on each GitHub release:

- [Windows x64](https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-windows-x64.zip)
- [macOS ARM (M1+)](https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-macos-arm64.zip)
- [macOS x86](https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-macos-x64.zip)
- [Linux x64](https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-linux-x64.zip)

### Ubuntu Install Script

```bash
curl -L https://raw.githubusercontent.com/zackees/fastled-wasm/refs/heads/main/install_linux.sh | /bin/bash
```

## Run

Change into a sketch directory and run:

```bash
fastled
```

Useful flags:

| Flag | Description |
|------|-------------|
| `--just-compile` | Compile and exit without opening a browser or watching files |
| `--no-app` | Emit the JavaScript API/WASM artifacts without the default `index.js` application |
| `--link <static|dynamic>` | Static linking is the default; dynamic mode emits and load-links `sketch.wasm` |
| `--debug` | Build with debug-friendly compiler settings |
| `--quick` | Default build mode |
| `--release` | Optimized build (~1/3 smaller binary) |
| `--fastled-path <path>` | Point the build at a local FastLED checkout |
| `--purge` | Clear cached FastLED downloads and stale WASM build artifacts |
| `--serve-dir <dir>` | Serve an existing directory without compiling |

Set `FASTLED_VIEWER_LOGS=1` to forward the viewer window's browser console output (`console.*`, uncaught errors, failed fetches) to the terminal's stderr, prefixed with `[viewer]` — useful when diagnosing a blank or broken viewer.

### Testing your sketch from CI or an agent

The shipped viewer can render a sketch, capture its canvas, collect browser
logs, and exit without Playwright or a separate browser installation:

```bash
fastled examples/Blink --test --test-wait-secs=2 \
  --test-screenshot=out/blink.png --test-log=out/blink.viewer.log \
  --test-exit-on-error
```

For a frame sequence, add
`--test-interval-secs=0.5 --test-count=10` and use a path such as
`out/blink-{n:03}.png`. The interval is the target between scheduled capture
starts; slow canvas readback or uploads can delay later frames. The wait begins
only after the compiled page has a canvas and two animation frames have
elapsed. Exit codes are `0` for success,
`1` for compile/viewer/I/O failure, `2` for a captured page error when
`--test-exit-on-error` is enabled, `124` for the total timeout, and `125` for
the ready timeout. An interrupted run exits `130`. The viewer and local server
are stopped automatically.

To run trusted host checks after the first rendered frame, repeat
`--test-cmd` options. They execute sequentially from the sketch directory,
and the test succeeds only after both the viewer schedule and all commands
finish:

```bash
fastled examples/Blink --test --test-log=out/run.log \
  --test-cmd="python scripts/check_ready.py" \
  --test-cmd="python scripts/check_runtime_state.py"
```

Commands use `/bin/sh -c` on Unix and `%COMSPEC% /D /S /C` (normally
`cmd.exe`) on Windows, with null stdin and no extra console window. Pipes,
redirection, `&&`, and quoting follow the platform shell; invoke
`powershell -NoProfile -Command ...` explicitly for PowerShell syntax.
Command stdout and stderr are drained and mirrored to the terminal. With
`--test-log`, timestamped stream markers are appended, such as
`[fastled-test-cmd 0 stdout] ...` and `[fastled-test-cmd 0] exit=0`.
Commands are trusted input and inherit the user's environment and filesystem
permissions. A spawn failure or nonzero command exit returns `1`; the total
timeout (`124`) and interrupt (`130`) also terminate the active command and
its descendants.

## Features

### Browser Compatibility

Safari is a required production target. Release builds must not depend on Emscripten's JavaScript Promise Integration (JSPI), including `-sJSPI`, `WebAssembly.Suspending`, or `WebAssembly.promising`. Emscripten upgrades must preserve a non-JSPI async path and pass a real Safari smoke test before becoming the default toolchain.

### Hot Reload

Once launched, the compiler remains open and watches for file changes. Edits to your sketch are automatically recompiled and the browser reloads with the updated output. Build output is streamed to the browser in real time via SSE.

### Hot Reload FastLED Source

If you launch `fastled` inside the FastLED repo with `--fastled-path`, changes to the library source code are detected and trigger recompilation. Unlike sketch-only rebuilds, modifying a header file may produce a longer recompile.

### Big Data in `/data` Directory

Large files (e.g. video) in a sketch's `data/` directory are handled specially to avoid round-tripping blobs. The WASM compiler generates a `sketch_assets.json` manifest for an emulated SD card system mounted at `/data/`. Files named `*.json`, `*.csv`, `*.txt` are injected before `setup()` runs; all others are streamed asynchronously at runtime. The loader accepts legacy `files.json` output when running an older generated sketch.

For an example see `examples/SdCard` in the FastLED repo.

### Compile Speed

Three compile modes are available: `--quick` (default), `--release` (optimized for size, ~1/3 smaller binary), and `--debug` (debug-friendly compiler settings). Aggressive object caching means incremental rebuilds are near-instant.

### Arduino Compatibility

Most simple Arduino sketches compile out of the box. Common functions like `digitalWrite()`, `Serial.println()`, and others are stubbed. `digitalRead()` returns 0 and `analogRead()` returns random numbers.

## HTTPS

The local preview server supports HTTPS using the bundled localhost certificate pair. HTTPS is required for browser features such as microphone access. See [HTTPS_SSL.md](HTTPS_SSL.md).

## Python Package

The `fastled` Python package is a compatibility launcher for the native Rust
CLI. Internal build, project discovery, and fuzzy matching behavior lives in
Rust and is tested there.

## FAQs

**Q: Why does FastLED become choppy when the browser tab is in the background?**
A: FastLED WASM currently runs on the main thread. Chrome throttles the event loop for background tabs. Moving to a Web Worker would solve this.

**Q: Why does a long `delay()` freeze the browser?**
A: `delay()` blocks `loop()` which blocks the main thread. A Web Worker would decouple FastLED from the browser's UI thread.

**Q: How can I reduce the compiled size of my sketch?**
A: A significant portion of the binary is Emscripten JS bundling overhead. The `wasm_compiler_settings.py` in the FastLED repo can tune this. Using `--release` produces a ~1/3 smaller binary than `--quick`.

## Development

```bash
./install
./test
./lint
```
