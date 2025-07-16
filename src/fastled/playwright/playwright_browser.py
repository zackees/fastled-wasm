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

from fastled.playwright.resize_tracking import ResizeTracker

# Set custom Playwright browser installation path
PLAYWRIGHT_DIR = Path.home() / ".fastled" / "playwright"
PLAYWRIGHT_DIR.mkdir(parents=True, exist_ok=True)


def get_chromium_executable_path() -> str | None:
    """Get the path to the custom Chromium executable if it exists."""
    import glob
    import platform

    playwright_dir = PLAYWRIGHT_DIR

    if platform.system() == "Windows":
        chromium_pattern = str(
            playwright_dir / "chromium-*" / "chrome-win" / "chrome.exe"
        )
    elif platform.system() == "Darwin":  # macOS
        chromium_pattern = str(
            playwright_dir
            / "chromium-*"
            / "chrome-mac"
            / "Chromium.app"
            / "Contents"
            / "MacOS"
            / "Chromium"
        )
    else:  # Linux
        chromium_pattern = str(
            playwright_dir / "chromium-*" / "chrome-linux" / "chrome"
        )

    matches = glob.glob(chromium_pattern)
    return matches[0] if matches else None


class PlaywrightBrowser:
    """Playwright browser manager for FastLED sketches."""

    def __init__(self, headless: bool = False, enable_extensions: bool = True):
        """Initialize the Playwright browser manager.

        Args:
            headless: Whether to run the browser in headless mode
            enable_extensions: Whether to enable Chrome extensions (C++ DevTools Support)
        """

        self.headless = headless
        self.enable_extensions = enable_extensions
        self.auto_resize = True  # Always enable auto-resize
        self.browser: Any = None
        self.context: Any = None
        self.page: Any = None
        self.playwright: Any = None
        self._should_exit = asyncio.Event()
        self._extensions_dir: Path | None = None
        self.resize_tracker: ResizeTracker | None = None

        # Initialize extensions if enabled
        if self.enable_extensions:
            self._setup_extensions()

    def _setup_extensions(self) -> None:
        """Setup Chrome extensions for enhanced debugging."""
        try:
            from fastled.playwright.chrome_extension_downloader import (
                download_cpp_devtools_extension,
            )

            extension_path = download_cpp_devtools_extension()
            if extension_path and extension_path.exists():
                self._extensions_dir = extension_path
                print(
                    f"[PYTHON] C++ DevTools Support extension ready: {extension_path}"
                )
            else:
                print("[PYTHON] Warning: C++ DevTools Support extension not available")
                self.enable_extensions = False
        except Exception as e:
            print(f"[PYTHON] Warning: Failed to setup Chrome extensions: {e}")
            self.enable_extensions = False

    def _detect_device_scale_factor(self) -> float | None:
        """Detect the system's device scale factor for natural browser behavior.

        Returns:
            The detected device scale factor, or None if detection fails or
            the value is outside reasonable bounds (0.5-4.0).
        """
        try:
            import tkinter

            root = tkinter.Tk()
            root.withdraw()  # Hide the window
            scale_factor = root.winfo_fpixels("1i") / 72.0
            root.destroy()

            # Validate the scale factor is in a reasonable range
            if 0.5 <= scale_factor <= 4.0:
                return scale_factor
            else:
                print(
                    f"[PYTHON] Detected scale factor {scale_factor:.2f} is outside reasonable bounds (0.5-4.0)"
                )
                return None

        except Exception as e:
            print(f"[PYTHON] Could not detect device scale factor: {e}")
            return None

    async def start(self) -> None:
        """Start the Playwright browser."""
        if self.browser is None and self.context is None:

            from playwright.async_api import async_playwright

            self.playwright = async_playwright()
            playwright = await self.playwright.start()

            if self.enable_extensions and self._extensions_dir:
                # Use persistent context for extensions
                user_data_dir = PLAYWRIGHT_DIR / "user-data"
                user_data_dir.mkdir(parents=True, exist_ok=True)

                launch_kwargs = {
                    "headless": False,  # Extensions require headed mode
                    "channel": "chromium",  # Required for extensions
                    "args": [
                        "--disable-dev-shm-usage",
                        "--disable-web-security",
                        "--allow-running-insecure-content",
                        f"--disable-extensions-except={self._extensions_dir}",
                        f"--load-extension={self._extensions_dir}",
                    ],
                }

                # Get custom Chromium executable path if available
                executable_path = get_chromium_executable_path()
                if executable_path:
                    launch_kwargs["executable_path"] = executable_path
                    print(
                        f"[PYTHON] Using custom Chromium executable: {executable_path}"
                    )

                self.context = await playwright.chromium.launch_persistent_context(
                    str(user_data_dir), **launch_kwargs
                )

                print(
                    "[PYTHON] Started Playwright browser with C++ DevTools Support extension"
                )

            else:
                # Regular browser launch without extensions
                executable_path = get_chromium_executable_path()
                launch_kwargs = {
                    "headless": self.headless,
                    "args": [
                        "--disable-dev-shm-usage",
                        "--disable-web-security",
                        "--allow-running-insecure-content",
                    ],
                }

                if executable_path:
                    launch_kwargs["executable_path"] = executable_path
                    print(
                        f"[PYTHON] Using custom Chromium executable: {executable_path}"
                    )

                self.browser = await playwright.chromium.launch(**launch_kwargs)

        if self.page is None:
            if self.context:
                # Using persistent context (with extensions)
                if len(self.context.pages) > 0:
                    self.page = self.context.pages[0]
                else:
                    self.page = await self.context.new_page()
            elif self.browser:
                # Using regular browser
                # Detect system device scale factor for natural browser behavior
                device_scale_factor = self._detect_device_scale_factor()

                # Create browser context with detected or default device scale factor
                if device_scale_factor:
                    context = await self.browser.new_context(
                        device_scale_factor=device_scale_factor
                    )
                    print(
                        f"[PYTHON] Created browser context with device scale factor: {device_scale_factor:.2f}"
                    )
                else:
                    context = await self.browser.new_context()
                    print(
                        "[PYTHON] Created browser context with default device scale factor"
                    )

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

            # Verify device scale factor is working correctly
            try:
                device_pixel_ratio = await self.page.evaluate("window.devicePixelRatio")
                print(
                    f"[PYTHON] Verified browser device pixel ratio: {device_pixel_ratio}"
                )
            except Exception as e:
                print(f"[PYTHON] Could not verify device pixel ratio: {e}")

            # Set up auto-resizing functionality if enabled
            if self.auto_resize:
                await self._setup_auto_resize()

            # Check if C++ DevTools extension is loaded
            if self.enable_extensions and self._extensions_dir:
                try:
                    # Check if the extension is available in the DevTools
                    extensions_available = await self.page.evaluate(
                        """
                        () => {
                            // Check if chrome.devtools is available (extension context)
                            return typeof chrome !== 'undefined' && 
                                   typeof chrome.runtime !== 'undefined' &&
                                   chrome.runtime.id !== undefined;
                        }
                    """
                    )

                    if extensions_available:
                        print(
                            "[PYTHON] ✅ C++ DevTools Support extension is active and ready for DWARF debugging"
                        )
                    else:
                        print(
                            "[PYTHON] ⚠️  C++ DevTools Support extension may not be fully loaded"
                        )

                except Exception as e:
                    print(f"[PYTHON] Could not verify extension status: {e}")

    async def _setup_auto_resize(self) -> None:
        """Set up automatic window resizing based on content size."""
        if self.page is None:
            print("[PYTHON] Cannot setup auto-resize: page is None")
            return

        print(
            "[PYTHON] Setting up browser window tracking with viewport-only adjustment"
        )

        # Create resize tracker instance
        self.resize_tracker = ResizeTracker(self.page)

        # Start polling loop that tracks browser window changes and adjusts viewport only
        asyncio.create_task(self._track_browser_adjust_viewport())

    async def _track_browser_adjust_viewport(self) -> None:
        """Track browser window changes and adjust viewport accordingly.

        This method polls for changes in the browser window size using the
        ResizeTracker and handles any errors that occur.
        """
        if self.resize_tracker is None:
            print("[PYTHON] Cannot start tracking: resize_tracker is None")
            return

        while not self._should_exit.is_set():
            try:
                # Wait between polls
                await asyncio.sleep(0.25)  # Poll every 250ms

                # Check if page is still alive
                if self.page is None or self.page.is_closed():
                    print("[PYTHON] Page closed, signaling exit")
                    self._should_exit.set()
                    return

                # Update resize tracking
                result = await self.resize_tracker.update()

                if result is not None:
                    assert isinstance(result, Exception)
                    # An exception occurred in resize tracking
                    error_message = str(result)
                    warnings.warn(f"[PYTHON] Error in resize tracking: {error_message}")

                    # Be EXTREMELY conservative about browser close detection
                    # Only trigger shutdown on very specific errors that definitively indicate browser closure
                    browser_definitely_closed = any(
                        phrase in error_message.lower()
                        for phrase in [
                            "browser has been closed",
                            "target closed",
                            "connection closed",
                            "target page, probably because the page has been closed",
                            "page has been closed",
                            "browser context has been closed",
                        ]
                    )

                    # Also check actual browser state before deciding to shut down
                    browser_state_indicates_closed = False
                    try:
                        if self.browser and hasattr(self.browser, "is_closed"):
                            browser_state_indicates_closed = self.browser.is_closed()
                        elif self.context and hasattr(self.context, "closed"):
                            browser_state_indicates_closed = self.context.closed
                    except Exception:
                        # If we can't check the state, don't assume it's closed
                        warnings.warn(
                            f"[PYTHON] Could not check browser state: {result}. Assuming browser is not closed."
                        )
                        browser_state_indicates_closed = False

                    if browser_definitely_closed or browser_state_indicates_closed:
                        if browser_definitely_closed:
                            print(
                                f'[PYTHON] Browser has been closed because "{error_message}" matched one of the error phrases or browser state indicates closed, shutting down gracefully...'
                            )
                        elif browser_state_indicates_closed:
                            print(
                                "[PYTHON] Browser state indicates closed, shutting down gracefully..."
                            )
                        self._should_exit.set()
                        return
                    else:
                        # For other errors, just log and continue - don't shut down
                        print(
                            f"[PYTHON] Recoverable error in resize tracking: {result}"
                        )
                        # Add a small delay to prevent tight error loops
                        await asyncio.sleep(1.0)

            except Exception as e:
                error_message = str(e)
                warnings.warn(
                    f"[PYTHON] Unexpected error in browser tracking loop: {error_message}"
                )
                # Add a small delay to prevent tight error loops
                await asyncio.sleep(1.0)

        warnings.warn("[PYTHON] Browser tracking loop exited.")

    async def wait_for_close(self) -> None:
        """Wait for the browser to be closed."""
        if self.context:
            # Wait for persistent context to be closed
            try:
                while not self.context.closed:
                    await asyncio.sleep(1)
            except Exception:
                pass
        elif self.browser:
            try:
                # Wait for the browser to be closed
                while not self.browser.is_closed():
                    await asyncio.sleep(1)
            except Exception:
                pass

    async def close(self) -> None:
        """Close the Playwright browser."""
        # Signal all tracking loops to exit
        self._should_exit.set()

        try:
            # Clean up resize tracker
            self.resize_tracker = None

            if self.page:
                await self.page.close()
                self.page = None

            if self.context:
                await self.context.close()
                self.context = None

            if self.browser:
                await self.browser.close()
                self.browser = None

            if self.playwright:
                # The playwright context manager may not have a stop() method in all versions
                # Try stop() first, fall back to __aexit__ if needed
                try:
                    if hasattr(self.playwright, "stop"):
                        await self.playwright.stop()
                    else:
                        # For async context managers, use __aexit__
                        await self.playwright.__aexit__(None, None, None)
                except Exception as stop_error:
                    print(
                        f"[PYTHON] Warning: Could not properly stop playwright: {stop_error}"
                    )
                    # Try alternative cleanup methods
                    try:
                        if hasattr(self.playwright, "__aexit__"):
                            await self.playwright.__aexit__(None, None, None)
                    except Exception:
                        pass  # Ignore secondary cleanup failures
                finally:
                    self.playwright = None
        except KeyboardInterrupt:
            print("[PYTHON] Keyboard interrupt detected, closing Playwright browser")
            self._should_exit.set()
            import _thread

            _thread.interrupt_main()
        except Exception as e:
            print(f"[PYTHON] Error closing Playwright browser: {e}")
            self._should_exit.set()


