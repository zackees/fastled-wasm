"""Shared, deliberately strict helpers for the bundled clangd build scripts."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
from typing import Any

TARGETS = {"win32-x64", "win32-arm64", "linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"}
LOCK_TOP_KEYS = {"schema_version", "provider", "targets"}
PROVIDER_KEYS = {"package", "package_version", "component"}
TARGET_KEYS = {
    "platform", "arch", "llvm_version", "llvm_commit", "provenance_method",
    "archive_filename", "archive_sha256", "binary_path", "binary_size",
    "binary_sha256", "resource_include_path",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative(value: str) -> bool:
    path = PurePosixPath(value)
    return bool(value) and not path.is_absolute() and ".." not in path.parts and "\\" not in value


def below(root: Path, candidate: Path) -> Path:
    root = root.resolve()
    resolved = candidate.resolve()
    try:
        resolved.relative_to(root)
    except ValueError as error:
        raise ValueError(f"path escapes root: {candidate}") from error
    return resolved


def load_lock(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if set(data) != LOCK_TOP_KEYS or data["schema_version"] != 1:
        raise ValueError("invalid lock top-level schema")
    provider = data["provider"]
    if not isinstance(provider, dict) or set(provider) != PROVIDER_KEYS:
        raise ValueError("invalid lock provider schema")
    if provider != {"package": "clang-tool-chain-bins", "package_version": "0.4.6", "component": "clang-extra"}:
        raise ValueError("lock must use pinned clang-tool-chain-bins 0.4.6 clang-extra")
    targets = data["targets"]
    if not isinstance(targets, dict) or set(targets) != TARGETS:
        raise ValueError("lock must contain exactly the six VS Code targets")
    identities: set[tuple[str, str]] = set()
    for target_name, target in targets.items():
        if not isinstance(target, dict) or set(target) != TARGET_KEYS:
            raise ValueError(f"invalid lock schema for {target_name}")
        if target["platform"] not in {"win", "linux", "darwin"} or target["arch"] not in {"x86_64", "arm64"}:
            raise ValueError(f"unsupported platform/arch for {target_name}")
        if (target["platform"], target["arch"]) in identities:
            raise ValueError("duplicate platform/arch")
        identities.add((target["platform"], target["arch"]))
        if not isinstance(target["binary_size"], int) or target["binary_size"] <= 0:
            raise ValueError(f"invalid binary size for {target_name}")
        for key in ("archive_sha256", "binary_sha256"):
            value = target[key]
            if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
                raise ValueError(f"invalid {key} for {target_name}")
        for key in ("archive_filename", "binary_path", "resource_include_path"):
            if not isinstance(target[key], str) or not safe_relative(target[key]):
                raise ValueError(f"unsafe {key} for {target_name}")
    return data


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def payload_files(root: Path) -> list[dict[str, Any]]:
    files: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"payload contains a symlink: {path}")
        if path.is_file() and path.name != "manifest.json":
            rel = path.relative_to(root).as_posix()
            files.append({"path": rel, "size": path.stat().st_size, "sha256": sha256(path)})
    return files


def native_target() -> str | None:
    platform = {"win32": "win32", "linux": "linux", "darwin": "darwin"}.get(os.sys.platform)
    arch = {"AMD64": "x64", "x86_64": "x64", "aarch64": "arm64", "ARM64": "arm64"}.get(os.uname().machine if hasattr(os, "uname") else os.environ.get("PROCESSOR_ARCHITECTURE", ""))
    if platform and arch:
        candidate = f"{platform}-{arch}"
        return candidate if candidate in TARGETS else None
    return None
