#!/usr/bin/env python3
"""Package one platform-specific or universal VSIX without stale clangd state."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from clangd_common import load_lock

ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> None:
    if os.name == "nt" and command[0] in {"npm", "npx"}:
        command = [command[0] + ".cmd", *command[1:]]
    subprocess.run(command, cwd=ROOT, check=True)


def main() -> None:
    lock = load_lock(ROOT / "clangd-artifacts.json")
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=[*sorted(lock["targets"]), "universal"], required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--ctcb-home", type=Path, default=ROOT / ".ctcb-cache")
    args = parser.parse_args()
    payload = ROOT / "resources" / "clangd"
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    output = out / f"fastled-wasm-{version}-{args.target}.vsix"
    package_path = ROOT / "package.json"
    original_package = package_path.read_text(encoding="utf-8")
    try:
        run([sys.executable, "scripts/ingest_clangd.py", "--clean", "--output", str(payload)])
        if args.target != "universal":
            run([sys.executable, "scripts/ingest_clangd.py", "--target", args.target, "--output", str(payload), "--ctcb-home", str(args.ctcb_home)])
            run([sys.executable, "scripts/verify_clangd_runtime.py", "--root", str(payload), "--target", args.target])
        package = json.loads(original_package)
        package["fastledBundledClangd"] = {"packageKind": args.target}
        package_path.write_text(json.dumps(package, indent=2) + "\n", encoding="utf-8")
        run(["npm", "run", "compile"])
        command = ["npx", "--no-install", "vsce", "package", "--out", str(output)]
        if args.target != "universal":
            command.extend(["--target", args.target])
        run(command)
        run([sys.executable, "scripts/verify_vsix.py", "--vsix", str(output), "--target", args.target])
        run([sys.executable, "scripts/test_installed_vsix.py", "--vsix", str(output), "--expected", "universal" if args.target == "universal" else "native"])
    finally:
        # Native payloads are generated release input, never source state.
        shutil.rmtree(payload, ignore_errors=True)
        package_path.write_text(original_package, encoding="utf-8")


if __name__ == "__main__":
    main()
