# FastLED wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/push_win.yml)


# About

This python app will compile your FastLED style sketches into html/js/wasm output that runs directly in the browser.

Compile times are **unreal** - faster than any other physical platform.

I've seen compiles as fast as 5 seconds, though they typically take around 10.

This works on Windows/Linux/Mac(arm/x64).

Docker is required.


https://github.com/user-attachments/assets/bde26ddd-d24d-4a78-90b6-ac05359677fa



# Install

`pip install fastled-wasm`

# Use

Change to the directory where the sketch lives and run

```bash
fastled-wasm --open
```

The compiler should download, compile the target and then launch a web-browser.



### About the compilation.

Pre-processing is done to your source files. A fake Arduino.h will be inserted into your source files that will
provide shims for most of the common api points.



# Revisions

  1.0.2 - Small bug with new installs.
  1.0.1 - Re-use is no longer the default, due to problems.
  1.0.0 - Initial release.
