# FastLED wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![Build Compiler for Docker amd64](https://github.com/zackees/fastled-wasm/actions/workflows/build_compiler_for_docker_amd64.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_compiler_for_docker_amd64.yml)
[![Build Compiler for Docker arm64](https://github.com/zackees/fastled-wasm/actions/workflows/build_compiler_for_docker_arm64.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_compiler_for_docker_arm64.yml)
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

**Note that you may need to install x86 docker emulation on Mac-m1 and later, as this is an x86 only image at the prsent.**

# Use

Change to the directory where the sketch lives and run

```bash
fastled-wasm
```

The compiler should download, compile the target and then launch a web-browser.

# Hot reload by default

Once launched, the compiler will remain open, listening to changes and recompiling as necessary and hot-reloading the sketch into the current browser.

This style of development should be familiar to those doing web development.

# Data

If you want to embed data, then do so in the `data/` directory at the project root. The files will appear in the `data/` director of any spawned FileSystem or SDCard.



### About the compilation.

Pre-processing is done to your source files. A fake Arduino.h will be inserted into your source files that will
provide shims for most of the common api points.



# Revisions

  * 1.0.6 - Removed `--no-open` and `--watch`, `--watch` is now assumed unless `--just-compile` is used.
  * 1.0.5 - Implemented `--update` to update the compiler image from the docker registry.
  * 1.0.4 - Implemented `--watch` which will watch for changes and then re-launch the compilation step.
  * 1.0.3 - Integrated `live-server` to launch when available.
  * 1.0.2 - Small bug with new installs.
  * 1.0.1 - Re-use is no longer the default, due to problems.
  * 1.0.0 - Initial release.
