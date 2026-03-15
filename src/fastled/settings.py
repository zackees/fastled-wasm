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
    if "pytest" in sys.modules:
        return True
    if "PYTEST_CURRENT_TEST" in os.environ:
        return True
    return False


def _get_server_port() -> int:
    """Get the appropriate server port based on runtime context."""
    if not _is_running_under_github_actions() and _is_running_under_pytest():
        return 9022
    else:
        return 9021


SERVER_PORT = _get_server_port()
DEFAULT_PORT = SERVER_PORT
