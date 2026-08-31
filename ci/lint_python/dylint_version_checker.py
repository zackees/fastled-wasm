"""Require an exact cargo-dylint version before it is used."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


def check_version(tool: str, expected_version: str) -> int:
    """Return success only when ``tool`` reports the exact expected version."""

    tool_name = Path(tool).name
    expected = f"{tool_name} {expected_version}"
    try:
        result = subprocess.run(
            [tool, "dylint", "--version"],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        print(f"could not query {tool_name}: {error}", file=sys.stderr)
        return 1

    actual = result.stdout.strip()
    if result.returncode != 0:
        detail = result.stderr.strip() or actual or f"exit status {result.returncode}"
        print(f"could not query {tool_name}: {detail}", file=sys.stderr)
        return 1
    if actual != expected:
        print(
            f"expected {expected}, found {actual or '<empty output>'}", file=sys.stderr
        )
        return 1

    print(actual)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("tool")
    parser.add_argument("expected_version")
    args = parser.parse_args()

    return check_version(args.tool, args.expected_version)


if __name__ == "__main__":
    raise SystemExit(main())
