# Agent Instructions

This file is read by Claude Code, Codex, and other coding agents that work on this repo. Codex agents should also see `CODEX.md`, which points back here.

## Layout

- Python package shim: `src/fastled/`
- Rust workspace: `crates/fastled-cli`
- Tests: `tests/unit/` (thin Python API smoke tests), `soldr cargo test --workspace` (Rust)
- CI workflows: `.github/workflows/_*.yml`
- Dev scripts: `./install`, `./lint`, `./test`, `./build-wheel`, `./clean`

## Build Cache

Use `soldr cargo ...`, `soldr rustfmt ...`, and related soldr-wrapped commands for Rust work. Plain `cargo`, `rustc`, `rustfmt`, and `rustup` bypass soldr and are blocked by `ci/hooks/tool_guard.py`.

If `soldr` is not on PATH, install it with `uv tool install soldr`.

## Tests

`bash test` runs the Python API smoke tests plus the Rust workspace tests. End-to-end WASM compiles should be run only when the change touches the native build backend.

For iterative work:

```bash
uv run pytest tests/unit/test_python_api.py -v
soldr cargo test --workspace
```

Do not pipe pytest through `| tail -N` if you want streaming output; it buffers until pytest exits.

## Architecture Notes

- The user-facing CLI is the Rust binary `fastled` built from `crates/fastled-cli`.
- Python `cli.py` and `app.py` are tiny shims that call `fastled._rust_cli.invoke_rust_fastled_cli`.
- The Python package intentionally does not ship a PyO3 extension; it bundles and launches the native Rust CLI.
- The compile path is Rust-owned through `crates/fastled-cli/src/build.rs` and `crates/fastled-cli/src/wasm_build.rs`.
- Python no longer owns build, toolchain, sketch selection, project init, install, server, debug-symbol, or frontend-bundling orchestration.

## Conventions

- Lint command: `bash lint`.
- Test command: `bash test` or targeted subsets above.
- Prefer native Rust implementations and fail-loud behavior over silent Python fallbacks.
- Do not add backwards-compat hacks for code paths that no longer exist.

## Common Gotchas

- The installed `fastled` CLI on PATH may be a Python compatibility shim or stale script; the bundled native binary is `fastled[.exe]`.
- Error messages from the Rust CLI go to stderr.
- On Windows, stale generated binaries under `src/fastled/bin/` are build artifacts and should not be committed.

## Filing Issues / PRs

- `gh issue create --repo zackees/fastled-wasm ...`
- `gh pr create ...`
- Reference the originating issue (`Refs #72`) in commits/PRs when applicable.
