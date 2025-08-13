"""Header dump functionality for EMSDK headers export."""

import sys
from pathlib import Path


def dump_emsdk_headers(output_path: Path | str, server_url: str | None = None) -> None:
    """
    Dump EMSDK headers to a specified path.

    Args:
        output_path: Path where to save the headers ZIP file
        server_url: URL of the server. If None, tries to create local server first,
                   then falls back to remote server if local fails.
    """
    from fastled import Api
    from fastled.settings import DEFAULT_URL
    from fastled.util import download_emsdk_headers

    # Convert to Path if string
    if isinstance(output_path, str):
        output_path = Path(output_path)

    ends_with_zip = output_path.suffix == ".zip"
    if not ends_with_zip:
        raise ValueError(f"{output_path} must end with .zip")

    try:
        if server_url is not None:
            # Use the provided server URL
            download_emsdk_headers(server_url, output_path)
            print(f"SUCCESS: EMSDK headers exported to {output_path}")
        else:
            # Try to create local server first
            try:
                with Api.server() as server:
                    base_url = server.url()
                    download_emsdk_headers(base_url, output_path)
                    print(
                        f"SUCCESS: EMSDK headers exported to {output_path} (using local server)"
                    )
            except Exception as local_error:
                print(
                    f"WARNING: Local server failed ({local_error}), falling back to remote server"
                )
                # Fall back to remote server
                download_emsdk_headers(DEFAULT_URL, output_path)
                print(
                    f"SUCCESS: EMSDK headers exported to {output_path} (using remote server)"
                )

    except Exception as e:
        print(f"ERROR: Exception during header dump: {e}")
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python -m fastled.header_dump <output_path>")
        sys.exit(1)

    output_path = sys.argv[1]
    dump_emsdk_headers(output_path)
