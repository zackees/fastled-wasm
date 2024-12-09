# FastLED Wasm compiler

Compiles an Arduino/Platformio sketch into a wasm binary that can be run directly in the web browser.


[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![Build and Push Multi Docker Image](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml)



# About

This python app will compile your FastLED style sketches into html/js/wasm output that runs directly in the browser.

Compile times are extremely fast, thanks to aggressive object caching for C++ and sketch fingerprinting with a zip file cache. Recompilation of sketch files with minimal changes will occure in less than a second.

By default the web compiler will always be used unless that user specifies `--local`, in which case this compiler will invoke docker to bring in a runtime necessary to run the compiler toolchain.

The local compiler will be much faster than the web version in most circumstances after the first compile. The web compiler
has the advantage that as a persistant service the compile cache will remain much more up to date.


https://github.com/user-attachments/assets/64ae0e6c-5f8b-4830-ab87-dcc25bc61218

# Demo

https://zackees.github.io/fastled-wasm/


# Install

```bash
pip install fastled
```

**Note that you may need to install x86 docker emulation on Mac-m1 and later, as this is an x86 only image at the prsent.**

# Use

Change to the directory where the sketch lives and run, will run the compilation
on the web compiler.

```bash
cd <SKETCH-DIRECTORY>
fastled
```

Or if you have docker you can run a server automatically.

```bash
cd <SKETCH-DIRECTORY>
fastled --local
```

You can also spawn a server in one process and then access it in another, like this:

```bash
fastled --server
# now launch the client
fastled examples/wasm --local
```

After compilation a web browser windows will pop up. Changes to the sketch will automatically trigger a recompilation.

# Hot reload by default

Once launched, the compiler will remain open, listening to changes and recompiling as necessary and hot-reloading the sketch into the current browser.

This style of development should be familiar to those doing web development.

# Hot Reload for working with the FastLED repo

If you launch `fastled` in the FastLED repo then this tool will automatically detect this and map the src directory into the
host container. Whenever there are changes in the source code from the mapped directory, then these will be re-compiled
on the next change or if you hit the space bar when prompted. Unlike a sketch folder, a re-compile on the FastLED src
can be much longer, for example if you modify a header file.

# Data

Huge blobs of data like video will absolutely kill the compile performance as these blobs would normally have to be shuffled
back and forth. Therefore a special directory `data/` is implicitly used to hold this blob data. Any data in this directory
will be replaced with a stub containing the size and hash of the file during upload. On download these stubs are swapped back
with their originals.

The wasm compiler will recognize all files in the `data/` directory and generate a `files.json` manifest and can be used
in your wasm sketch using an emulated SD card system mounted at `/data/` on the SD Card. In order to increase load speed, these
files will be asynchroniously streamed into the running sketch instance during runtime. The only caveat here is that although these files will be available during the setup() phase of the sketch, they will not be fully hydrated, so if you do a seek(end) of these files the results are undefined.

For an example of how to use this see `examples/SdCard` which is fully wasm compatible.

# Compile Speed

The compile speeds for this compiler have been optimized pretty much to the max. There are three compile settings available to the user. The default is `--quick`. Aggressive optimizations are done with `--release` which will aggressively optimize for size. The speed difference between `--release` and `--quick` seems negligable. But `--release` will produce a ~1/3 smaller binary. There is also `--debug`, which will include symbols necessary for debugging and getting the C++ function symbols working correctly in the browser during step through debugging. It works better than expected, but don't expect to have gdb or msvc debugger level of debugging experience.

We use `ccache` to cache object files. This seems actually help a lot and is better than platformio's method of tracking what needs to be rebuilt. This works as a two tier cache system. What Platformio misses will be covered by ccache's more advanced file changing system.

The compilation to wasm will happen under a lock. Removing this lock requires removing the platformio toolchain as the compiler backend which enforces it's own internal lock preventing parallel use.

Simple syntax errors will be caught by the pre-processing step. This happens without a lock to reduce the single lock bottleneck.

# Sketch Cache

Sketchs are aggresively finger-printed and stored in a cache. White space, comments, and other superficial data will be stripped out during pre-processing and minimization for fingerprinting. This source file decimation is only used for finger
printing while the actual source files are sent to compiler to preserve line numbers and file names.

This pre-processing done is done via gcc and special regex's and will happen without a lock. This will allow you to have extremely quick recompiles for whitespace and changes in comments even if the compiler is executing under it's lock.

# Local compiles

If the web-compiler get's congested then it's recommend that you run the compiler locally. This requires docker and will be invoked whenever you pass in `--local`. This will first pull the most recent Docker image of the Fastled compiler, launching a webserver and then connecting to it with the client once it's been up.

# Auto updates

In server mode the git repository will be cloned as a side repo and then periodically updated and rsync'd to the src directory. This allows a long running instance to stay updated.

### Wasm compatibility with Arduino sketchs

The compatibility is actually pretty good. Most simple sketchs should compile out of the box. Even some of the avr platform includes are stubbed out to make it work. The familiar `digitalWrite()`, `Serial.println()` and other common functions work. Although `digitalRead()` will always return 0 and `analogRead()` will return random numbers.

### Faqs

Q: How often is the docker image updated?
A: It's scheduled for rebuild once a day at 3am Pacific time, and also on every change to this repo.

Q: How can I run my own cloud instance of the FastLED wasm compiler?
A: Render.com (which fastled is hosted on) or DigialOcean can accept a github repo and auto-build the docker image.

Q: Why does FastLED tend to become choppy when the browser is in the background?
A: FastLED Wasm currently runs on the main thread and therefor Chrome will begin throttling the event loop when the browser is not in the foreground. The solution to this is to move FastLED to a web worker where it will get a background thread that Chrome / Firefox won't throttle.

Q: Why does a long `delay()` cause the browser to freeze and become sluggish?
A: `delay()` will block `loop()` which blocks the main thread of the browser. The solution is a webworker which will not affect main thread performance of the browser.


Q: How can I get the compiled size of my FastLED sketch smaller?
A: A big chunk of space is being used by unnecessary javascript `emscripten` is  bundling. This can be tweeked by the wasm_compiler_settings.py file in the FastLED repo.

# Revisions
  
  * 1.1.33 - Auto updating frequency has been reduced from one hour to one day. To update immediatly use `--update`.
  * 1.1.32 - `--init` now asks for which example you want, then tells you where the example was downloaded to. No longer auto-compiles.
  * 1.1.31 - `--local` is auto-enabled if docker is installed, use `--web` to force web compiler. Updating is much more pretty.
  * 1.1.30 - Added `--init` to initialize a demo project.
  * 1.1.29 - Remove annoying dbg messages i left in.
  * 1.1.28 - Adds cache control headers to the live server to disable all caching in the live browser.
  * 1.1.27 - Fixed `--interactive` so that it now works correctly.
  * 1.1.25 - Improved detecting which sketch directory the user means by fuzzy matching.
  * 1.1.24 - Adds progress spinning bar for pulling images, which take a long time.
  * 1.1.23 - Various fixes for MacOS
  * 1.1.22 - Selecting sketch now allows strings and narrowing down paths if ambiguity
  * 1.1.21 - Now always watches for space/enter key events to trigger a recompile.
  * 1.1.20 - Fixed a regression for 1.1.16 involving docker throwing an exception before DockerManager.is_running() could be called so it can be launched.
  * 1.1.19 - Automatically does a limit searches for sketch directories if you leave it blank.
  * 1.1.18 - Fixes for when the image has never been downloaded.
  * 1.1.17 - Added `--update` and `--no-auto-update` to control whether the compiler in docker mode will try to update.
  * 1.1.16 - Rewrote docker logic to use container suspension and resumption. Much much faster.
  * 1.1.15 - Fixed logic for considering ipv6 addresses. Auto selection of ipv6 is now restored.
  * 1.1.14 - Fixes for regression in using --server and --localhost as two instances, this is now under test.
  * 1.1.13 - Disable the use of ipv6. It produces random timeouts on the onrender server we are using for the web compiler.
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
