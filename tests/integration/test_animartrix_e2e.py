"""
End-to-end test for the Animartrix example.

This test:
1. Compiles ~/dev/fastled/examples/Animartrix using native EMSDK
2. Starts a server serving the compiled output
3. Opens the page in a headless Playwright browser
4. Verifies the page loads with no JavaScript errors
"""

import asyncio
import os
import random
import subprocess
import time
import unittest
from pathlib import Path

import pytest  # type: ignore[reportMissingImports]
from playwright.async_api import async_playwright

from fastled import Test

ANIMARTRIX_DIR = Path.home() / "dev" / "fastled" / "examples" / "Animartrix"
FASTLED_JS_DIR = ANIMARTRIX_DIR / "fastled_js"

# Use a random port to avoid conflicts with other tests
TEST_PORT = random.randint(9200, 9400)


class AnimartrixE2ETest(unittest.TestCase):
    """End-to-end test: compile Animartrix, serve it, open in browser, check for errors."""

    @pytest.mark.timeout(600)
    def test_animartrix_compiles_and_runs_in_browser(self) -> None:
        """Compile Animartrix, serve it, and verify no JS errors in the browser."""
        if not ANIMARTRIX_DIR.exists():
            self.skipTest(f"Animartrix example not found at {ANIMARTRIX_DIR}")

        # Step 1: Compile the sketch
        original_dir = os.getcwd()
        try:
            os.chdir(str(ANIMARTRIX_DIR))
            cp = subprocess.run(
                "fastled --just-compile",
                shell=True,
                capture_output=True,
                check=False,
                timeout=300,
            )
            if cp.returncode != 0:
                stdout = cp.stdout.decode("utf-8", errors="replace")
                stderr = cp.stderr.decode("utf-8", errors="replace")
                self.fail(
                    f"Compilation failed (rc={cp.returncode}):\n"
                    f"stdout:\n{stdout}\nstderr:\n{stderr}"
                )
        finally:
            os.chdir(original_dir)

        # Verify output exists
        self.assertTrue(
            FASTLED_JS_DIR.exists(), f"Expected output dir {FASTLED_JS_DIR} to exist"
        )
        self.assertTrue(
            (FASTLED_JS_DIR / "index.html").exists(), "Expected index.html in output"
        )
        self.assertTrue(
            (FASTLED_JS_DIR / "fastled.wasm").exists(),
            "Expected fastled.wasm in output",
        )

        # Step 2: Serve and test in browser
        asyncio.run(self._verify_browser_no_errors())

    async def _verify_browser_no_errors(self) -> None:
        """Start server, open page in Playwright, and check for JS errors."""
        proc = Test.spawn_http_server(
            FASTLED_JS_DIR,
            open_browser=False,
            enable_https=False,
        )

        try:
            time.sleep(2)
            # The Rust server auto-assigns a port; use localhost default
            server_url = "http://localhost:8089"

            async with async_playwright() as p:
                browser = await p.chromium.launch(headless=True)

                try:
                    context = await browser.new_context()
                    page = await context.new_page()

                    # Collect console errors
                    errors: list[str] = []
                    page.on(
                        "console",
                        lambda msg: (
                            errors.append(msg.text) if msg.type == "error" else None
                        ),
                    )
                    page.on(
                        "pageerror",
                        lambda exc: errors.append(str(exc)),
                    )

                    # Navigate to the page
                    response = await page.goto(
                        server_url,
                        wait_until="domcontentloaded",
                        timeout=30000,
                    )

                    self.assertIsNotNone(response, "Failed to load page")
                    assert response is not None
                    self.assertEqual(
                        response.status,
                        200,
                        f"Expected 200, got {response.status}",
                    )

                    # Wait for WASM to load and run briefly
                    await page.wait_for_timeout(5000)

                    # Check for errors
                    if errors:
                        self.fail(
                            "Browser console errors detected:\n"
                            + "\n".join(f"  - {e}" for e in errors)
                        )

                    print(f"Animartrix e2e test passed: {server_url}")

                finally:
                    await browser.close()

        finally:
            proc.terminate()
            time.sleep(1)


if __name__ == "__main__":
    unittest.main()
