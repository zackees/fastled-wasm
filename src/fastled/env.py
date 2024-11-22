import os
import platform

MACHINE = platform.machine().lower()
IS_ARM: bool = "arm" in MACHINE or "aarch64" in MACHINE
PLATFORM_TAG: str = "-arm64" if IS_ARM else ""
CONTAINER_NAME = f"fastled-wasm-compiler{PLATFORM_TAG}"
DEFAULT_URL = str(os.environ.get("FASTLED_URL", "https://fastled.onrender.com"))
