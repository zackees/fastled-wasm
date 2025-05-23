# FAQ

### Why is this compiler so fast?

The compiler image uses compile caching. This cache is pre-warmed by compiling the Blink sketch, and then frozen. This happens for debug/release/quick builds, which is why it’s so blazing fast. The whole compiler toolchain is cocked and ready to fire and then frozen in place right before it compiles your sketch.

The entire Fastled library pre-compiled as a static archive and headers made available to the sketch.

During compile time, the compiler only has to consider your code and linking against a static lib.

All of this together has eliminated 90% of the compile time. But this number will increase to 97% when I apply some of the more painful refactors to eliminate the emscripten steps (--bind) that happen at final program generation time.
