#!/usr/bin/env python3
"""Validate a generated clangd bundle, and execute it only on its native host."""
from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from clangd_common import below, load_lock, native_target, payload_files, safe_relative, sha256

ROOT = Path(__file__).resolve().parents[1]


def expected_paths(lock_target: dict) -> set[str]:
    return {lock_target["binary_path"], "third_party/llvm/LICENSE.TXT", "third_party/llvm/NOTICE.md"}


def manifest(root: Path, target_name: str, target: dict) -> dict:
    file = root / "manifest.json"
    if not file.is_file():
        raise ValueError("manifest missing")
    data = json.loads(file.read_text(encoding="utf-8"))
    required = {"schema_version", "target", "llvm_version", "llvm_commit", "ctcb_version", "archive_sha256", "binary", "resource_include_path", "files"}
    if set(data) != required or data["schema_version"] != 1 or data["target"] != target_name:
        raise ValueError("manifest target/schema mismatch")
    if data["binary"] != {"path": target["binary_path"], "size": target["binary_size"], "sha256": target["binary_sha256"]}:
        raise ValueError("manifest binary mismatch")
    if data["llvm_version"] != target["llvm_version"] or data["resource_include_path"] != target["resource_include_path"]:
        raise ValueError("manifest LLVM/resource mismatch")
    actual = payload_files(root)
    if data["files"] != actual:
        raise ValueError("manifest file inventory/hash mismatch")
    include_prefix = target["resource_include_path"] + "/"
    for item in actual:
        value = item["path"]
        if value not in expected_paths(target) and not value.startswith(include_prefix):
            raise ValueError(f"disallowed payload file: {value}")
        if not safe_relative(value):
            raise ValueError(f"unsafe manifest path: {value}")
        below(root, root / value)
    for sentinel in ("stddef.h", "stdint.h"):
        if not (root / target["resource_include_path"] / sentinel).is_file():
            raise ValueError(f"builtin header missing: {sentinel}")
    return data


def architecture(binary: Path, target_name: str) -> None:
    raw = binary.read_bytes()[:4096]
    wanted = "arm64" if target_name.endswith("arm64") else "x64"
    if raw[:2] == b"MZ":
        offset = int.from_bytes(raw[0x3C:0x40], "little")
        machine = int.from_bytes(binary.read_bytes()[offset + 4:offset + 6], "little")
        if machine != ({"x64": 0x8664, "arm64": 0xAA64}[wanted]):
            raise ValueError("PE architecture mismatch")
    elif raw[:4] == b"\x7fELF":
        machine = int.from_bytes(raw[18:20], "little")
        if machine != ({"x64": 62, "arm64": 183}[wanted]):
            raise ValueError("ELF architecture mismatch")
    elif int.from_bytes(raw[:4], "big") in {0xFEEDFACF, 0xCFFAEDFE}:
        endian = "little" if raw[:4] == b"\xcf\xfa\xed\xfe" else "big"
        cpu = int.from_bytes(raw[4:8], endian, signed=True)
        if cpu != ({"x64": 0x01000007, "arm64": 0x0100000C}[wanted]):
            raise ValueError("Mach-O architecture mismatch")
    else:
        raise ValueError("unrecognized clangd binary format")


def check_dependencies(binary: Path, target_name: str, root: Path) -> None:
    if target_name.startswith("linux-"):
        dynamic = subprocess.run(["readelf", "-d", str(binary)], text=True, capture_output=True, check=True).stdout
        if re.search(r"lib(?:LLVM|clang)", dynamic):
            raise ValueError("bundled LLVM/clang dependency")
        ldd = subprocess.run(["ldd", str(binary)], text=True, capture_output=True, check=True).stdout
        if "not found" in ldd:
            raise ValueError("unresolved native dependency")
    elif target_name.startswith("darwin-"):
        output = subprocess.run(["otool", "-L", str(binary)], text=True, capture_output=True, check=True).stdout.splitlines()[1:]
        for line in output:
            dependency = line.strip().split(" ", 1)[0]
            if not dependency.startswith(("/usr/lib/", "/System/Library/")):
                raise ValueError(f"disallowed Mach-O dependency: {dependency}")
    elif target_name.startswith("win32-"):
        # dumpbin is present on hosted Windows images.  Its output is enough to
        # prove no imported DLL resolves inside our package.
        dumpbin = shutil.which("dumpbin")
        if dumpbin:
            output = subprocess.run([dumpbin, "/DEPENDENTS", str(binary)], text=True, capture_output=True, check=True).stdout.lower()
            if str(root).lower() in output:
                raise ValueError("Windows dependency resolves in bundle")


def run_native(binary: Path, target: dict, root: Path) -> None:
    clean_env = {"PATH": ""}
    if os.name == "nt":
        clean_env["SystemRoot"] = os.environ.get("SystemRoot", r"C:\\Windows")
        clean_env["WINDIR"] = os.environ.get("WINDIR", clean_env["SystemRoot"])
    try:
        version = subprocess.run([str(binary), "--version"], env=clean_env, text=True, capture_output=True, timeout=5, check=True).stdout
    except subprocess.TimeoutExpired as error:
        raise ValueError("clangd --version timed out") from error
    if target["llvm_version"] not in version:
        raise ValueError("clangd version mismatch")
    fixture = ROOT / "test" / "fixtures" / "clangd"
    with tempfile.TemporaryDirectory() as directory:
        workspace = Path(directory)
        cpp = workspace / "fixture.cpp"
        shutil.copyfile(fixture / "fixture.cpp", cpp)
        rendered = (fixture / "compile_commands.json.in").read_text(encoding="utf-8").replace("@FIXTURE_CPP@", str(cpp).replace("\\", "/"))
        (workspace / "compile_commands.json").write_text(rendered, encoding="utf-8")
        result = subprocess.run([str(binary), "--check=" + str(cpp), "--compile-commands-dir=" + str(workspace)], env=clean_env, text=True, capture_output=True, timeout=30)
        if result.returncode:
            raise ValueError(f"clangd --check failed: {result.stderr}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    lock = load_lock(ROOT / "clangd-artifacts.json")
    target = lock["targets"].get(args.target)
    if target is None:
        parser.error("unknown target")
    root = args.root.resolve()
    manifest(root, args.target, target)
    binary = below(root, root / target["binary_path"])
    architecture(binary, args.target)
    if native_target() == args.target:
        check_dependencies(binary, args.target, root)
        run_native(binary, target, root)
    print(f"verified {args.target}: {binary}")


if __name__ == "__main__":
    main()
