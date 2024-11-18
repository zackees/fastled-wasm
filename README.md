# FastLED wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.


[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![Build and Push Multi Docker Image](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml)


# About

This python app will compile your FastLED style sketches into html/js/wasm output that runs directly in the browser.

Compile times are extremely fast - I've seen as low as 5 seconds but 8-15 seconds is typical.

This works on Windows/Linux/Mac(arm/x64).

Docker is required.

https://github.com/user-attachments/assets/64ae0e6c-5f8b-4830-ab87-dcc25bc61218

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
cd <SKETCH-DIRECTORY>
fastled-wasm
```

Or if you don't have docker then use our web compiler

```bash
cd <SKETCH-DIRECTORY>
fastled-wasm --web
```

After compilation a web browser windows will pop up.

# Hot reload by default

Once launched, the compiler will remain open, listening to changes and recompiling as necessary and hot-reloading the sketch into the current browser.

This style of development should be familiar to those doing web development.

# Data

If you want to embed data, then do so in the `data/` directory at the project root. The files will appear in the `data/` director of any spawned FileSystem or SDCard.


### About the compilation.

Pre-processing is done to your source files. A fake Arduino.h will be inserted into your source files that will
provide shims for most of the common api points.


# Revisions

  * 1.1.12 - By default, fastled will default to the web compiler. `--localhost` to either attach to an existing server launched with `--server` or else one will be created automatically and launched.
  * 1.1.11 - Dev improvement: FastLED src code volume mapped into docker will just in time update without having to manually trigger it.
  * 1.1.10 - Swap large assets with embedded placeholders. This helps video sketches upload and compile instantly. Assets are re-added on after compile artifacts are returned.
  * 1.1.9 - Remove auto server and instead tell the user corrective action to take.
  * 1.1.8 - Program now knows it's own version which will be displayed with help file. Use `--version` to get it directly.
  * 1.1.7 - Sketch cache re-enabled, but selectively invalidated on cpp/h updates. Cleaned up deprecated args. Fixed double thread running for containers that was causing slowdown.
  * 1.1.6 - Use the fast src volume map allow quick updates to fastled when developing on the source code.
  * 1.1.5 - Filter out hidden files and directories from being included in the sketch archive sent to the compiler.
  * 1.1.4 - Fix regression introduced by testing out ipv4/ipv6 connections from a thread pool.
  * 1.1.3 - Live editing of *.h and *.cpp files is now possible. Sketch cache will be disabled in this mode.
  * 1.1.2 - `--server` will now volume map fastled src directory if it detects this. This was also implemented on the docker side.
  * 1.1.1 - `--interactive` is now supported to debug the container. Volume maps and better compatibilty with ipv4/v6 by concurrent connection finding.
  * 1.1.0 - Use `fastled` as the command for the wasm compiler.
  * 1.0.17 - Pulls updates when necessary. Removed dependency on keyring.
  * 1.0.16 - `fastled-wasm` package name has been changed to `fled`
  * 1.0.15 - `fled` is an alias of `fastled-wasm` and will eventually replace it. `--web-host` was folded into `--web`, which if unspecified will attempt to run a local docker server and fallback to the cloud server if that fails. Specifying `--web` with no arguments will default to the cloud server while an argument (like `localhost`) will cause it to bind to that already running server for compilation.
  * 1.0.14 - For non significant changes (comments, whitespace) in C++/ino/*.h files, compilation is skipped. This significantly reduces load on the server and prevents unnecessary local client browser refreshes.
  * 1.0.13 - Increase speed of local compiles by running the server version of the compiler so it can keep it's cache and not have to pay docker startup costs because now it's a persistant server until exit.
  * 1.0.12 - Added suppport for compile modes. Pass in `--release`, `--quick`, `--debug` for different compile options. We also support `--profile` to profile the build process.
  * 1.0.11 - `--web` compile will automatically be enabled if the local build using docker fails.
  * 1.0.10 - Watching files is now available for `--web`
  * 1.0.9 - Enabled web compile. Access it with `--web` or `--web-host`
  * 1.0.8 - Allow more than one fastled-wasm browser instances to co-exist by searching for unused ports after 8081.
  * 1.0.7 - Docker multi image build implemented, tool now points to new docker image compile.
  * 1.0.6 - Removed `--no-open` and `--watch`, `--watch` is now assumed unless `--just-compile` is used.
  * 1.0.5 - Implemented `--update` to update the compiler image from the docker registry.
  * 1.0.4 - Implemented `--watch` which will watch for changes and then re-launch the compilation step.
  * 1.0.3 - Integrated `live-server` to launch when available.
  * 1.0.2 - Small bug with new installs.
  * 1.0.1 - Re-use is no longer the default, due to problems.
  * 1.0.0 - Initial release.
