"""
Interruptible HTTP requests that can be cancelled with Ctrl+C.

This module provides cross-platform HTTP request functionality that can be
interrupted with Ctrl+C by using asyncio cancellation and periodic checks.
"""

import asyncio

import httpx


class InterruptibleHTTPRequest:
    """A wrapper for making HTTP requests that can be interrupted by Ctrl+C."""

    def __init__(self):
        self.cancelled = False

    async def _make_request_async(
        self,
        url: str,
        files: dict,
        headers: dict,
        transport: httpx.HTTPTransport | None = None,
        timeout: float = 240,
        follow_redirects: bool = True,
    ) -> httpx.Response:
        """Make an async HTTP request."""
        # Convert sync transport to async transport if provided
        async_transport = None
        if transport is not None:
            # For IPv4 connections, create async transport with local address
            async_transport = httpx.AsyncHTTPTransport(local_address="0.0.0.0")

        async with httpx.AsyncClient(
            transport=async_transport,
            timeout=timeout,
        ) as client:
            response = await client.post(
                url,
                follow_redirects=follow_redirects,
                files=files,
                headers=headers,
            )
            return response

    def make_request_interruptible(
        self,
        url: str,
        files: dict,
        headers: dict,
        transport: httpx.HTTPTransport | None = None,
        timeout: float = 240,
        follow_redirects: bool = True,
    ) -> httpx.Response:
        """Make an HTTP request that can be interrupted by Ctrl+C."""
        try:
            # Create a new event loop if we're not in one
            try:
                loop = asyncio.get_running_loop()
                # We're already in an event loop, use run_in_executor
                return asyncio.run_coroutine_threadsafe(
                    self._run_with_keyboard_check(
                        url, files, headers, transport, timeout, follow_redirects
                    ),
                    loop,
                ).result()
            except RuntimeError:
                # No running loop, create one
                return asyncio.run(
                    self._run_with_keyboard_check(
                        url, files, headers, transport, timeout, follow_redirects
                    )
                )
        except KeyboardInterrupt:
            print("\nHTTP request cancelled by user")
            raise

    async def _run_with_keyboard_check(
        self,
        url: str,
        files: dict,
        headers: dict,
        transport: httpx.HTTPTransport | None = None,
        timeout: float = 240,
        follow_redirects: bool = True,
    ) -> httpx.Response:
        """Run the request with periodic keyboard interrupt checks."""
        task = asyncio.create_task(
            self._make_request_async(
                url, files, headers, transport, timeout, follow_redirects
            )
        )

        # Poll for keyboard interrupt while waiting for the request
        # This approach allows the task to be cancelled when KeyboardInterrupt
        # is raised in the calling thread
        while not task.done():
            try:
                # Wait for either completion or a short timeout
                response = await asyncio.wait_for(asyncio.shield(task), timeout=0.1)
                return response
            except asyncio.TimeoutError:
                # Continue waiting - the short timeout allows for more responsive
                # cancellation when KeyboardInterrupt is raised
                continue
            except KeyboardInterrupt:
                task.cancel()
                print("\nHTTP request cancelled by user")
                raise

        return await task


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
    request_handler = InterruptibleHTTPRequest()
    return request_handler.make_request_interruptible(
        url=url,
        files=files or {},
        headers=headers or {},
        transport=transport,
        timeout=timeout,
        follow_redirects=follow_redirects,
    )
