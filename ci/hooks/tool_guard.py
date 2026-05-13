#!/usr/bin/env python3
"""PreToolUse Bash hook: enforce soldr for Rust tooling.

Blocks invocations that bypass soldr/zccache:

* Bare ``cargo``, ``rustc``, ``rustfmt``, ``clippy-driver`` (and their
  ``cargo-`` variants). These must run through ``soldr cargo …`` so the
  zccache compile cache is consulted; otherwise cold builds take 8-10
  minutes (see issue #75).
* Legacy ``./_cargo``, ``./_rustc``, ``./_rustfmt`` trampolines. Removed
  by issue #76; suggests the soldr equivalent.
* ``uv run <rust-tool>``. Bypasses soldr's toolchain selection.

Exit code 0 always; denial signalled via JSON payload on stdout so the
Claude Code harness reports the reason inline.
"""

from __future__ import annotations

import json
import re
import sys

RUST_TOOLS = {
    "cargo",
    "rustc",
    "rustfmt",
    "clippy-driver",
    "cargo-clippy",
    "cargo-fmt",
}
LEGACY_RUST_TRAMPOLINES = {"_cargo", "_rustc", "_rustfmt"}


def _is_env_assignment(word: str) -> bool:
    return re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", word) is not None


def _command_words(seg: str) -> list[str]:
    words = seg.split()
    if words and words[0] == "env":
        words = words[1:]
    while words and _is_env_assignment(words[0]):
        words = words[1:]
    return words


def _resolve_uv_run_tool(seg: str) -> str | None:
    m = re.match(r"uv\s+run\s+--script\s+(\S+)", seg)
    if m:
        return m.group(1)
    m = re.match(r"uv\s+run\s+(\S+)", seg)
    return m.group(1) if m else None


def check_command(command: str) -> tuple[str, str] | None:
    """Return (tool, reason) if forbidden, otherwise None."""
    segments = re.split(r"&&|\|\||;", command)

    for seg in segments:
        seg = seg.strip()
        if not seg:
            continue

        words = _command_words(seg)
        if not words:
            continue

        first = words[0]
        bare = first.lstrip("./\\")
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
            tool_bare = tool.lstrip("./\\")
            if tool_bare in LEGACY_RUST_TRAMPOLINES:
                replacement = tool_bare[1:]
                return (
                    tool_bare,
                    f"`{tool}` was retired (see issue #76). "
                    f"Use `soldr {replacement} ...` instead.",
                )
            if tool in RUST_TOOLS:
                return (
                    tool,
                    f"Use `soldr {tool} ...` instead of `uv run {tool} ...`. "
                    "`uv run <rust-tool>` bypasses soldr's toolchain "
                    "selection and the zccache compile cache.",
                )
            continue

        if normalized.startswith("uv pip "):
            continue

        if first in RUST_TOOLS:
            return (
                first,
                f"Use `soldr {first} ...` instead of bare `{first}`. "
                "Plain cargo invocations bypass zccache and turn a 30-second "
                "incremental build into a 10-minute cold build (see issue #75).",
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

    if data.get("tool_name", "") != "Bash":
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
