"""Refs #268: the frontend page must fit the viewer window.

Two CSS bugs made the shipped viewer look wrong at common window sizes:
`#main-container` was content-box with `width: 100%` plus 20 px padding, so the
page was always 40 px wider than the window and the controls column ran off the
right edge; and the page title was a fixed 96 px at every viewport.

Both reproduce from the real `index.html` and `index.css` alone, so this needs
no WASM compile and no running sketch.
"""

import functools
import http.server
import os
import subprocess
import threading
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parents[2]
FRONTEND = ROOT / "src/fastled/frontend"


@pytest.fixture(scope="module")
def static_page(tmp_path_factory: Any) -> Any:
    esbuild = os.environ.get("FASTLED_ESBUILD")
    if not esbuild:
        if os.environ.get("CI"):
            pytest.fail("CI requires FASTLED_ESBUILD")
        pytest.skip("set FASTLED_ESBUILD")
    root = tmp_path_factory.mktemp("layout-268")
    subprocess.run(
        [
            esbuild,
            str(FRONTEND / "index.css"),
            "--bundle",
            "--target=es2021",
            "--external:./assets/*",
            f"--outfile={root}/index.css",
        ],
        check=True,
    )
    html = (FRONTEND / "index.html").read_text(encoding="utf-8")
    # Layout only: the app module would need a compiled sketch.
    html = html.replace('<script type="module" src="./app.ts"></script>', "")
    (root / "index.html").write_text(html, encoding="utf-8")
    # Served over HTTP rather than file:// so a remote WebKit (a Playwright
    # server in a container, on hosts that cannot install it) can load it.
    handler = functools.partial(
        http.server.SimpleHTTPRequestHandler, directory=str(root)
    )
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}/index.html"
    finally:
        server.shutdown()


def _require(module: str) -> Any:
    import importlib

    if os.environ.get("CI"):
        return importlib.import_module(module)
    return pytest.importorskip(module)


def _launch(manager: Any, browser_name: str) -> Any:
    endpoint = os.environ.get("FASTLED_PLAYWRIGHT_WEBKIT_ENDPOINT")
    if browser_name == "webkit" and endpoint:
        return manager.webkit.connect(endpoint)
    return getattr(manager, browser_name).launch()


MEASURE = """() => {
    const container = document.getElementById('main-container').getBoundingClientRect();
    return {
        innerWidth: innerWidth,
        scrollWidth: document.documentElement.scrollWidth,
        containerRight: container.right,
        titlePx: parseFloat(getComputedStyle(document.querySelector('h1')).fontSize),
    };
}"""


@pytest.mark.parametrize("browser_name", ["chromium", "webkit"])
def test_layout_268_page_fits_a_viewer_sized_window(
    static_page: str, browser_name: str
) -> None:
    """RED: the page was 40 px wider than the window, clipping the controls."""
    playwright = _require("playwright.sync_api")
    with playwright.sync_playwright() as manager:
        browser = _launch(manager, browser_name)
        # 989 CSS px is the viewer window the bug was reported from (a ~1456 px
        # window at ~1.47x scaling).
        page = browser.new_page(viewport={"width": 989, "height": 700})
        page.goto(static_page)
        narrow = page.evaluate(MEASURE)
        # 5vw reaches the 6em cap at 1920 px, so measure past it.
        page.set_viewport_size({"width": 2200, "height": 1000})
        wide = page.evaluate(MEASURE)
        browser.close()

    assert (
        narrow["scrollWidth"] <= narrow["innerWidth"]
    ), f"page overflows by {narrow['scrollWidth'] - narrow['innerWidth']}px"
    assert narrow["containerRight"] <= narrow["innerWidth"], narrow
    # The title scales down in a viewer-sized window...
    assert narrow["titlePx"] < 96, f"title stayed {narrow['titlePx']}px"
    # ...and keeps its 6em size on a wide screen.
    assert wide["titlePx"] == 96, f"wide-screen title changed to {wide['titlePx']}px"