def run_playwright_browser(
    url: str, headless: bool = False, enable_extensions: bool = True
) -> None:
    """Run Playwright browser in a separate process.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        enable_extensions: Whether to enable Chrome extensions (C++ DevTools Support)
    """

    async def main():
        browser = None
        try:
            browser = PlaywrightBrowser(
                headless=headless, enable_extensions=enable_extensions
            )
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
                    browser = PlaywrightBrowser(
                        headless=headless, enable_extensions=enable_extensions
                    )
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

    def open(
        self, url: str, headless: bool = False, enable_extensions: bool = True
    ) -> None:
        """Open URL with Playwright browser and keep it alive.

        Args:
            url: The URL to open
            headless: Whether to run in headless mode
            enable_extensions: Whether to enable Chrome extensions (C++ DevTools Support)
        """

        try:
            # Run Playwright in a separate process to avoid blocking
            import multiprocessing

            self.process = multiprocessing.Process(
                target=run_playwright_browser_persistent,
                args=(url, headless, enable_extensions),
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

            except KeyboardInterrupt:
                print("[MAIN] Browser monitor interrupted by user")
                import _thread

                _thread.interrupt_main()
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


def run_playwright_browser_persistent(
    url: str, headless: bool = False, enable_extensions: bool = True
) -> None:
    """Run Playwright browser in a persistent mode that stays alive until terminated.

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        enable_extensions: Whether to enable Chrome extensions (C++ DevTools Support)
    """

    async def main():
        browser = None
        try:
            browser = PlaywrightBrowser(
                headless=headless, enable_extensions=enable_extensions
            )
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
                    browser = PlaywrightBrowser(
                        headless=headless, enable_extensions=enable_extensions
                    )
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


def open_with_playwright(
    url: str, headless: bool = False, enable_extensions: bool = True
) -> PlaywrightBrowserProxy:
    """Open URL with Playwright browser and return a proxy object for lifecycle management.

    This function can be used as a drop-in replacement for webbrowser.open().

    Args:
        url: The URL to open
        headless: Whether to run in headless mode
        enable_extensions: Whether to enable Chrome extensions (C++ DevTools Support)

    Returns:
        PlaywrightBrowserProxy object for managing the browser lifecycle
    """
    proxy = PlaywrightBrowserProxy()
    proxy.open(url, headless, enable_extensions)
    return proxy


def install_playwright_browsers() -> bool:
    """Install Playwright browsers if not already installed.

    Installs browsers to ~/.fastled/playwright directory.

    Returns:
        True if installation was successful or browsers were already installed
    """

    try:
        import os
        from pathlib import Path

        # Set custom browser installation path
        playwright_dir = Path.home() / ".fastled" / "playwright"
        playwright_dir.mkdir(parents=True, exist_ok=True)

        # Check if browsers are installed in the custom directory
        import glob
        import platform

        if platform.system() == "Windows":
            chromium_pattern = str(
                playwright_dir / "chromium-*" / "chrome-win" / "chrome.exe"
            )
        elif platform.system() == "Darwin":  # macOS
            chromium_pattern = str(
                playwright_dir / "chromium-*" / "chrome-mac" / "Chromium.app"
            )
        else:  # Linux
            chromium_pattern = str(
                playwright_dir / "chromium-*" / "chrome-linux" / "chrome"
            )

        if glob.glob(chromium_pattern):
            print(f"✅ Playwright browsers already installed at: {playwright_dir}")
            return True

        # If we get here, browsers need to be installed
        print("Installing Playwright browsers...")
        print(f"Installing to: {playwright_dir}")
        import subprocess

        env = dict(os.environ)
        env["PLAYWRIGHT_BROWSERS_PATH"] = str(playwright_dir.resolve())

        result = subprocess.run(
            [sys.executable, "-m", "playwright", "install", "chromium"],
            env=env,
        )

        if result.returncode == 0:
            print("✅ Playwright browsers installed successfully!")
            print(f"   Location: {playwright_dir}")

            # Also download the C++ DevTools Support extension
            try:
                from fastled.playwright.chrome_extension_downloader import (
                    download_cpp_devtools_extension,
                )

                extension_path = download_cpp_devtools_extension()
                if extension_path:
                    print(
                        "✅ C++ DevTools Support extension ready for DWARF debugging!"
                    )
                else:
                    print("⚠️  C++ DevTools Support extension download failed")
            except Exception as e:
                print(f"⚠️  Failed to setup C++ DevTools Support extension: {e}")

            return True
        else:
            print(
                f"❌ Failed to install Playwright browsers (exit code: {result.returncode})"
            )
            return False

    except Exception as e:
        print(f"❌ Error installing Playwright browsers: {e}")
        return False
