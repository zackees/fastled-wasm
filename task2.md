Multiple Directories found, choose one:
  [1]: examples\Blink
  [2]: examples\BlinkParallel

Please specify a sketch directory
You can enter a number or type a fuzzy search: 1
Using sketch directory: examples\Blink
FastLED version: 1.4.30
Current working directory: C:\Users\niteris\dev\fastled
🎭 Playwright browsers available at: C:\Users\niteris\.fastled\playwright
Running in client/server mode.
Found local server at http://localhost:9021


######################################
# Compiling on http://localhost:9021 #
######################################


Zipping files...
Adding file: examples\Blink\Blink.ino
Web compiling on http://localhost:9021...
Step 1: Compiling libfastled...
Compiling libfastled on http://localhost:9021/compile/libfastled via IPv4
✅ libfastled compilation successful
Step 2: Compiling sketch...
Compiling sketch on http://localhost:9021/compile/wasm via IPv4. Zip size: 1066 bytes
Response status code: <Response [200 OK]>


###########################################
# Compilation success, took 12.85 seconds #
###########################################


Updating source directory from /host/fastled/src if necessary
✓ Found thin library: /build/quick/libfastled-thin.a (2993164 bytes)
Syncing FastLED source from /host/fastled/src to src
  Processing 525 files with line ending conversion...
  Summary: 525 files processed, 0 updated, 525 unchanged
  No files were updated - libfastled recompilation will be suppressed if libraries exist
Fast sync from /host/fastled/src to src complete in 0.98 seconds
No files changed and all libraries present, skipping sync and rebuild
Starting FastLED WASM compilation script...
Keep files flag: False
Using mapped directory: /tmp/tmp9a8awnr9
Normal build, so removing /js/src
Copying files from mapped directory to container...
Copying file: /tmp/tmp9a8awnr9/wasm/Blink.ino -> /js/src/Blink.ino
Copying file: /tmp/tmp9a8awnr9/wasm/build_mode.txt -> /js/src/build_mode.txt
Transforming files to cpp...
Found .ino file: /js/src/Blink.ino
Renaming /js/src/Blink.ino to Blink.ino

########################################
# Inserting headers in source files... #
########################################

Inserting header in file: /js/src/Blink.ino
Processed: /js/src/Blink.ino
Current directory: /js/src structure has [PosixPath('/js/src/Blink.ino'), PosixPath('/js/src/build_mode.txt')]

############################################################
# Transform to cpp and insert header operations completed. #
############################################################


###########################################################
# Starting compilation process with mode: BuildMode.QUICK #
#   js_dir: /js                                           #
#   profile_build: False                                  #
###########################################################

Starting compilation...
Starting compilation process...

###################################
# WASM is building in mode: QUICK #
###################################


####################################################################
# Build process profiling is disabled                              #
# use --profile to get metrics on how long the build process took. #
####################################################################


###########################################################
# Using direct emcc compilation (--no-platformio enabled) #
###########################################################

✓ PlatformIO bypassed - using direct emscripten compiler calls
✓ Build mode: QUICK
✓ Compiler root: /js
✓ Will use compile_sketch.py module for compilation
✓ Direct compilation command prepared: python -m fastled_wasm_compiler.compile_sketch --sketch /js/src --mode quick
Command: python -m fastled_wasm_compiler.compile_sketch --sketch /js/src --mode quick
Command cwd: /js

##################################################################################
# Build started with command:                                                    #
#   python -m fastled_wasm_compiler.compile_sketch --sketch /js/src --mode quick #
##################################################################################

