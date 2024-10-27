# FastLED wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml)

# Install

`pip install fastled-wasm`

# Use

Change to the directory where the sketch lives and run

```bash
fastled-wasm --open
```

The compiler should download, compile the target and then launch a webbrowser.
