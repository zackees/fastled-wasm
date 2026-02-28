"""
Interruptible HTTP requests that can be cancelled with Ctrl+C.

This module provides cross-platform HTTP request functionality that can be
interrupted with Ctrl+C by running the request in a background thread and
polling with short timeouts to stay responsive to KeyboardInterrupt.
"""

import threading

import httpx


def make_interruptible_post_request(
    url: str,
    files: dict | None = None,
    headers: dict | None = None,
    transport: httpx.HTTPTransport | None = None,
    timeout: float = 240,
    follow_redirects: bool = True,
) -> httpx.Response:
    """
    Convenience function to make an interruptible POST request.

    Args:
        url: The URL to make the request to
        files: Files to upload (optional)
        headers: HTTP headers (optional)
        transport: HTTP transport to use (optional)
        timeout: Request timeout in seconds
        follow_redirects: Whether to follow redirects

    Returns:
        The HTTP response

    Raises:
        KeyboardInterrupt: If the request was cancelled by Ctrl+C
    """
    result: list[httpx.Response | None] = [None]
    error: list[BaseException | None] = [None]

    def _do_request() -> None:
        try:
            transport_obj = (
                httpx.HTTPTransport(local_address="0.0.0.0") if transport else None
            )
            with httpx.Client(transport=transport_obj, timeout=timeout) as client:
                result[0] = client.post(
                    url,
                    files=files or {},
                    headers=headers or {},
                    follow_redirects=follow_redirects,
                )
        except Exception as e:
            error[0] = e

    thread = threading.Thread(target=_do_request, daemon=True)
    thread.start()

    try:
        while thread.is_alive():
            thread.join(timeout=0.1)
    except KeyboardInterrupt:
        print("\nHTTP request cancelled by user")
        raise

    if error[0] is not None:
        raise error[0]
    assert result[0] is not None
    return result[0]
