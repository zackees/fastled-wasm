import os
import platform

FILE_CHANGED_DEBOUNCE_SECONDS = 2.0
MACHINE = platform.machine().lower()
IS_ARM: bool = "arm" in MACHINE or "aarch64" in MACHINE
PLATFORM_TAG: str = "-arm64" if IS_ARM else ""
CONTAINER_NAME = f"fastled-wasm-container{PLATFORM_TAG}"
DEFAULT_URL = str(os.environ.get("FASTLED_URL", "https://fastled.onrender.com"))
SERVER_PORT = 9021

IMAGE_NAME = "niteris/fastled-wasm"
DEFAULT_CONTAINER_NAME = "fastled-wasm-container"
# IMAGE_TAG = "latest"

DOCKER_FILE = (
    "https://raw.githubusercontent.com/zackees/fastled-wasm/refs/heads/main/Dockerfile"
)
