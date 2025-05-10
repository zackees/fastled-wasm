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
