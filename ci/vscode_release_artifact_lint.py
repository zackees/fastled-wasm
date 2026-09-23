"""Validate the complete VS Code release payload before creating its tag."""

import argparse
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


def check(artifacts: Path, version: str) -> list[Path]:
    expected = {f"fastled-wasm-{version}-{target}.vsix" for target in TARGETS}
    found = {path.name for path in artifacts.iterdir() if path.is_file()}
    if found != expected:
        raise ValueError(
            f"expected exact seven versioned VSIX files: {sorted(expected)}; got {sorted(found)}"
        )
    packages = sorted(artifacts / name for name in expected)
    for package in packages:
        if package.stat().st_size == 0:
            raise ValueError(f"empty VSIX: {package.name}")
        try:
            with zipfile.ZipFile(package) as archive:
                if archive.testzip() is not None:
                    raise ValueError(f"corrupt VSIX: {package.name}")
                manifest = json.loads(archive.read("extension/package.json"))
        except (zipfile.BadZipFile, KeyError, json.JSONDecodeError) as error:
            raise ValueError(f"invalid VSIX: {package.name}: {error}") from error
        if manifest.get("name") != "fastled-wasm" or manifest.get("version") != version:
            raise ValueError(f"VSIX manifest mismatch: {package.name}")
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
