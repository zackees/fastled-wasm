import hashlib
from pathlib import Path


def hash_file(file_path: Path) -> str:
    hasher = hashlib.sha256()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def banner_string(msg: str) -> str:
    lines = msg.splitlines()
    max_length = max(len(line) for line in lines)
    border = "#" * (max_length + 4)
    out: list[str] = []
    out.append(border)
    for line in lines:
        out.append(f"# {line} " + " " * (max_length - len(line)) + "#")
    out.append(border)
    outstr = "\n".join(out)
    return f"\n{outstr}\n"


def print_banner(msg: str) -> None:
    """Print a message in a banner format."""
    print(banner_string(msg))


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


def download_emsdk_headers(base_url: str, filepath: Path) -> None:
    """Download EMSDK headers from the specified URL and save to filepath.

    Args:
        base_url: Base URL of the server (e.g., 'http://localhost:8080')
        filepath: Path where to save the headers ZIP file (must end with .zip)

    Raises:
        ValueError: If filepath doesn't end with .zip
        RuntimeError: If download fails or server returns error
    """
    if not str(filepath).endswith(".zip"):
        raise ValueError("Filepath must end with .zip")

    import httpx

    try:
        timeout = httpx.Timeout(30.0, read=30.0)
        with httpx.stream(
            "GET", f"{base_url}/headers/emsdk", timeout=timeout
        ) as response:
            if response.status_code == 200:
                filepath.parent.mkdir(parents=True, exist_ok=True)
                with open(filepath, "wb") as f:
                    for chunk in response.iter_bytes(chunk_size=512000):
                        f.write(chunk)
            else:
                raise RuntimeError(
                    f"Failed to get EMSDK headers: HTTP {response.status_code}"
                )
    except Exception as e:
        raise RuntimeError(f"Error downloading EMSDK headers: {e}") from e
