# Agent Instructions

This file is read by Claude Code, Codex, and other coding agents that work on this repo. Codex agents should also see `CODEX.md` — that file is a single-line pointer back here so both agents read the same playbook.

## Layout

- Python package: `src/fastled/`
- Rust workspace: `crates/fastled-cli`, `crates/fastled-py`, `crates/fastled-tauri`
- Tests: `tests/unit/` (Python), `cargo test --workspace` (Rust)
- CI workflows: `.github/workflows/_*.yml`
- Dev scripts: `./install`, `./lint`, `./test`, `./build-wheel`, `./clean`

## Build cache — use soldr + zccache

This project uses **soldr** (build orchestrator) + **zccache** (compile cache). They are NOT optional for local Rust builds: a clean `cargo build -p fastled-py` recompiles PyO3 + reqwest + tokio + axum + clap from scratch and takes 8–10 minutes. With soldr/zccache the same build hits the cache and finishes in seconds.

- Binaries live at `/c/tools/python13/Scripts/{soldr,zccache}` on Windows, or wherever `uv pip install soldr` placed them.
- `soldr` wraps `cargo` and automatically uses `zccache` as `RUSTC_WRAPPER`.
- CI installs soldr via `zackees/setup-soldr@v0` (see workflows). Local dev should match.

**When you invoke any Rust tool from an agent, use `soldr cargo …`, `soldr rustfmt …`, etc.** Plain `cargo` / `rustc` / `rustfmt` are blocked by the PreToolUse hook (`ci/hooks/tool_guard.py`) — they bypass zccache and turn a 30-second incremental build into a 10-minute cold build. The legacy `./_cargo`, `./_rustc`, `./_rustfmt` trampolines were retired in #76; the hook explicitly rejects them too.

If `soldr` is not on PATH, install it with `uv tool install soldr` (or see `zackees/soldr` on GitHub).

## Tests are slow — pick a narrow set

`bash test` runs the full pytest + cargo test suite. Some unit tests (`tests/unit/test_cli.py`, `tests/unit/test_build_service.py`) perform real WASM compiles via emscripten and can take 10+ minutes each.

For iterative work, run targeted tests:

```bash
uv run pytest tests/unit/test_string_diff.py tests/unit/test_select_sketch_directory.py -v
```

Skip `test_cli.py` unless you actually need to validate end-to-end WASM output:

```bash
uv run pytest tests/unit --ignore=tests/unit/test_cli.py -v
```

**Do not pipe pytest through `| tail -N`** if you want streaming output — it buffers until pytest exits, and a multi-minute pytest looks indistinguishable from a hang. Redirect to a file or use `-v`.

## Architecture notes

- The user-facing CLI is the Rust binary `fastled` (built from `crates/fastled-cli`, bundled into the wheel via `[tool.maturin] data`). Python `cli.py` / `app.py` are 15-line shims that call `fastled._rust_cli.invoke_rust_fastled_cli`.
- `fastled._native` is the PyO3 extension compiled from `crates/fastled-py/src/lib.rs`. It exposes Rust functions to Python (sketch discovery, build service, project init, string diff, frontend bundling, …).
- The compile path goes Rust → Python toolchain → Rust: `crates/fastled-cli/src/build.rs` constructs `fastled._native.NativeBuildService` and calls it; the Python toolchain (Meson/Ninja/Emscripten internals under `src/fastled/toolchain/`) is intentionally Python-owned.
- For PyO3 changes, after `cargo build -p fastled-py` the new `.dll` lands in `target/maturin/_native.dll`. `maturin develop` copies that to `src/fastled/_native.pyd`. If `_native.pyd` is locked by other Python processes (common on Windows), use the move-aside trick: `mv src/fastled/_native.pyd src/fastled/_native.pyd.old && cp target/maturin/_native.dll src/fastled/_native.pyd`. Remember to delete the `.pyd.old` before committing.

## Conventions

- Lint command: `bash lint` — runs cargo fmt + clippy, then ruff/black/isort/pyright. Must be green before commit.
- Test command: `bash test` (or targeted subset; see above).
- Cross-platform pyright warnings about `sys.platform` are expected — don't chase them.
- Always fix all diagnostics found, even pre-existing ones not caused by current changes, before declaring a change complete.
- Prefer native (Rust) implementations and fail-loud over silent Python fallbacks. The migration tracked in #14/#71/#72/#73 is moving everything in this direction.
- Don't add backwards-compat hacks for code paths that no longer exist (e.g. unused `_var` renames, "removed" comments).

## Common gotchas

- The installed `fastled` CLI on PATH (e.g. `/c/tools/python13/Scripts/fastled.exe`) may be stale; `find_rust_fastled_cli` in `src/fastled/_rust_cli.py` prefers `target/{release,debug}/fastled.exe` over PATH for exactly this reason. If a CLI test fails, check `uv run python -c "from fastled._rust_cli import find_rust_fastled_cli; print(find_rust_fastled_cli())"`.
- Error messages from the Rust CLI go to **stderr**. Tests asserting `result.stdout` for error text should use `result.stdout + result.stderr` (the parse_args.py removal in #72 exposed this).
- `tasklist` on Windows may report tens of thousands of `python.exe` zombies — most are stale tasklist artifacts, not real processes. `ps -ef | grep python` is more reliable for live ones.

## Filing issues / PRs

- `gh issue create --repo zackees/fastled-wasm …`
- `gh pr create …`
- Reference the originating issue (`Refs #72`) in commits/PRs when applicable.
