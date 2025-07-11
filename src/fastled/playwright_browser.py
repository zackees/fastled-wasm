"""
Playwright browser integration for FastLED WASM compiler.

This module provides a Playwright-based browser implementation that can be used
in a Playwright browser instead of the default system browser when
Playwright is available.
"""

import asyncio
import os
import sys
import threading
import warnings
from pathlib import Path
from typing import Any

# Set custom Playwright browser installation path
PLAYWRIGHT_DIR = Path.home() / ".fastled" / "playwright"
PLAYWRIGHT_DIR.mkdir(parents=True, exist_ok=True)
os.environ["PLAYWRIGHT_BROWSERS_PATH"] = str(PLAYWRIGHT_DIR)

try:
    from playwright.async_api import Browser, Page, async_playwright

    PLAYWRIGHT_AVAILABLE = True
except ImportError:
    PLAYWRIGHT_AVAILABLE = False
    async_playwright = None
    Browser = None
    Page = None


def is_playwright_available() -> bool:
    """Check if Playwright is available."""
    return PLAYWRIGHT_AVAILABLE


class PlaywrightBrowser:
    """Playwright browser manager for FastLED sketches."""

    def __init__(self, headless: bool = False):
        """Initialize the Playwright browser manager.

        Args:
            headless: Whether to run the browser in headless mode
        """
        if not PLAYWRIGHT_AVAILABLE:
            raise ImportError(
                "Playwright is not installed. Install with: pip install fastled[full]"
            )

        self.headless = headless
        self.auto_resize = True  # Always enable auto-resize
        self.browser: Any = None
        self.page: Any = None
        self.playwright: Any = None
        self._should_exit = asyncio.Event()

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
                    print("[PYTHON] Page closed, signaling exit")
                    self._should_exit.set()
                    return

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


def run_playwright_browser(url: str, headless: bool = False) -> None:
    """Run Playwright browser in a separate process.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
    """
    if not PLAYWRIGHT_AVAILABLE:
        warnings.warn(
            "Playwright is not installed. Install with: pip install fastled[full]. "
            "Falling back to default browser."
        )
        import webbrowser

        webbrowser.open(url)
        return

    async def main():
        browser = None
        try:
            browser = PlaywrightBrowser(headless=headless)
            await browser.start()
            await browser.open_url(url)

            print("Playwright browser opened. Press Ctrl+C to close.")
            await browser.wait_for_close()

        except Exception as e:
            # If we get an error that suggests browsers aren't installed, try to install them
            if "executable doesn't exist" in str(e) or "Browser not found" in str(e):
                print("🎭 Playwright browsers not found. Installing...")
                if install_playwright_browsers():
                    print("🎭 Retrying browser startup...")
                    # Try again with fresh browser instance
                    browser = PlaywrightBrowser(headless=headless)
                    await browser.start()
                    await browser.open_url(url)

                    print("Playwright browser opened. Press Ctrl+C to close.")
                    await browser.wait_for_close()
                else:
                    print("❌ Failed to install Playwright browsers")
                    raise e
            else:
                raise e
        except KeyboardInterrupt:
            print("\nClosing Playwright browser...")
        finally:
            if browser is not None:
                try:
                    await browser.close()
                except Exception as e:
                    print(f"Warning: Failed to close Playwright browser: {e}")

    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nPlaywright browser closed.")
    except Exception as e:
        print(f"Playwright browser failed: {e}. Falling back to default browser.")
        import webbrowser

        webbrowser.open(url)


