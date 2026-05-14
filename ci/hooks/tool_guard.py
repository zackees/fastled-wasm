#!/usr/bin/env python3
"""PreToolUse Bash hook: enforce soldr for Rust tooling.

Blocks invocations that bypass soldr/zccache:

* Bare ``cargo``, ``rustc``, ``rustfmt``, ``clippy-driver``, ``rustup`` (and their
  ``cargo-`` variants). These must run through ``soldr cargo ...`` so the
  zccache compile cache is consulted; otherwise cold builds take 8-10
  minutes (see issue #75).
* Legacy ``./_cargo``, ``./_rustc``, ``./_rustfmt`` trampolines. Removed
  by issue #76; suggests the soldr equivalent.
* ``uv run <rust-tool>``. Bypasses soldr's toolchain selection.

Exit code 0 always; denial signalled via JSON payload on stdout so the
agent harness reports the reason inline.
"""

from __future__ import annotations

import json
import re
import shlex
import sys

RUST_TOOLS = {
    "cargo",
    "rustc",
    "rustfmt",
    "rustup",
    "clippy-driver",
    "cargo-clippy",
    "cargo-fmt",
}
LEGACY_RUST_TRAMPOLINES = {"_cargo", "_rustc", "_rustfmt"}
SHELL_TOOL_NAMES = {"Bash", "Shell", "shell_command", "functions.shell_command"}
NESTED_SHELLS = {"bash", "sh", "zsh", "pwsh", "powershell", "cmd"}


def _is_env_assignment(word: str) -> bool:
    return re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", word) is not None


def _strip_env_prefix(words: list[str]) -> list[str]:
    if words and words[0] == "env":
        words = words[1:]
    while words and _is_env_assignment(words[0]):
        words = words[1:]
    return words


def _command_words(seg: str) -> list[str]:
    return _strip_env_prefix(seg.split())


def _shell_words(seg: str) -> list[str]:
    try:
        return shlex.split(seg)
    except ValueError:
        return []


def _tool_basename(word: str) -> str:
    bare = word.replace("\\", "/").rsplit("/", 1)[-1]
    if bare.lower().endswith(".exe"):
        bare = bare[:-4]
    return bare


def _resolve_uv_run_tool(seg: str) -> str | None:
    m = re.match(r"uv\s+run\s+--script\s+(\S+)", seg)
    if m:
        return m.group(1)
    m = re.match(r"uv\s+run\s+(\S+)", seg)
    return m.group(1) if m else None


def _strip_noncode(command: str) -> str:
    """Remove heredocs, quoted strings, and command substitutions.

    These regions contain prose that may include words like 'cargo build'
    in PR bodies or documentation, but they are not invoked as commands.
    """
    out = re.sub(
        r"<<-?\s*'?\"?(\w+)'?\"?[\s\S]*?(?:^|\n)\s*\1\s*(?=\n|$)",
        " ",
        command,
        flags=re.MULTILINE,
    )
    out = re.sub(r"\$\([^()]*\)", " ", out)
    out = re.sub(r"`[^`]*`", " ", out)
    out = re.sub(r"'[^']*'", " ", out)
    out = re.sub(r'"[^"]*"', " ", out)
    return out


def _nested_shell_command(words: list[str]) -> str | None:
    words = _strip_env_prefix(words)
    if not words:
        return None

    shell = _tool_basename(words[0]).lower()
    if shell not in NESTED_SHELLS:
        return None

    for index, word in enumerate(words[1:], start=1):
        lower = word.lower()
        if shell == "cmd":
            if lower in {"/c", "-c"} and index + 1 < len(words):
                return " ".join(words[index + 1 :])
            continue

        if lower in {"-c", "-command"} or (lower.startswith("-") and "c" in lower[1:]):
            if index + 1 < len(words):
                return words[index + 1]
    return None


def _check_nested_shells(command: str) -> tuple[str, str] | None:
    segments = re.split(r"&&|\|\||;|\n", command)
    for seg in segments:
        words = _shell_words(seg.strip())
        if not words:
            continue
        nested = _nested_shell_command(words)
        if nested:
            result = check_command(nested, _depth=1)
            if result:
                return result
    return None


def check_command(command: str, _depth: int = 0) -> tuple[str, str] | None:
    """Return (tool, reason) if forbidden, otherwise None."""
    if _depth == 0:
        nested_result = _check_nested_shells(command)
        if nested_result:
            return nested_result

    command = _strip_noncode(command)
    segments = re.split(r"&&|\|\||;|\n", command)

    for seg in segments:
        seg = seg.strip()
        if not seg:
            continue

        words = _command_words(seg)
        if not words:
            continue

        first = words[0]
        bare = _tool_basename(first)
        normalized = " ".join(words)

        if bare in LEGACY_RUST_TRAMPOLINES:
            replacement = bare[1:]
            return (
                bare,
                f"`./{bare}` was retired (see issue #76). "
                f"Use `soldr {replacement} ...` instead.",
            )

        if normalized.startswith("soldr "):
            continue

        if normalized.startswith("uv run ") or normalized.startswith("uv  run "):
            tool = _resolve_uv_run_tool(normalized)
            if tool is None:
                continue
            tool_bare = _tool_basename(tool)
            if tool_bare in LEGACY_RUST_TRAMPOLINES:
                replacement = tool_bare[1:]
                return (
                    tool_bare,
                    f"`{tool}` was retired (see issue #76). "
                    f"Use `soldr {replacement} ...` instead.",
                )
            if tool_bare in RUST_TOOLS:
                replacement = (
                    "soldr cargo" if tool_bare == "rustup" else f"soldr {tool_bare}"
                )
                return (
                    tool,
                    f"Use `{replacement} ...` instead of `uv run {tool_bare} ...`. "
                    "`uv run <rust-tool>` bypasses soldr's toolchain "
                    "selection and the zccache compile cache.",
                )
            continue

        if normalized.startswith("uv pip "):
            continue

        if bare in RUST_TOOLS:
            replacement = "soldr cargo" if bare == "rustup" else f"soldr {bare}"
            return (
                bare,
                f"Use `{replacement} ...` instead of bare `{bare}`. "
                "Plain Rust tool invocations bypass soldr's toolchain "
                "selection and zccache integration (see issue #75).",
            )

    return None


def deny(reason: str) -> None:
    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        },
        sys.stdout,
    )


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0

    if data.get("tool_name", "") not in SHELL_TOOL_NAMES:
        return 0

    command = data.get("tool_input", {}).get("command", "")
    if not command:
        return 0

    result = check_command(command)
    if result:
        _, reason = result
        deny(reason)

    return 0


if __name__ == "__main__":
    sys.exit(main())
