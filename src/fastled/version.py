import httpx

from fastled.__version__ import __version_url_latest__


def get_latest_version() -> str | Exception:
    """
    Fetch the latest version from the GitHub repository.
    """
    try:
        response = httpx.get(__version_url_latest__)
        response.raise_for_status()
        # Extract the version string from the response text
        version_line = response.text.split("__version__ = ")[1].split('"')[1]
        return version_line
    except Exception as e:
        return e


if __name__ == "__main__":
    latest_version = get_latest_version()
    if isinstance(latest_version, Exception):
        print(f"Error fetching latest version: {latest_version}")
    else:
        print(f"Latest version: {latest_version}")
