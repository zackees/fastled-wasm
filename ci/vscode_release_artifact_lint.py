"""Validate the complete VS Code release payload before creating its tag."""

import argparse
import hashlib
import json
import zipfile
from pathlib import Path

TARGETS = frozenset(
    {
        "win32-x64",
        "win32-arm64",
        "linux-x64",
        "linux-arm64",
        "darwin-x64",
        "darwin-arm64",
        "universal",
    }
)
NATIVE_TARGETS = TARGETS - {"universal"}


def check(artifacts: Path, version: str) -> list[Path]:
    expected_packages = {f"fastled-wasm-{version}-{target}.vsix" for target in TARGETS}
    expected_sidecars = {
        f"{target}.{suffix}"
        for target in NATIVE_TARGETS
        for suffix in ("manifest.json", "sha256", "size")
    }
    expected = expected_packages | expected_sidecars
    found = {path.name for path in artifacts.iterdir() if path.is_file()}
    if found != expected:
        raise ValueError(
            f"expected seven VSIX packages and their native sidecars: {sorted(expected)}; got {sorted(found)}"
        )
    packages = sorted(artifacts / name for name in expected_packages)
    for package in packages:
        if package.stat().st_size == 0:
            raise ValueError(f"empty VSIX: {package.name}")
        embedded_manifest: bytes | None = None
        try:
            with zipfile.ZipFile(package) as archive:
                if archive.testzip() is not None:
                    raise ValueError(f"corrupt VSIX: {package.name}")
                manifest = json.loads(archive.read("extension/package.json"))
                target = next(
                    target
                    for target in TARGETS
                    if package.name.endswith(f"-{target}.vsix")
                )
                if target in NATIVE_TARGETS:
                    embedded_manifest = archive.read(
                        "extension/resources/clangd/manifest.json"
                    )
        except (zipfile.BadZipFile, KeyError, json.JSONDecodeError) as error:
            raise ValueError(f"invalid VSIX: {package.name}: {error}") from error
        if manifest.get("name") != "fastled-wasm" or manifest.get("version") != version:
            raise ValueError(f"VSIX manifest mismatch: {package.name}")
        if target in NATIVE_TARGETS:
            if embedded_manifest is None:
                raise ValueError(f"missing native manifest: {package.name}")
            expected_hash = (
                f"{hashlib.sha256(package.read_bytes()).hexdigest()}  {package.name}\n"
            )
            expected_size = f"{package.stat().st_size}\n"
            if (
                artifacts / f"{target}.manifest.json"
            ).read_bytes() != embedded_manifest:
                raise ValueError(f"native manifest sidecar mismatch: {package.name}")
            if (artifacts / f"{target}.sha256").read_text() != expected_hash:
                raise ValueError(f"SHA-256 sidecar mismatch: {package.name}")
            if (artifacts / f"{target}.size").read_text() != expected_size:
                raise ValueError(f"size sidecar mismatch: {package.name}")
    return packages


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    packages = check(args.artifacts, args.version)
    print(f"Validated {len(packages)} VSIX packages for version {args.version}")


if __name__ == "__main__":
    main()