✓ Using stdbuf to force line buffering for real-time output
0.00
🚀 Starting FastLED sketch compilation (no-platformio mode)
0.00 🔊 VERBOSE MODE: Showing detailed emcc/linker output
0.00 📁 Sketch directory: /js/src
0.00 🔧 Build mode: quick
0.00 📂 Output directory: /build/quick
🚀 Mold daemon started for faster linking
0.00 ✓ Output directory prepared: /build/quick
0.00
📋 Source file discovery:
0.00 ✓ Found 1 source file(s):
0.00   1. Blink.ino (3892 bytes)
0.00
🔧 Compilation configuration (from compilation_flags.toml):
0.00 📋 CXX_FLAGS:
0.00    1. -DFASTLED_ENGINE_EVENTS_MAX_LISTENERS=50
0.00    2. -DFASTLED_FORCE_NAMESPACE=1
0.00    3. -DFASTLED_USE_PROGMEM=0
0.00    4. -DUSE_OFFSET_CONVERTER=0
0.00    5. -DGL_ENABLE_GET_PROC_ADDRESS=0
0.00    6. -DEMSCRIPTEN_NO_THREADS
0.00    7. -D_REENTRANT=0
0.00    8. -std=gnu++17
0.00    9. -fpermissive
0.00   10. -Wno-constant-logical-operand
0.00   11. -Wnon-c-typedef-for-linkage
0.00   12. -Werror=bad-function-cast
0.00   13. -Werror=cast-function-type
0.00   14. -fno-threadsafe-statics
0.00   15. -fno-exceptions
0.00   16. -I.
0.00   17. -Isrc
0.00   18. -Isrc
0.00   19. -Isrc/platforms/wasm/compiler
0.00   20. -DSKETCH_COMPILE=1
0.00   21. -DFASTLED_WASM_USE_CCALL
0.00   22. -flto=thin
0.00   23. -O0
0.00   24. -g0
0.00   25. -fno-inline-functions
0.00   26. -fno-vectorize
0.00   27. -fno-unroll-loops
0.00   28. -fno-strict-aliasing
0.00
📋 LINK_FLAGS:
0.00    1. -fuse-ld=mold
0.00    2. -sWASM=1
0.00    3. -sUSE_PTHREADS=0
0.00    4. --no-entry
0.00    5. --emit-symbol-map
0.00    6. -sMODULARIZE=1
0.00    7. -sEXPORT_NAME=fastled
0.00    8. -sALLOW_MEMORY_GROWTH=1
0.00    9. -sINITIAL_MEMORY=134217728
0.00   10. -sAUTO_NATIVE_LIBRARIES=0
0.00   11. -sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','stringToUTF8','lengthBytesUTF8','HEAPU8','getValue']
0.00   12. -sEXPORTED_FUNCTIONS=['_malloc','_free','_extern_setup','_extern_loop','_fastled_declare_files','_getStripPixelData']
0.00   13. -sEXIT_RUNTIME=0
0.00   14. -sFILESYSTEM=0
0.00   15. -Wl,--gc-sections
0.00   16. --source-map-base=http://localhost:8000/
0.00
📋 Sources: /js/src/Blink.ino
0.00 📋 Sketch directory: /js/src
0.00 NO_THIN_LTO=0: Using thin archive
0.00
📚 FastLED library: /build/quick/libfastled-thin.a
0.00 ✓ FastLED library found (2993164 bytes, thin archive)
0.00
🔨 Compiling 1 source files in parallel:
0.00 ================================================================================
0.00 🔧 Using 1 worker threads for parallel compilation
0.56 ✅ COMPILED [1/1]: Blink.ino → Blink.ino.o (success) in 0.55 seconds
0.56 🔨 Build command:
0.56   /build_tools/ccache-emcxx.sh -c -x c++ -o /build/quick/Blink.ino.o -DFASTLED_ENGINE_EVENTS_MAX_LISTENERS=50 -DFASTLED_FORCE_NAMESPACE=1 -DFASTLED_USE_PROGMEM=0 -DUSE_OFFSET_CONVERTER=0 -DGL_ENABLE_GET_PROC_ADDRESS=0 -DEMSCRIPTEN_NO_THREADS -D_REENTRANT=0 -std=gnu++17 -fpermissive -Wno-constant-logical-operand -Wnon-c-typedef-for-linkage -Werror=bad-function-cast -Werror=cast-function-type -fno-threadsafe-statics -fno-exceptions -I. -Isrc -Isrc -Isrc/platforms/wasm/compiler -DSKETCH_COMPILE=1 -DFASTLED_WASM_USE_CCALL -flto=thin -O0 -g0 -fno-inline-functions -fno-vectorize -fno-unroll-loops -fno-strict-aliasing -include /build/quick/fastled_pch.h /js/src/Blink.ino
0.56 🔧 Mode-specific flags: -flto=thin -O0 -g0 -fno-inline-functions -fno-vectorize -fno-unroll-loops -fno-strict-aliasing
0.56 🚀 PCH OPTIMIZATION APPLIED: Using precompiled header fastled_pch.h
0.56     ✂️ Removed: FastLED.h/Arduino.h includes from source files
0.56          [1] Blink.ino
0.56   ✓ [1/1] Blink.ino → Blink.ino.o (102964 bytes)
0.56 --------------------------------------------------------------------------------
0.56 ✅ All 1 source files compiled successfully
0.56
🔗 Linking phase - Creating final WASM output:
0.56 ================================================================================
0.56 ✓ Linking 1 object file(s) into final output
0.56 ✓ Total object file size: 102964 bytes
0.56 ⚡ Linking with quick FastLED library: /build/quick/libfastled-thin.a (thin)
1.73 📤 Linker output:
1.73     warning: no output file specified, not emitting output
1.76 ✅ LINKED: fastled.js (success) in 1.20 seconds
1.76 🔗 Build command:
1.76   /build_tools/ccache-emcxx.sh -fuse-ld=mold -sWASM=1 -sUSE_PTHREADS=0 --no-entry --emit-symbol-map -sMODULARIZE=1 -sEXPORT_NAME=fastled -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=134217728 
-sAUTO_NATIVE_LIBRARIES=0 -sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','stringToUTF8','lengthBytesUTF8','HEAPU8','getValue'] -sEXPORTED_FUNCTIONS=['_malloc','_free','_extern_setup','_extern_loop','_fastled_declare_files','_getStripPixelData'] -sEXIT_RUNTIME=0 -sFILESYSTEM=0 -Wl,--gc-sections --source-map-base=http://localhost:8000/ -o /build/quick/fastled.js /build/quick/Blink.ino.o /build/quick/libfastled-thin.a
1.76 ================================================================================
1.76 ✅ JavaScript output: /build/quick/fastled.js (68.3k)
1.76 ✅ WebAssembly output: /build/quick/fastled.wasm (209.9k)
1.76
✅ Program built at: /build/quick/fastled.js
1.76 🔊 VERBOSE BUILD COMPLETED: All emcc/linker calls shown above

