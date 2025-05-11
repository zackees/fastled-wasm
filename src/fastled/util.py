import hashlib
from pathlib import Path


def hash_file(file_path: Path) -> str:
    hasher = hashlib.sha256()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def banner_string(msg: str) -> str:
    """
    Return `msg` surrounded by a border of # characters, including
    leading and trailing newlines.
    """
    border = "#" * (len(msg) + 4)
    return f"\n{border}\n# {msg}\n{border}\n"


def port_is_free(port: int) -> bool:
    """Check if a port is free."""
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            _ = sock.bind(("localhost", port)) and sock.bind(("0.0.0.0", port))
            return True
        except OSError:
            return False


def find_free_port(start_port: int, end_port: int) -> int | None:
    """Find a free port on the system."""

    for port in range(start_port, end_port):
        if port_is_free(port):
            return port
    import warnings

    warnings.warn(
        f"No free port found in the range {start_port}-{end_port}. Using {start_port}."
    )
    return None
