import importlib.resources as pkg_resources
from dataclasses import dataclass
from pathlib import Path


@dataclass
class SslConfig:
    certfile: Path
    keyfile: Path


def get_asset_path(filename: str) -> Path | None:
    """Locate a file from the fastled.assets package resources."""
    try:
        resource = pkg_resources.files("fastled.assets").joinpath(filename)
        # Convert to Path for file-system access
        path = Path(str(resource))
        return path if path.exists() else None
    except (ModuleNotFoundError, AttributeError):
        return None


def get_ssl_config() -> SslConfig | None:
    """Get the keys for the server"""
    # certfile = get_asset_path("localhost-key.pem")
    # keyfile = get_asset_path("localhost.pem")
    # if certfile is None or keyfile is None:
    #     raise ValueError("Could not find keys for server")
    # # return certfile, keyfile
    # return SslConfig(certfile=certfile, keyfile=keyfile)
    return None
