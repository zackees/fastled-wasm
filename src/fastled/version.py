from concurrent.futures import Future, ThreadPoolExecutor

import httpx

from fastled.__version__ import __version_url_latest__


def _fetch_version() -> str | Exception:
    """
    Helper function to fetch the latest version from the GitHub repository.
    """
    try:
        response = httpx.get(__version_url_latest__)
        response.raise_for_status()
        # Extract the version string from the response text
        version_line = response.text.split("__version__ = ")[1].split('"')[1]
        return version_line
    except Exception as e:
        return e


def get_latest_version() -> Future[str | Exception]:
    """
    Fetch the latest version from the GitHub repository.
    Returns a future that will resolve with the version string or an exception.
    """
    executor = ThreadPoolExecutor()
    return executor.submit(_fetch_version)


def unit_test() -> None:
    future = get_latest_version()
    latest_version = future.result()  # Wait for the future to complete
    if isinstance(latest_version, Exception):
        print(f"Error fetching latest version: {latest_version}")
    else:
        print(f"Latest version: {latest_version}")


if __name__ == "__main__":
    unit_test()
