#!/usr/bin/env python3
"""Inspect VSIX contents without extracting or trusting its file names."""
from __future__ import annotations

import argparse
import json
import re
import zipfile
from pathlib import Path, PurePosixPath

from clangd_common import load_lock, safe_relative

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vsix", type=Path, required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    lock = load_lock(ROOT / "clangd-artifacts.json")
    if args.target != "universal" and args.target not in lock["targets"]:
        parser.error("unknown target")
    expected_suffix = f"-{args.target}.vsix"
    if not args.vsix.name.endswith(expected_suffix):
        raise ValueError("VSIX filename does not match target")
    with zipfile.ZipFile(args.vsix) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or any(not safe_relative(name) for name in names):
            raise ValueError("unsafe or duplicate VSIX path")
        package = json.loads(archive.read("extension/package.json"))
        if package.get("extensionKind") != ["workspace"]:
            raise ValueError("extension is not workspace-only")
        clangd_names = [name for name in names if name.startswith("extension/resources/clangd/")]
        if args.target == "universal":
            if clangd_names:
                raise ValueError("universal VSIX contains native clangd payload")
            return
        target = lock["targets"][args.target]
        manifest_name = "extension/resources/clangd/manifest.json"
        if clangd_names.count(manifest_name) != 1:
            raise ValueError("missing or duplicate clangd manifest")
        manifest = json.loads(archive.read(manifest_name))
        if manifest.get("target") != args.target:
            raise ValueError("VSIX manifest target mismatch")
        files = manifest.get("files")
        if not isinstance(files, list):
            raise ValueError("invalid manifest files")
        for item in files:
            path = item.get("path") if isinstance(item, dict) else None
            name = "extension/resources/clangd/" + str(path)
            if not isinstance(path, str) or not safe_relative(path) or name not in names:
                raise ValueError("manifest references missing/unsafe file")
            data = archive.read(name)
            import hashlib
            if len(data) != item.get("size") or hashlib.sha256(data).hexdigest() != item.get("sha256"):
                raise ValueError(f"VSIX payload hash mismatch: {path}")
        allowed = {"extension/resources/clangd/" + item["path"] for item in files}
        actual_files = {name for name in clangd_names if not name.endswith("/") and name != manifest_name}
        if actual_files != allowed:
            raise ValueError("VSIX native payload differs from manifest")
        bad = [name for name in actual_files if re.search(r"(?:\.dll|\.so(?:\.|$)|\.dylib)$|/lib/clang/21/lib/", name)]
        if bad:
            raise ValueError(f"disallowed native payload: {bad}")
        binary = "extension/resources/clangd/" + target["binary_path"]
        if binary not in names:
            raise ValueError("clangd binary missing from VSIX")
    print(f"verified {args.vsix}")


if __name__ == "__main__":
    main()
