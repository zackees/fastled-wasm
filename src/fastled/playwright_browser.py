"""
Playwright browser integration for FastLED WASM compiler.

This module provides functionality to open the compiled FastLED sketch
in a Playwright browser instead of the default system browser when
the 'full' optional dependency is installed.
"""

import asyncio
import sys
import warnings
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from playwright.async_api import Browser, Page

try:
    from playwright.async_api import Browser, Page, async_playwright

    PLAYWRIGHT_AVAILABLE = True
except ImportError:
    PLAYWRIGHT_AVAILABLE = False
    async_playwright = None
    Browser = Any  # type: ignore
    Page = Any  # type: ignore


def is_playwright_available() -> bool:
    """Check if Playwright is available."""
    return PLAYWRIGHT_AVAILABLE


class PlaywrightBrowser:
    """Playwright browser manager for FastLED sketches."""

    def __init__(self, headless: bool = False, auto_resize: bool = True):
        """Initialize the Playwright browser manager.

        Args:
            headless: Whether to run the browser in headless mode
            auto_resize: Whether to automatically resize the browser window to fit content
        """
        if not PLAYWRIGHT_AVAILABLE:
            raise ImportError(
                "Playwright is not installed. Install with: pip install fastled[full]"
            )

        # debug
        auto_resize = True

        self.headless = headless
        self.auto_resize = auto_resize
        self.browser: Any = None
        self.page: Any = None
        self.playwright: Any = None

    async def start(self) -> None:
        """Start the Playwright browser."""
        if self.playwright is None and async_playwright is not None:
            self.playwright = await async_playwright().start()

        if self.browser is None and self.playwright is not None:
            # Try Chrome first, then Firefox, then WebKit
            try:
                self.browser = await self.playwright.chromium.launch(
                    headless=self.headless,
                    args=["--disable-web-security", "--allow-running-insecure-content"],
                )
            except Exception:
                try:
                    self.browser = await self.playwright.firefox.launch(
                        headless=self.headless
                    )
                except Exception:
                    self.browser = await self.playwright.webkit.launch(
                        headless=self.headless
                    )

        if self.page is None and self.browser is not None:
            # Create a new browser context and page
            context = await self.browser.new_context()
            self.page = await context.new_page()

    async def open_url(self, url: str) -> None:
        """Open a URL in the Playwright browser.

        Args:
            url: The URL to open
        """
        if self.page is None:
            await self.start()

        print(f"Opening FastLED sketch in Playwright browser: {url}")
        if self.page is not None:
            await self.page.goto(url)

            # Wait for the page to load
            await self.page.wait_for_load_state("networkidle")

            # Set up auto-resizing functionality if enabled
            if self.auto_resize:
                await self._setup_auto_resize()

    async def _setup_auto_resize(self) -> None:
        """Set up automatic window resizing based on content size."""
        if self.page is None:
            print("[PYTHON] Cannot setup auto-resize: page is None")
            return

        print(
            "[PYTHON] Setting up browser window tracking with viewport-only adjustment"
        )

        # Start polling loop that tracks browser window changes and adjusts viewport only
        asyncio.create_task(self._track_browser_adjust_viewport())

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

    async def _track_browser_adjust_viewport(self) -> None:
        """Track browser window outer size changes and adjust viewport accordingly."""
        if self.page is None:
            return

        print(
            "[PYTHON] Starting browser window tracking (outer size → viewport adjustment)"
        )
        last_outer_size = None

        while True:
            try:
                # Wait 1 second between polls
                await asyncio.sleep(1)

                # Check if page is still alive
                if self.page is None or self.page.is_closed():
                    print("[PYTHON] Page closed, stopping browser tracking")
                    break

                # Get browser window dimensions
                window_info = await self._get_window_info()

                if window_info:
                    current_outer = (
                        window_info["outerWidth"],
                        window_info["outerHeight"],
                    )

                    # Print current state occasionally
                    if last_outer_size is None or current_outer != last_outer_size:
                        print(
                            f"[PYTHON] Browser: outer={window_info['outerWidth']}x{window_info['outerHeight']}, content={window_info['contentWidth']}x{window_info['contentHeight']}"
                        )

                    # Track changes in OUTER window size (user resizes browser)
                    if last_outer_size is None or current_outer != last_outer_size:

                        if last_outer_size is not None:
                            print("[PYTHON] *** BROWSER WINDOW RESIZED ***")
                            print(
                                f"[PYTHON] Outer window changed from {last_outer_size[0]}x{last_outer_size[1]} to {current_outer[0]}x{current_outer[1]}"
                            )

                        last_outer_size = current_outer

                        # Set viewport to match the outer window size
                        if not self.headless:
                            try:
                                outer_width = int(window_info["outerWidth"])
                                outer_height = int(window_info["outerHeight"])

                                print(
                                    f"[PYTHON] Setting viewport to match outer window size: {outer_width}x{outer_height}"
                                )

                                await self.page.set_viewport_size(
                                    {"width": outer_width, "height": outer_height}
                                )
                                print("[PYTHON] Viewport set successfully")

                                # Wait briefly for browser to settle after viewport change
                                # await asyncio.sleep(0.5)

                                # Query the actual window dimensions after the viewport change
                                updated_window_info = await self._get_window_info()

                                if updated_window_info:

                                    # Update our tracking with the actual final outer size
                                    last_outer_size = (
                                        updated_window_info["outerWidth"],
                                        updated_window_info["outerHeight"],
                                    )
                                    print(
                                        f"[PYTHON] Updated last_outer_size to actual final size: {last_outer_size}"
                                    )
                                else:
                                    print("[PYTHON] Could not get updated window info")

                            except Exception as e:
                                print(f"[PYTHON] Failed to set viewport: {e}")

                else:
                    print("[PYTHON] Could not get browser window info")

            except Exception as e:
                print(f"[PYTHON] Error in browser tracking: {e}")
                continue

    async def wait_for_close(self) -> None:
        """Wait for the browser to be closed."""
        if self.browser is None:
            return

        try:
            # Wait for the browser to be closed
            while not self.browser.is_closed():
                await asyncio.sleep(1)
        except Exception:
            pass

    async def close(self) -> None:
        """Close the Playwright browser."""
        if self.page:
            await self.page.close()
            self.page = None

        if self.browser:
            await self.browser.close()
            self.browser = None

        if self.playwright:
            await self.playwright.stop()
            self.playwright = None