class PlaywrightBrowserProxy:
    """Proxy object to manage Playwright browser lifecycle."""

    def __init__(self):
        self.process = None
        self.browser_manager = None
        self.monitor_thread = None
        self._closing_intentionally = False

    def open(self, url: str, headless: bool = False) -> None:
        """Open URL with Playwright browser and keep it alive.

        Args:
            url: The URL to open
            headless: Whether to run in headless mode
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
                args=(url, headless),
            )
            self.process.start()

            # Start monitoring thread to exit main process when browser subprocess exits
            self._start_monitor_thread()

            # Register cleanup
            import atexit

            atexit.register(self.close)

        except Exception as e:
            warnings.warn(
                f"Failed to start Playwright browser: {e}. Falling back to default browser."
            )
            import webbrowser

            webbrowser.open(url)

    def _start_monitor_thread(self) -> None:
        """Start a thread to monitor the browser process and exit main process when it terminates."""
        if self.monitor_thread is not None:
            return

        import os

        def monitor_process():
            """Monitor the browser process and exit when it terminates."""
            if self.process is None:
                return

            try:
                # Wait for the process to terminate
                self.process.join()

                # Check if the process terminated (and we didn't kill it ourselves)
                if (
                    self.process.exitcode is not None
                    and not self._closing_intentionally
                ):
                    print("[MAIN] Browser closed, exiting main program")
                    # Force exit the entire program
                    os._exit(0)

            except Exception as e:
                print(f"[MAIN] Error monitoring browser process: {e}")

        self.monitor_thread = threading.Thread(target=monitor_process, daemon=True)
        self.monitor_thread.start()

    def close(self) -> None:
        """Close the Playwright browser."""
        if self.process and self.process.is_alive():
            print("Closing Playwright browser...")
            # Mark that we're intentionally closing to prevent monitor from triggering exit
            self._closing_intentionally = True
            self.process.terminate()
            self.process.join(timeout=5)
            if self.process.is_alive():
                self.process.kill()
            self.process = None


def run_playwright_browser_persistent(url: str, headless: bool = False) -> None:
    """Run Playwright browser in a persistent mode that stays alive until terminated.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
    """
    if not PLAYWRIGHT_AVAILABLE:
        return

    async def main():
        browser = None
        try:
            browser = PlaywrightBrowser(headless=headless)
            await browser.start()
            await browser.open_url(url)

            print(
                "Playwright browser opened. Browser will remain open until the FastLED process exits."
            )

            # Keep the browser alive until exit is signaled
            while not browser._should_exit.is_set():
                await asyncio.sleep(0.1)

        except Exception as e:
            # If we get an error that suggests browsers aren't installed, try to install them
            if "executable doesn't exist" in str(e) or "Browser not found" in str(e):
                print("🎭 Playwright browsers not found. Installing...")
                if install_playwright_browsers():
                    print("🎭 Retrying browser startup...")
                    # Try again with fresh browser instance
                    browser = PlaywrightBrowser(headless=headless)
                    await browser.start()
                    await browser.open_url(url)

                    print(
                        "Playwright browser opened. Browser will remain open until the FastLED process exits."
                    )

                    # Keep the browser alive until exit is signaled
                    while not browser._should_exit.is_set():
                        await asyncio.sleep(0.1)
                else:
                    print("❌ Failed to install Playwright browsers")
                    raise e
            else:
                raise e
        except KeyboardInterrupt:
            print("\nClosing Playwright browser...")
        finally:
            if browser is not None:
                try:
                    await browser.close()
                except Exception as e:
                    print(f"Warning: Failed to close Playwright browser: {e}")

    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nPlaywright browser closed.")
    except Exception as e:
        print(f"Playwright browser failed: {e}")


def open_with_playwright(url: str, headless: bool = False) -> PlaywrightBrowserProxy:
    """Open URL with Playwright browser and return a proxy object for lifecycle management.

    This function can be used as a drop-in replacement for webbrowser.open().

    Args:
        url: The URL to open
        headless: Whether to run in headless mode

    Returns:
        PlaywrightBrowserProxy object for managing the browser lifecycle
    """
    proxy = PlaywrightBrowserProxy()
    proxy.open(url, headless)
    return proxy


def install_playwright_browsers() -> bool:
    """Install Playwright browsers if not already installed.

    Installs browsers to ~/.fastled/playwright directory.

    Returns:
        True if installation was successful or browsers were already installed
    """
    if not PLAYWRIGHT_AVAILABLE:
        return False

    try:
        import os
        from pathlib import Path

        # Set custom browser installation path
        playwright_dir = Path.home() / ".fastled" / "playwright"
        playwright_dir.mkdir(parents=True, exist_ok=True)

        # Set environment variable for Playwright browser path
        os.environ["PLAYWRIGHT_BROWSERS_PATH"] = str(playwright_dir)

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
        print(f"Installing to: {playwright_dir}")
        import subprocess

        result = subprocess.run(
            [sys.executable, "-m", "playwright", "install", "chromium"],
            capture_output=True,
            text=True,
            env=dict(os.environ, PLAYWRIGHT_BROWSERS_PATH=str(playwright_dir)),
        )

        if result.returncode == 0:
            print("✅ Playwright browsers installed successfully!")
            print(f"   Location: {playwright_dir}")
            return True
        else:
            print(f"❌ Failed to install Playwright browsers: {result.stderr}")
            return False

    except Exception as e:
        print(f"❌ Error installing Playwright browsers: {e}")
        return False
