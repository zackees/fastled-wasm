# FastLED Wasm compiler

[![Linting](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/lint.yml)
[![MacOS_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_macos.yml)
[![Ubuntu_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_ubuntu.yml)
[![Win_Tests](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_win.yml)
[![Test Build Executables](https://github.com/zackees/fastled-wasm/actions/workflows/test_build_exe.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/test_build_exe.yml)

[![Build and Push Multi Docker Image](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_multi_docker_image.yml)
[![Publish Release](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/publish_release.yml)
[![Build Webpage](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml/badge.svg)](https://github.com/zackees/fastled-wasm/actions/workflows/build_webpage.yml)


## Compile your FastLED sketch and run it on the Browser!!

![image](https://github.com/user-attachments/assets/243aeb4d-e42f-4cc3-9c31-0af51271f3e0)

# Demo

https://zackees.github.io/fastled-wasm/


# About

This python app will compile your FastLED style sketches into html/js/wasm output that runs directly in the browser.

Compile times are extremely fast, thanks to aggressive object caching for C++ and sketch fingerprinting with a zip file cache. Recompilation of sketch files with minimal changes will occure in less than a second.

If you have docker installed, the compiler will download the docker image and run a private local server on your machine. If you don't have Docker installed then the app will fall back to using the public web compiler.


In every conceivable way, the local compiler will be much faster than the web version.


# Run

Once `fastled` is installed you'll just navigate to your sketch directory and run it.

Example:

```bash
$ cd mysketchdirectory
$ fastled
```

## App-like Browser Support

FastLED includes Playwright for enhanced browser control and automation capabilities. To use the app-like browser experience, use the `--app` flag:

```bash
$ fastled --app my_sketch
```

The `--app` flag provides better automation capabilities and is especially useful for:
- Automated testing of your LED sketches
- Consistent cross-platform browser behavior
- Advanced debugging and development workflows
- Persistent browser sessions that stay open until the FastLED process exits

**Key Benefits:**
- Triggered by the `--app` flag - automatically downloads browsers if needed
- The Playwright browser remains open throughout your development session
- Automatic cleanup when the FastLED process exits
- Better control over browser behavior and automation capabilities
- Consistent behavior across different platforms

If you don't use the `--app` flag, the system will use your default browser.

# Install

This is a python app, so any python package manager will work. We also provide python compiled binaries for Windows, MacOS, and Linux.

### Pip

`pip install fastled`

### UV

`uv pip install fastled --system`

### Pipx

`pipx install fastled`

### Executables

  * Windows: https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-windows-x64.zip
  * MacOS (M1+): https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-macos-arm64.zip
  * MacOS (x86): https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-macos-x64.zip
  * Linux (x86): https://github.com/zackees/fastled-wasm/releases/latest/download/fastled-linux-x64.zip



### Ubuntu Install Script

You can get the latest ubuntu binary of the FastLED cli compiler like this:

```
curl -L https://raw.githubusercontent.com/zackees/fastled-wasm/refs/heads/main/install_linux.sh | /bin/bash
```


# Tutorial video

**Note this video is a little outdated, you will install the app now with `pip install fastled` and run it like `fastled mysketchfolder`**

https://github.com/user-attachments/assets/64ae0e6c-5f8b-4830-ab87-dcc25bc61218



# Install

```bash
pip install fastled
```

# Debugger

This tool features full C++ debugging through the browser.

For instructions on setup see [DEBUGGER.md](DEBUGGER.md)

# Command Line Use

Change to the directory where the sketch lives and run, will run the compilation
on the web compiler.

```bash
# This will use the web-compiler, unless you have docker installed in which case a local
# server will be instantiated automatically.
cd <SKETCH-DIRECTORY>
fastled
```

Forces the local server to to spawn in order to run to do the compile.

```bash
cd <SKETCH-DIRECTORY>
fastled --local  # if server doesn't already exist, one is created.
```

You can also spawn a server in one process and then access it in another, like this:

```bash
fastled --server  # server will now run in the background.
# now launch the client
fastled examples/wasm --local  # local will find the local server and use it to do the compile.
```

After compilation a web browser windows will pop up. Changes to the sketch will automatically trigger a recompilation.

# Python Api

**Compiling through the api**
```python

from fastapi import Api, CompileResult

out: CompileResult = Api.web_compile("path/to/sketch")
print(out.success)
print(out.stdout)

```

**Launching a compile server**
```python

from fastapi import Api, CompileServer

server: CompileServer = Api.spawn_server()
print(f"Local server running at {server.url()}")
server.web_compile("path/to/sketch")  # output will be "path/to/sketch/fastled_js"
server.stop()
```

**Launching a server in a scope**
```python

from fastapi import Api

# Launching a server in a scope
with Api.server() as server:
    server.web_compile("path/to/sketch")

```

**Initializing a project example from the web compiler**
```python

from fastapi import Api

examples = Api.get_examples()
print(f"Print available examples: {examples}")
Api.project_init(examples[0])


```

**Initializing a project example from the CompileServer**
```python

from fastapi import Api

with Api.server() as server:
    examples = server.get_examples()
    server.project_init(examples[0])

```

**LiveClient will auto-trigger a build on code changes, just like the cli does**
```python

# Live Client will compile against the web-compiler
from fastapi import Api, LiveClient
client: LiveClient = Api.live_client(
    "path/to/sketch_directory",
)
# Now user can start editing their sketch and it will auto-compile
# ... after a while stop it like this.
client.stop()
```

**LiveClient with local CompileServer**
```python

# Live Client will compile against a local server.
from fastapi import Api, LiveClient

with Api.server() as server:
    client: LiveClient = Api.live_client(
        "path/to/sketch_directory",
        host=server
    )
    # Now user can start editing their sketch and it will auto-compile
    # ... after a while stop it like this.
    client.stop()
```

**Build Docker Image from a local copy of the FastLED repo**
```python
from fastapi import Docker, Api
container_name: str = Docker.spawn_server_from_fastled_repo()
with Api.server(container_name=container_name) as server:
  ...
```

# Features

## Hot reload by default

Once launched, the compiler will remain open, listening to changes and recompiling as necessary, hot-reloading the sketch into the current browser.

This style of development should be familiar to those doing web development.

## Hot Reload fastled/src when working in the FastLED repo

If you launch `fastled` in the FastLED repo then this tool will automatically detect this and map the src directory into the
host container. Whenever there are changes in the source code from the mapped directory, then these will be re-compiled
on the next change or if you hit the space bar when prompted. Unlike a sketch folder, a re-compile on the FastLED src
can be much longer, for example, if you modify a header file.

## Big Data in `/data` directory won't be round-tripped

Huge blobs of data like video will absolutely kill the compile performance as these blobs would normally have to be shuffled
back and forth. Therefore a special directory `data/` is implicitly used to hold this blob data. Any data in this directory
will be replaced with a stub containing the size and hash of the file during upload. On download, these stubs are swapped back
with their originals during decompression.

The wasm compiler will recognize all files in the `data/` directory and generate a `files.json` manifest which can be used
in your wasm sketch using an emulated SD card system mounted at `/data/` on the SD Card. In order to increase load speed, these
files will be asynchronously streamed into the running sketch instance during runtime. Files named with *.json, *.csv, *.txt will be
immediately injected in the app before setup() is called and can be used immediately in setup() in their entirety.

All other files will be streamed in. The `Video` element in FastLED is designed to gracefully handle missing data streamed in through
the file system.

For an example of how to use this see `examples/SdCard` which is fully wasm compatible.

## Compile Speed

There are three compile settings available to the user. The default is `--quick`. Aggressive optimizations are done with `--release` which will optimize for size, although the speed difference between `--release` and `--quick` seems negligible. But `--release` will produce a ~1/3 smaller binary. There is also `--debug`, which will include symbols necessary for debugging and getting the C++ function symbols working correctly in the browser during step-through debugging. In my personal tests it works better than expected, but don't expect to have gdb or msvc debugger level of the debugging experience.

We use `ccache` to cache object files. This seems actually help a lot and is better than Platformio's method of tracking what needs to be rebuilt. This works as a two-tier cache system. What Platformio misses will be covered by ccache's more advanced file changing system.

The compilation to wasm will happen under a lock. Removing this lock requires removing the Platformio toolchain as the compiler backend which enforces its own internal lock preventing parallel use.

## Sketch Cache

Sketches are aggressively fingerprinted and stored in a cache. White space, comments, and other superficial data will be stripped out during pre-processing and minimization for fingerprinting. This source file decimation is only used for finger
printing while the actual source files are sent to the compiler to preserve line numbers and file names.

This pre-processing done is done via gcc and special regex's and will happen without a lock. This will allow you to have extremely quick recompiles for whitespace and changes in comments.

## Local compiles

If the web compiler gets congested then it's recommended that you run the compiler locally. This requires docker and will be invoked whenever you pass in `--local`. This will first pull the most recent Docker image of the Fastled compiler, launch a webserver, and then connect to it with the client once it's been up.

## Auto updates

In server mode, the git repository will be cloned as a side repo and then periodically updated and rsync'd to the src directory. This allows a long-running instance to stay updated.

## Compatibility with Arduino sketches

The compatibility is pretty good. Most simple sketches should compile out of the box. Even some of the AVR platforms are stubbed out to make it work. For Arduino, the familiar `digitalWrite()`, `Serial.println()`, and other common functions work. Although `digitalRead()` will always return 0 and `analogRead()` will return random numbers.

### Faqs

Q: How often is the docker image updated?
A: It's scheduled for rebuild once a day at 3am Pacific time, and also on every change to this repo.

Q: How can I run my own cloud instance of the FastLED wasm compiler?
A: Render.com (which fastled is hosted on) or DigialOcean can accept a GitHub repo and auto-build the docker image.

Q: Why does FastLED tend to become choppy when the browser is in the background?
A: FastLED Wasm currently runs on the main thread and therefor Chrome will begin throttling the event loop when the browser is not in the foreground. The solution to this is to move FastLED to a web worker where it will get a background thread that Chrome / Firefox won't throttle.

Q: Why does a long `delay()` cause the browser to freeze and become sluggish?
A: `delay()` will block `loop()` which blocks the main thread of the browser. The solution is a webworker which will not affect the main thread performance of the browser.


Q: How can I get the compiled size of my FastLED sketch smaller?
A: A big chunk of space is being used by unnecessary javascript `emscripten` bundling. The wasm_compiler_settings.py file in the FastLED repo can tweak this.


# Revisions
  * 1.4.00 - Browser now uses playwright when `--app` flag is used. Much better app like experience.
  * 1.2.31 - Bunch of fixes and ease of use while compiling code in the repo.
  * 1.2.22 - Prefer to use `live-server` from npm. If npm exists on the system then do a background install of `live-server` for next run.
  * 1.2.20 - Fixed up path issue for web browser launch for hot reload.
  * 1.2.19 - Compilation failure now overwrites the index.html file with error message.
  * 1.2.16 - Force mime types in web browser as some users may not have it correct in their registry.
  * 1.2.15 - Rewrote http server using flask. Reduced install size significantly.
  * 1.2.13 - More fixes for npm live-server, now uses `static-npm` to launch it.
  * 1.2.12 - Fixes for python npm for launching live-server
  * 1.2.10 - `fastled.Test` now allows you to spawn the http webserver on a folder with an index.html in it.
  * 1.2.7 - A bunch of fixes for MacOS and probably linux.
  * 1.2.6 - Now builds image from the project root of FastLED.
  * 1.1.25 - Fix up paths for `--init`
  * 1.1.24 - Mac/Linux now properly responds to ctrl-c when waiting for a key event.
  * 1.1.23 - Fixes missing `live-server` on platforms that don't have it already.
  * 1.2.22 - Added `--purge` and added docker api at __init__.
  * 1.2.00 - `fastled.exe` is now a signed binary on windows, however it's a self signed binary so you'll still get the warning on the first open. There's been a small api change between the server and the client for fetching projects.
  * 1.1.69 - Changed the binary name to `fastled.exe` instead of something like `fastled-windows-x64.exe`
  * 1.1.68 - Add a site builder to fastled.Test which generates a website with a bunch of demos. This is used to build the demo site automatically.
  * 1.1.67 - Pinned all the minimum versions of dependencies so we don't bind to an out of date py dep: https://github.com/zackees/fastled-wasm/issues/3
  * 1.1.61 - Excluded non compiling examples from the Test object as part of the api - no sense in having them if they won't compile.
  * 1.1.60 - Platform executables (macos-arm/macos-x86/windows/linux-x86) now auto building with each release. Add tests.
  * 1.1.52 - Add linux-arm
  * 1.1.49 - Try again.
  * 1.1.46 - Add mac x86 exe
  * 1.1.45 - Another try for web publishing from github.
  * 1.1.42 - Second test for web publishing from github.
  * 1.1.41 - Platform executable (through pyinstaller) now enabled.
  * 1.1.40 - Remove `sketch_directory` from Api object. This was only needed before we had a client/server architecture.
  * 1.1.39 - Added `LiveClient`, `fastled.Api.live_server()` will spawn it. Allows user to have a live compiling client that re-triggers a compile on file changes.
  * 1.1.38 - Cleanup the `fastled.Api` object and streamline for general use.
  * 1.1.37 - `Test.test_examples()` is now unit tested to work correctly.
  * 1.1.36 - We now have an api. `from fastled import Api` and `from fastled import Test` for testing.
  * 1.1.35 - When searching for files cap the limit at a high amount to prevent hang.
  * 1.1.34 - On windows check to make sure we are in linux container mode, if not try to switch and if that fails then use `--web` compiler.
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


# TODO
  * `live-server --https --cert=localhost.pem --key=localhost-key.pem --port=5500`
  * `live-server --port=8416 --host=localhost . --https --cert=C:/Users/niteris/dev/fastled-wasm/src/fastled/assets/localhost-key.pem --key=C:/Users/niteris/dev/fastled-wasm/src/fastled/assets/localhost.pem --no-browser`




  live-server --https --port=8416 --host=localhost .  --cert=C:/Users/niteris/dev/fastled-wasm/src/fastled/assets/localhost-key.pem --key=C:/Users/niteris/dev/fastled-wasm/src/fastled/assets/localhost.pem --no-browser


  live-server --https --cert=src/fastled/assets/localhost.pem --key=src/fastled/assets/localhost-key.pem --port=5500



## Performance


```
###############################
# Build started with command: #
#   pio run -v                #
###############################

0.41 Warning! Ignore unknown configuration option `build_dir` in section [env:wasm]
0.41 Warning! Ignore unknown configuration option `force_verbose` in section [env:wasm]
0.41 Processing wasm (platform: native; extra_scripts: post:wasm_compiler_flags.py; lib_compat_mode: off; build_flags: -std=c++17, -DFASTLED_NO_PINMAP, -DHAS_HARDWARE_PIN_SUPPORT, -DFASTLED_FORCE_SOFTWARE_SPI, -I/js/src/platforms/wasm/compiler; custom_wasm_export_name: fastled; lib_deps: FastLED=symlink://fastled; build_dir: build/wasm; force_verbose: yes)
0.41 --------------------------------------------------------------------------------
0.99 LDF: Library Dependency Finder -> https://bit.ly/configure-pio-ldf
0.99 LDF Modes: Finder ~ chain, Compatibility ~ off
0.99 Found 1 compatible libraries
0.99 Scanning dependencies...
1.40 Dependency Graph
1.40 |-- FastLED @ 3.9.16 (License: MIT, Path: /js/fastled)
1.49 Building in release mode
1.49
1.49 #########################
1.49 # C++/C Compiler Flags: #
1.49 #########################
1.49
1.49 CC/CXX flags:
1.49   -DFASTLED_ENGINE_EVENTS_MAX_LISTENERS=50
1.49   -DFASTLED_FORCE_NAMESPACE=1
1.49   -DFASTLED_USE_PROGMEM=0
1.49   -O1
1.49   -DUSE_OFFSET_CONVERTER=0
1.49   -std=gnu++17
1.49   -fpermissive
1.49   -Wno-constant-logical-operand
1.49   -Wnon-c-typedef-for-linkage
1.49   -Werror=bad-function-cast
1.49   -Werror=cast-function-type
1.49   -I
1.49   src
1.49   -I/js/src/platforms/wasm/compiler
1.49
1.49 #################
1.49 # Linker Flags: #
1.49 #################
1.49
1.49   --bind
1.49   -fuse-ld=lld
1.49   -sWASM=1
1.49   -sALLOW_MEMORY_GROWTH=1
1.49   -sINITIAL_MEMORY=134217728
1.49   -sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','stringToUTF8','lengthBytesUTF8']
1.49   -sEXPORTED_FUNCTIONS=['_malloc','_free','_extern_setup','_extern_loop','_fastled_declare_files']
1.49   --no-entry
1.49   -sERROR_ON_WASM_CHANGES_AFTER_LINK
1.49   -sMODULARIZE=1
1.49   -sEXPORT_NAME=fastled
1.49   -o
1.49   /js/.pio/build/wasm/fastled.js
1.49
1.49 ##########################
1.49 # FastLED Library Flags: #
1.49 ##########################
1.49
1.49   --bind
1.49   -Wl,--whole-archive,-fuse-ld=lld
1.49   -Werror=bad-function-cast
1.49   -Werror=cast-function-type
1.49
1.49 ################
1.49 # End of Flags #
1.49 ################
1.49
2.20 ccache em++ -o .pio/build/wasm/program --bind -fuse-ld=lld -sWASM=1 -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=134217728 -sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','stringToUTF8','lengthBytesUTF8'] -sEXPORTED_FUNCTIONS=['_malloc','_free','_extern_setup','_extern_loop','_fastled_declare_files'] --no-entry -sERROR_ON_WASM_CHANGES_AFTER_LINK -sMODULARIZE=1 -sEXPORT_NAME=fastled -o /js/.pio/build/wasm/fastled.js .pio/build/wasm/libbb9/fastled/FastLED.o .pio/build/wasm/libbb9/fastled/bitswap.o .pio/build/wasm/libbb9/fastled/cled_controller.o .pio/build/wasm/libbb9/fastled/colorpalettes.o .pio/build/wasm/libbb9/fastled/crgb.o .pio/build/wasm/libbb9/fastled/fl/allocator.o .pio/build/wasm/libbb9/fastled/fl/audio.o .pio/build/wasm/libbb9/fastled/fl/bilinear_expansion.o .pio/build/wasm/libbb9/fastled/fl/blur.o .pio/build/wasm/libbb9/fastled/fl/bytestreammemory.o .pio/build/wasm/libbb9/fastled/fl/colorutils.o .pio/build/wasm/libbb9/fastled/fl/downscale.o .pio/build/wasm/libbb9/fastled/fl/engine_events.o .pio/build/wasm/libbb9/fastled/fl/fft.o .pio/build/wasm/libbb9/fastled/fl/fft_impl.o .pio/build/wasm/libbb9/fastled/fl/file_system.o .pio/build/wasm/libbb9/fastled/fl/fill.o .pio/build/wasm/libbb9/fastled/fl/five_bit_hd_gamma.o .pio/build/wasm/libbb9/fastled/fl/gradient.o .pio/build/wasm/libbb9/fastled/fl/json.o .pio/build/wasm/libbb9/fastled/fl/leds.o .pio/build/wasm/libbb9/fastled/fl/line_simplification.o .pio/build/wasm/libbb9/fastled/fl/noise_woryley.o .pio/build/wasm/libbb9/fastled/fl/ptr.o .pio/build/wasm/libbb9/fastled/fl/raster_sparse.o .pio/build/wasm/libbb9/fastled/fl/rectangular_draw_buffer.o .pio/build/wasm/libbb9/fastled/fl/screenmap.o .pio/build/wasm/libbb9/fastled/fl/sin32.o .pio/build/wasm/libbb9/fastled/fl/str.o .pio/build/wasm/libbb9/fastled/fl/strstream.o .pio/build/wasm/libbb9/fastled/fl/stub_main.o .pio/build/wasm/libbb9/fastled/fl/tile2x2.o .pio/build/wasm/libbb9/fastled/fl/time_alpha.o .pio/build/wasm/libbb9/fastled/fl/transform.o .pio/build/wasm/libbb9/fastled/fl/type_traits.o .pio/build/wasm/libbb9/fastled/fl/ui.o .pio/build/wasm/libbb9/fastled/fl/wave_simulation.o .pio/build/wasm/libbb9/fastled/fl/wave_simulation_real.o .pio/build/wasm/libbb9/fastled/fl/xmap.o .pio/build/wasm/libbb9/fastled/fl/xymap.o .pio/build/wasm/libbb9/fastled/fl/xypath.o .pio/build/wasm/libbb9/fastled/fl/xypath_impls.o .pio/build/wasm/libbb9/fastled/fl/xypath_renderer.o .pio/build/wasm/libbb9/fastled/fx/2d/blend.o .pio/build/wasm/libbb9/fastled/fx/2d/noisepalette.o .pio/build/wasm/libbb9/fastled/fx/2d/scale_up.o .pio/build/wasm/libbb9/fastled/fx/2d/wave.o .pio/build/wasm/libbb9/fastled/fx/frame.o .pio/build/wasm/libbb9/fastled/fx/fx_engine.o .pio/build/wasm/libbb9/fastled/fx/time.o .pio/build/wasm/libbb9/fastled/fx/video.o .pio/build/wasm/libbb9/fastled/fx/video/frame_interpolator.o .pio/build/wasm/libbb9/fastled/fx/video/frame_tracker.o .pio/build/wasm/libbb9/fastled/fx/video/pixel_stream.o .pio/build/wasm/libbb9/fastled/fx/video/video_impl.o .pio/build/wasm/libbb9/fastled/hsv2rgb.o .pio/build/wasm/libbb9/fastled/lib8tion.o .pio/build/wasm/libbb9/fastled/noise.o .pio/build/wasm/libbb9/fastled/platforms.o .pio/build/wasm/libbb9/fastled/platforms/arm/k20/clockless_objectfled.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/clockless_i2s_esp32s3.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/i2s/i2s_esp32dev.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/rmt_4/idf4_rmt.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/rmt_4/idf4_rmt_impl.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/rmt_5/idf5_rmt.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/rmt_5/strip_rmt.o .pio/build/wasm/libbb9/fastled/platforms/esp/32/spi_ws2812/strip_spi.o .pio/build/wasm/libbb9/fastled/platforms/wasm/active_strip_data.o .pio/build/wasm/libbb9/fastled/platforms/wasm/compiler/Arduino.o .pio/build/wasm/libbb9/fastled/platforms/wasm/engine_listener.o .pio/build/wasm/libbb9/fastled/platforms/wasm/fs_wasm.o .pio/build/wasm/libbb9/fastled/platforms/wasm/js.o .pio/build/wasm/libbb9/fastled/platforms/wasm/timer.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/audio.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/button.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/checkbox.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/description.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/number_field.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/slider.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/title.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/ui_internal.o .pio/build/wasm/libbb9/fastled/platforms/wasm/ui/ui_manager.o .pio/build/wasm/libbb9/fastled/power_mgt.o .pio/build/wasm/libbb9/fastled/rgbw.o .pio/build/wasm/libbb9/fastled/sensors/digital_pin.o .pio/build/wasm/libbb9/fastled/sensors/pir.o .pio/build/wasm/libbb9/fastled/simplex.o .pio/build/wasm/libbb9/fastled/third_party/cq_kernel/cq_kernel.o .pio/build/wasm/libbb9/fastled/third_party/cq_kernel/kiss_fft.o .pio/build/wasm/libbb9/fastled/third_party/cq_kernel/kiss_fftr.o .pio/build/wasm/libbb9/fastled/third_party/espressif/led_strip/src/led_strip_api.o .pio/build/wasm/libbb9/fastled/third_party/espressif/led_strip/src/led_strip_rmt_dev.o .pio/build/wasm/libbb9/fastled/third_party/espressif/led_strip/src/led_strip_rmt_encoder.o .pio/build/wasm/libbb9/fastled/third_party/espressif/led_strip/src/led_strip_spi_dev.o .pio/build/wasm/libbb9/fastled/third_party/object_fled/src/OjectFLED.o .pio/build/wasm/libbb9/fastled/transpose8x1_noinline.o .pio/build/wasm/libbb9/fastled/wiring.o .pio/build/wasm/src/wasm.ino.o -L.pio/build/wasm
3.61 ========================= [SUCCESS] Took 3.20 seconds =========================
```
