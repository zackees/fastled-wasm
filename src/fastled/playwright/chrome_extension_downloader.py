"""Chrome Web Store extension downloader (thin Python shim).

The actual CRX download + decode lives in `crates/fastled-cli/src/install.rs`
(`ensure_chrome_extension`) so the Python side no longer imports httpx.
This module just invokes the Rust binary via a hidden internal flag and
returns the local install path.
"""

import subprocess
import sys
import warnings
from pathlib import Path

from fastled.interrupts import handle_keyboard_interrupt

CPP_DEVTOOLS_EXTENSION_ID = "pdcpmagijalfljmkmjngeonclgbbannb"
CPP_DEVTOOLS_EXTENSION_NAME = "cpp-devtools-support"


def _find_rust_fastled_cli() -> Path | None:
    """Locate the Rust fastled binary, preferring target/ over the venv shim."""
    import shutil

    exe_name = "fastled.exe" if sys.platform == "win32" else "fastled"

    current = Path(__file__).resolve().parent
    for _ in range(10):
        if (current / "Cargo.toml").is_file():
            for profile in ("release", "debug"):
                candidate = current / "target" / profile / exe_name
                if candidate.is_file():
                    return candidate
            target_dir = current / "target"
            if target_dir.is_dir():
                for arch_dir in target_dir.iterdir():
                    if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                        for profile in ("release", "debug"):
                            candidate = arch_dir / profile / exe_name
                            if candidate.is_file():
                                return candidate
            break
        parent = current.parent
        if parent == current:
            break
        current = parent

    found = shutil.which(exe_name)
    if found:
        return Path(found)
    return None


def download_cpp_devtools_extension() -> Path | None:
    """Ensure the C++ DevTools (DWARF) Chrome extension is installed locally.

    Returns the install path, or ``None`` if the Rust binary is missing or
    the download failed (a warning is emitted for caller-visibility).
    """
    cli = _find_rust_fastled_cli()
    if cli is None:
        warnings.warn(
            "Could not locate the Rust fastled CLI; "
            "Chrome extension cannot be installed."
        )
        return None

    cmd = [
        str(cli),
        "--internal-ensure-chrome-extension",
        CPP_DEVTOOLS_EXTENSION_ID,
        CPP_DEVTOOLS_EXTENSION_NAME,
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    except KeyboardInterrupt as ki:
        handle_keyboard_interrupt(ki)
        raise
    except Exception as e:
        warnings.warn(f"Failed to invoke fastled CLI for Chrome extension: {e}")
        return None

    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        warnings.warn(
            "fastled --internal-ensure-chrome-extension failed "
            f"(exit {result.returncode})"
        )
        return None

    path_str = result.stdout.strip().splitlines()[-1] if result.stdout else ""
    path = Path(path_str)
    if not path.is_dir():
        warnings.warn(f"Rust binary returned non-existent extension path: {path_str!r}")
        return None
    return path


if __name__ == "__main__":
    extension_path = download_cpp_devtools_extension()
    if extension_path:
        print(f"Extension downloaded to: {extension_path}")
    else:
        print("Failed to download extension")
