// The dynamic FastLED runtime is sketch-independent. Tell Emscripten to
// fetch and load the separately linked sketch side module before startup.
// This source is embedded into fastled.js via --pre-js.
Module["dynamicLibraries"] = ["sketch.wasm"];
