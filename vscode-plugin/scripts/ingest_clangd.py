#!/usr/bin/env python3
"""Create an audited, minimal clangd bundle from the pinned CTCB distribution."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

from clangd_common import below, load_lock, payload_files, sha256, write_json

ROOT = Path(__file__).resolve().parents[1]
LOCK_PATH = ROOT / "clangd-artifacts.json"
LICENSES = (ROOT / "third_party" / "llvm" / "LICENSE.TXT", ROOT / "third_party" / "llvm" / "NOTICE.md")


def parse_installer_json(output: str) -> dict:
    lines = [line for line in output.splitlines() if line.strip()]
    if len(lines) != 1:
        raise ValueError("CTCB install must print exactly one JSON object")
    data = json.loads(lines[0])
    if not isinstance(data, dict):
        raise ValueError("CTCB output is not an object")
    return data


def install(target: dict, provider: dict, home: Path) -> Path:
    command = [
        "uvx", "--from", f"{provider['package']}=={provider['package_version']}",
        "clang-tool-chain-bins", "install", "clangd", "--component", provider["component"],
        "--platform", target["platform"], "--arch", target["arch"],
        "--version", target["llvm_version"], "--home-dir", str(home.resolve()),
    ]
    try:
        completed = subprocess.run(command, capture_output=True, text=True, timeout=20 * 60, check=False)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError("CTCB download/install timed out") from error
    if completed.returncode:
        raise RuntimeError(f"CTCB install failed ({completed.returncode}):\n{completed.stdout}\n{completed.stderr}")
    result = parse_installer_json(completed.stdout)
    expected = {
        "component": provider["component"], "platform": target["platform"], "arch": target["arch"],
        "version": target["llvm_version"], "archive_sha256": target["archive_sha256"],
    }
    for key, value in expected.items():
        if result.get(key) != value:
            raise ValueError(f"CTCB identity mismatch for {key}: {result.get(key)!r}")
    if result.get("status") not in {"installed", "already_installed"}:
        raise ValueError(f"unexpected CTCB status: {result.get('status')!r}")
    install_path = result.get("install_path")
    if not isinstance(install_path, str):
        raise ValueError("CTCB output lacks install_path")
    root = Path(install_path).resolve()
    if not root.is_dir():
        raise ValueError("CTCB install_path does not exist")
    return root


def copy_file(source_root: Path, source: Path, stage: Path) -> None:
    resolved = below(source_root, source)
    if resolved.is_symlink() or not resolved.is_file():
        raise ValueError(f"not a regular source file: {source}")
    destination = stage / resolved.relative_to(source_root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(resolved, destination)


def stage_bundle(target_name: str, target: dict, provider: dict, source_root: Path, output: Path) -> None:
    binary = below(source_root, source_root / target["binary_path"])
    if binary.is_symlink() or not binary.is_file() or binary.stat().st_size != target["binary_size"] or sha256(binary) != target["binary_sha256"]:
        raise ValueError("source clangd hash or size does not match lock")
    include = below(source_root, source_root / target["resource_include_path"])
    for sentinel in (include / "stddef.h", include / "stdint.h"):
        if sentinel.is_symlink() or not sentinel.is_file():
            raise ValueError(f"missing required builtin header: {sentinel.name}")
    if any(not item.is_file() or item.is_symlink() for item in LICENSES):
        raise ValueError("committed LLVM license files are required")
    output.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=f".{output.name}.stage-", dir=output.parent))
    try:
        copy_file(source_root, binary, stage)
        for source in include.rglob("*"):
            if source.is_dir():
                continue
            copy_file(source_root, source, stage)
        for license_path in LICENSES:
            destination = stage / "third_party" / "llvm" / license_path.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(license_path, destination)
        if os.name != "nt":
            (stage / target["binary_path"]).chmod(0o755)
        manifest = {
            "schema_version": 1, "target": target_name,
            "llvm_version": target["llvm_version"], "llvm_commit": target["llvm_commit"],
            "ctcb_version": provider["package_version"], "archive_sha256": target["archive_sha256"],
            "binary": {"path": target["binary_path"], "size": target["binary_size"], "sha256": target["binary_sha256"]},
            "resource_include_path": target["resource_include_path"], "files": payload_files(stage),
        }
        write_json(stage / "manifest.json", manifest)
        # Re-hash the staged payload before it becomes visible to a packager.
        if next(item for item in payload_files(stage) if item["path"] == target["binary_path"])["sha256"] != target["binary_sha256"]:
            raise ValueError("staged binary hash mismatch")
        old = output.with_name(output.name + ".previous")
        if old.exists():
            shutil.rmtree(old)
        if output.exists():
            output.replace(old)
        stage.replace(output)
        if old.exists():
            shutil.rmtree(old)
    except Exception:
        shutil.rmtree(stage, ignore_errors=True)
        raise


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=sorted(load_lock(LOCK_PATH)["targets"]))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ctcb-home", type=Path)
    parser.add_argument("--clean", action="store_true")
    args = parser.parse_args()
    if args.clean:
        if args.target or args.ctcb_home:
            parser.error("--clean only accepts --output")
        shutil.rmtree(args.output, ignore_errors=True)
        return
    if not args.target or not args.ctcb_home:
        parser.error("--target and --ctcb-home are required unless --clean is used")
    lock = load_lock(LOCK_PATH)
    stage_bundle(args.target, lock["targets"][args.target], lock["provider"], install(lock["targets"][args.target], lock["provider"], args.ctcb_home), args.output.resolve())


if __name__ == "__main__":
    main()
