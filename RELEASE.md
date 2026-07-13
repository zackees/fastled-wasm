# How to do a release.

Go to src/fastled/__init__.py and increase the version number.

Make sure this is the ONLY change in your repo (or the release will fail
for unknown reasons) and commit and then push. Github builders will do all the rest.

If the release changes the Emscripten toolchain or WASM linker settings, verify
that the effective flags and generated JavaScript do not use JSPI (`-sJSPI`,
`-sJSPI_EXPORTS`, `WebAssembly.Suspending`, or `WebAssembly.promising`). Safari
is a required target, so run a real Safari smoke test before making that
toolchain the release default. Chrome and Node tests do not satisfy this check.



Make sure and watch the jobs to verify that it worked.
