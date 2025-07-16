import os
import platform
import sys

FILE_CHANGED_DEBOUNCE_SECONDS = 2.0
MACHINE = platform.machine().lower()
IS_ARM: bool = "arm" in MACHINE or "aarch64" in MACHINE
PLATFORM_TAG: str = "-arm64" if IS_ARM else ""


def _is_running_under_github_actions() -> bool:
    """Detect if we're running under github actions."""
    return "GITHUB_ACTIONS" in os.environ


def _is_running_under_pytest() -> bool:
    """Detect if we're running under pytest."""
    # Check if pytest is in the loaded modules
    if "pytest" in sys.modules:
        return True

    # Check for pytest environment variables
    if "PYTEST_CURRENT_TEST" in os.environ:
        return True

    return False


def _get_container_name() -> str:
    """Get the appropriate container name based on runtime context."""
    base_name = "fastled-wasm-container"

    if not _is_running_under_github_actions() and _is_running_under_pytest():
        # Use test container name when running under pytest
        return f"{base_name}-test{PLATFORM_TAG}"
    else:
        # Use regular container name
        return f"{base_name}{PLATFORM_TAG}"


def _get_server_port() -> int:
    """Get the appropriate server port based on runtime context."""
    if not _is_running_under_github_actions() and _is_running_under_pytest():
        # Use test port when running under pytest to avoid conflicts
        return 9022
    else:
        # Use regular port
        return 9021


CONTAINER_NAME = _get_container_name()
DEFAULT_URL = str(os.environ.get("FASTLED_URL", "https://fastled.onrender.com"))
SERVER_PORT = _get_server_port()
AUTH_TOKEN = "oBOT5jbsO4ztgrpNsQwlmFLIKB"

IMAGE_NAME = "niteris/fastled-wasm"
DEFAULT_CONTAINER_NAME = _get_container_name()
# IMAGE_TAG = "latest"

DOCKER_FILE = (
    "https://raw.githubusercontent.com/zackees/fastled-wasm/refs/heads/main/Dockerfile"
)
