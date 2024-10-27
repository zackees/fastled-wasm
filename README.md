# FastLED wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml)


# About

This python app will compile your FastLED style sketches into html/js/wasm output that runs directly in the browser.

Compile times are extremely fast - I've seen as low as 5 seconds but 8-15 seconds is typical.

This works on Windows/Linux/Mac(arm/x64).

Docker is required.

https://github.com/user-attachments/assets/bde26ddd-d24d-4a78-90b6-ac05359677fa


# Demo

https://zackees.github.io/fastled-wasm/


# Install

```bash
pip install fastled-wasm
```

# Use

Change to the directory where the sketch lives and run

```bash
fastled-wasm --watch  # watches for changes in the ino/src file changes and re-compiles automatically.
```

The compiler should download, compile the target and then launch a web-browser.



### About the compilation.

Pre-processing is done to your source files. A fake Arduino.h will be inserted into your source files that will
provide shims for most of the common api points.



# Revisions

  1.0.4 - Implemented `--watch` which will watch for changes and then re-launch the compilation step.
  1.0.3 - Integrated `live-server` to launch when available.
  1.0.2 - Small bug with new installs.
  1.0.1 - Re-use is no longer the default, due to problems.
  1.0.0 - Initial release.