def run_playwright_browser(
    url: str, headless: bool = False, auto_resize: bool = True
) -> None:
    """Run Playwright browser in a separate process.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        auto_resize: Whether to automatically resize the browser window to fit content
    """
    if not PLAYWRIGHT_AVAILABLE:
        warnings.warn(
            "Playwright is not installed. Install with: pip install fastled[full]. "
            "Falling back to default browser."
        )
        return

    async def main():
        browser = PlaywrightBrowser(headless=headless, auto_resize=auto_resize)
        try:
            await browser.start()
            await browser.open_url(url)

            if not headless:
                print("Playwright browser opened. Press Ctrl+C to close.")
                await browser.wait_for_close()
            else:
                # In headless mode, just wait a bit for the page to load
                await asyncio.sleep(2)

        except KeyboardInterrupt:
            print("\nClosing Playwright browser...")
        finally:
            await browser.close()

    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nPlaywright browser closed.")
    except Exception as e:
        warnings.warn(
            f"Playwright browser failed: {e}. Falling back to default browser."
        )


class PlaywrightBrowserProxy:
    """Proxy object to manage Playwright browser lifecycle."""

    def __init__(self):
        self.process = None
        self.browser_manager = None

    def open(self, url: str, headless: bool = False, auto_resize: bool = True) -> None:
        """Open URL with Playwright browser and keep it alive.

        Args:
            url: The URL to open
            headless: Whether to run in headless mode
            auto_resize: Whether to automatically resize the browser window to fit content
        """
        if not PLAYWRIGHT_AVAILABLE:
            warnings.warn(
                "Playwright is not installed. Install with: pip install fastled[full]. "
                "Falling back to default browser."
            )
            # Fall back to default browser
            import webbrowser

            webbrowser.open(url)
            return

        try:
            # Run Playwright in a separate process to avoid blocking
            import multiprocessing

            self.process = multiprocessing.Process(
                target=run_playwright_browser_persistent,
                args=(url, headless, auto_resize),
            )
            self.process.start()

            # Register cleanup
            import atexit

            atexit.register(self.close)

        except Exception as e:
            warnings.warn(
                f"Failed to start Playwright browser: {e}. Falling back to default browser."
            )
            import webbrowser

            webbrowser.open(url)

    def close(self) -> None:
        """Close the Playwright browser."""
        if self.process and self.process.is_alive():
            print("Closing Playwright browser...")
            self.process.terminate()
            self.process.join(timeout=5)
            if self.process.is_alive():
                self.process.kill()
            self.process = None


def run_playwright_browser_persistent(
    url: str, headless: bool = False, auto_resize: bool = True
) -> None:
    """Run Playwright browser in a persistent mode that stays alive until terminated.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        auto_resize: Whether to automatically resize the browser window to fit content
    """
    if not PLAYWRIGHT_AVAILABLE:
        return

    async def main():
        browser = PlaywrightBrowser(headless=headless, auto_resize=auto_resize)
        try:
            await browser.start()
            await browser.open_url(url)

            print(
                "Playwright browser opened. Browser will remain open until the FastLED process exits."
            )

            # Keep the browser alive indefinitely
            while True:
                await asyncio.sleep(1)

        except KeyboardInterrupt:
            print("\nClosing Playwright browser...")
        except Exception as e:
            print(f"Playwright browser error: {e}")
        finally:
            await browser.close()

    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nPlaywright browser closed.")
    except Exception as e:
        print(f"Playwright browser failed: {e}")


def open_with_playwright(
    url: str, headless: bool = False, auto_resize: bool = True
) -> PlaywrightBrowserProxy:
    """Open URL with Playwright browser and return a proxy object for lifecycle management.

    This function can be used as a drop-in replacement for webbrowser.open().

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        auto_resize: Whether to automatically resize the browser window to fit content

    Returns:
        PlaywrightBrowserProxy object for managing the browser lifecycle
    """
    proxy = PlaywrightBrowserProxy()
    proxy.open(url, headless, auto_resize)
    return proxy


def install_playwright_browsers() -> bool:
    """Install Playwright browsers if not already installed.

    Returns:
        True if installation was successful or browsers were already installed
    """
    if not PLAYWRIGHT_AVAILABLE:
        return False

    try:
        from playwright.sync_api import sync_playwright

        with sync_playwright() as p:
            # Try to launch a browser to see if it's installed
            try:
                browser = p.chromium.launch(headless=True)
                browser.close()
                return True
            except Exception:
                pass

        # If we get here, browsers need to be installed
        print("Installing Playwright browsers...")
        import subprocess

        result = subprocess.run(
            [sys.executable, "-m", "playwright", "install", "chromium"],
            capture_output=True,
            text=True,
        )

        if result.returncode == 0:
            print("Playwright browsers installed successfully.")
            return True
        else:
            print(f"Failed to install Playwright browsers: {result.stderr}")
            return False

    except Exception as e:
        print(f"Error installing Playwright browsers: {e}")
        return False
