"""Offline release-artifact checks before any tag or registry upload.

This is a preflight, not a PyPI upload reservation or a project-quota check.
"""

import argparse
import json
import zipfile
from pathlib import Path

TARGET_PLATFORMS = {
    "x86_64-unknown-linux-gnu": "manylinux_2_28_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_2_28_aarch64",
    "x86_64-pc-windows-msvc": "win_amd64",
    "aarch64-pc-windows-msvc": "win_arm64",
    "x86_64-apple-darwin": "macosx_10_12_x86_64",
    "aarch64-apple-darwin": "macosx_11_0_arm64",
}
MAX_WHEEL_BYTES = 100_000_000  # Conservative PyPI per-file limit.


def check(wheel_root: Path, binary_root: Path, version: str) -> dict:
    if not version or not all(char.isdigit() or char == "." for char in version):
        raise ValueError(f"invalid release version: {version!r}")
    wheel_files = sorted(path for path in wheel_root.rglob("*") if path.is_file())
    expected_wheels = {
        f"fastled-{version}-py3-none-{platform}.whl": target
        for target, platform in TARGET_PLATFORMS.items()
    }
    if {path.name for path in wheel_files} != set(expected_wheels) or len(
        wheel_files
    ) != len(expected_wheels):
        raise ValueError(
            f"expected exactly the six versioned platform wheels {sorted(expected_wheels)}; found {[path.name for path in wheel_files]}"
        )
    wheels = []
    for path in wheel_files:
        size = path.stat().st_size
        if not 0 < size < MAX_WHEEL_BYTES:
            raise ValueError(
                f"wheel is empty or at/above the {MAX_WHEEL_BYTES}-byte limit: {path} ({size} bytes)"
            )
        with zipfile.ZipFile(path) as archive:
            bad_member = archive.testzip()
            binary_name = (
                "fastled.exe" if "windows" in expected_wheels[path.name] else "fastled"
            )
            binary_path = f"fastled/bin/{binary_name}"
            if (
                bad_member
                or not any(
                    name.endswith(".dist-info/WHEEL") for name in archive.namelist()
                )
                or binary_path not in archive.namelist()
                or not archive.getinfo(binary_path).file_size
            ):
                raise ValueError(
                    f"invalid wheel archive: {path} (bad member: {bad_member})"
                )
        wheels.append({"name": path.name, "bytes": size})

    binary_dirs = (
        sorted(path for path in binary_root.iterdir() if path.is_dir())
        if binary_root.exists()
        else []
    )
    if len(binary_dirs) != len(TARGET_PLATFORMS):
        raise ValueError(
            f"expected {len(TARGET_PLATFORMS)} binary artifacts; found {[path.name for path in binary_dirs]}"
        )
    for target in TARGET_PLATFORMS:
        matches = [
            path
            for path in binary_dirs
            if path.name.startswith("binary-") and path.name.endswith("-" + target)
        ]
        if len(matches) != 1:
            raise ValueError(f"missing or duplicate binary artifact for {target}")
        executable = matches[0] / ("fastled.exe" if "windows" in target else "fastled")
        if not executable.is_file() or executable.stat().st_size == 0:
            raise ValueError(f"missing or empty release binary: {executable}")
    return {
        "schema_version": 1,
        "version": version,
        "wheels": wheels,
        "binary_targets": list(TARGET_PLATFORMS),
        "pypi_project_quota_verified": False,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheels", type=Path, required=True)
    parser.add_argument("--binaries", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.wheels, args.binaries, args.version), indent=2))


if __name__ == "__main__":
    main()