##################################
# Compilation process Finsished. #
##################################


Compilation successful.

Compilation return code: 0

###########################
# Compilation successful. #
###########################


###########################################
# No-PlatformIO build directory structure #
###########################################

✓ Using direct compilation build directory: /build/quick
✓ Build mode subdirectory: quick
✓ Expected output files: fastled.js, fastled.wasm
✓ Build directory exists: /build/quick

###########################
# Copying output files... #
###########################

Copying /build/quick/fastled.js.symbols to /tmp/tmp9a8awnr9/wasm/fastled_js/fastled.js.symbols
Copying /build/quick/fastled.wasm to /tmp/tmp9a8awnr9/wasm/fastled_js/fastled.wasm
Copying /build/quick/fastled.js to /tmp/tmp9a8awnr9/wasm/fastled_js/fastled.js
Copying src/platforms/wasm/compiler/index.html to output directory
Copying src/platforms/wasm/compiler/index.css to output directory
Copying files from src/platforms/wasm/compiler/modules to /tmp/tmp9a8awnr9/wasm/fastled_js/modules
Copying index.js to output directory

###############################
# Writing manifest files.json #
###############################


###############################
# No-PlatformIO Build Summary #
###############################

✅ Compilation method: Direct emcc calls (bypassed PlatformIO)
✅ Build mode: QUICK
✅ Build directory: /build/quick
✅ Source directory: /tmp/tmp9a8awnr9/wasm
✅ Output directory: fastled_js
📁 Checking output files in /tmp/tmp9a8awnr9/wasm/fastled_js:
  ✅ fastled.js (68283 bytes)
  ✅ fastled.wasm (209854 bytes)
🎯 Build completed using direct emscripten compilation

################################
# Cleaning up directories:     #
#   build (/js/.pio/build) and #
#   sketch (/js/src)           #
################################


##############################################
# Compilation process completed successfully #
##############################################


Web compilation successful
  Time: 12.85
  output: examples\Blink\fastled_js
  hash: 085a19b9ecb22f5aafdabecfc5b98785
  zip size: 172533 bytes
Found free port: 8170
[I 250718 01:20:00 server:331] Serving on http://127.0.0.1:8170
[I 250718 01:20:00 handlers:62] Start watching changes
[I 250718 01:20:00 handlers:64] Start detecting changes
Opening browser to http://localhost:8170
Will compile on sketch changes or if you hit the space bar.
Press Ctrl+C to stop...
[I 250718 01:20:02 handlers:135] Browser Connected: http://localhost:8170/

🔄 Newer version of niteris/fastled-wasm:latest is available (published on 2025-07-18).
Run with `fastled -u` to update the docker image to the latest version.
Or use `--background-update` to update automatically in the background after compilation.
[W 250718 01:20:04 wsgi:265] 404 GET /favicon.ico (127.0.0.1) 2.00ms


