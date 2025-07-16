"""
Resize tracking for Playwright browser integration.

This module provides a class to track browser window resize events and adjust
the viewport accordingly without using internal polling loops.
"""

from typing import Any


class ResizeTracker:
    """Tracks browser window resize events and adjusts viewport accordingly."""

    def __init__(self, page: Any):
        """Initialize the resize tracker.

        Args:
            page: The Playwright page object to track
        """
        self.page = page
        self.last_outer_size: tuple[int, int] | None = None

    async def _get_window_info(self) -> dict[str, int] | None:
        """Get browser window dimensions information.

        Returns:
            Dictionary containing window dimensions or None if unable to retrieve
        """
        if self.page is None:
            return None

        try:
            return await self.page.evaluate(
                """
                () => {
                    return {
                        outerWidth: window.outerWidth,
                        outerHeight: window.outerHeight,
                        innerWidth: window.innerWidth,
                        innerHeight: window.innerHeight,
                        contentWidth: document.documentElement.clientWidth,
                        contentHeight: document.documentElement.clientHeight
                    };
                }
                """
            )
        except Exception:
            return None

    async def set_viewport_size(self, width: int, height: int) -> None:
        """Set the viewport size.

        Args:
            width: The viewport width
            height: The viewport height
        """
        if self.page is None:
            raise Exception("Page is None")

        await self.page.set_viewport_size({"width": width, "height": height})

    async def update(self) -> Exception | None:
        """Update the resize tracking and adjust viewport if needed.

        Returns:
            None if successful, Exception if an error occurred
        """
        try:
            # Check if page is still alive
            if self.page is None or self.page.is_closed():
                return Exception("Page closed")

            window_info = await self._get_window_info()

            if window_info:
                current_outer = (
                    window_info["outerWidth"],
                    window_info["outerHeight"],
                )

                # Check if window size changed
                if (
                    self.last_outer_size is None
                    or current_outer != self.last_outer_size
                ):

                    if self.last_outer_size is not None:
                        print("[PYTHON] *** BROWSER WINDOW RESIZED ***")
                        print(
                            f"[PYTHON] Outer window changed from {self.last_outer_size[0]}x{self.last_outer_size[1]} to {current_outer[0]}x{current_outer[1]}"
                        )

                    self.last_outer_size = current_outer

                    # Set viewport to match the outer window size
                    outer_width = int(window_info["outerWidth"])
                    outer_height = int(window_info["outerHeight"])

                    print(
                        f"[PYTHON] Setting viewport to match outer window size: {outer_width}x{outer_height}"
                    )

                    await self.set_viewport_size(outer_width, outer_height)
                    print("[PYTHON] Viewport set successfully")

                    # Query the actual window dimensions after the viewport change
                    updated_window_info = await self._get_window_info()

                    if updated_window_info:
                        print(f"[PYTHON] Updated window info: {updated_window_info}")

                        # Update our tracking with the actual final outer size
                        self.last_outer_size = (
                            updated_window_info["outerWidth"],
                            updated_window_info["outerHeight"],
                        )
                        print(
                            f"[PYTHON] Updated last_outer_size to actual final size: {self.last_outer_size}"
                        )
                    else:
                        print("[PYTHON] Could not get updated window info")

        except Exception as e:
            return e

        # Success case
        return None
