# Agent hooks

Python scripts invoked by Claude Code and Codex hooks. Claude Code loads the
hook config from `.claude/settings.json`; Codex loads the migrated config from
`.codex/hooks.json`.

Hooks run through `ci/hooks/tool_guard`, not `uv run python`. The wrapper uses
`python` on Windows, including Git Bash/MSYS/Cygwin, and `python3` on
macOS/Linux. They are intentionally stdlib-only so PreToolUse checks do not
trigger a uv sync or a native-extension rebuild before every tool call.

## Hooks

- `tool_guard.py` - **PreToolUse / Bash**. Blocks bare `cargo`, `rustc`,
  `rustfmt`, `rustup`, `clippy-driver`, the `cargo-clippy` / `cargo-fmt` aliases, and the
  legacy `./_cargo`, `./_rustc`, `./_rustfmt` trampolines (retired in #76). All
  Rust tooling must go through `soldr` so zccache is consulted; otherwise cold
  builds take 8-10 minutes (see #75).

  Also rejects `uv run cargo ...` and friends because `uv run <rust-tool>`
  bypasses soldr's toolchain selection.

  `soldr <subcommand>` invocations and `uv pip ...` pass through.
