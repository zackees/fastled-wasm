# Claude Code hooks

Python scripts invoked by Claude Code hooks (configured in `.claude/settings.json`).

All hooks run via `uv run python ci/hooks/<name>.py` so they share the project
Python environment.

## Hooks

- `tool_guard.py` — **PreToolUse / Bash**. Blocks bare `cargo`, `rustc`,
  `rustfmt`, `clippy-driver`, the `cargo-clippy` / `cargo-fmt` aliases, and the
  legacy `./_cargo`, `./_rustc`, `./_rustfmt` trampolines (retired in #76). All
  Rust tooling must go through `soldr` so zccache is consulted; otherwise cold
  builds take 8–10 minutes (see #75).

  Also rejects `uv run cargo ...` and friends because `uv run <rust-tool>`
  bypasses soldr's toolchain selection.

  `soldr <subcommand>` invocations and `uv pip ...` pass through.
