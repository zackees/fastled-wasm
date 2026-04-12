"""
Smoke test for the Rust HTTP server spawned via the fastled CLI.

Replaces the deleted HTTPS integration tests (PR #48 dropped HTTPS support).
This test verifies that the server can serve static files over HTTP.
"""

import re
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

import httpx
import pytest  # type: ignore[reportMissingImports]

from fastled.interrupts import handle_keyboard_interrupt


def _find_rust_cli() -> Path | None:
    """Locate the *Rust* fastled CLI binary (not the Python shim).

    The Rust binary handles ``--serve-dir`` natively.  To distinguish it
    from the Python entry-point shim (which is also called ``fastled.exe``),
    we run a quick ``--serve-dir`` probe on any candidate before accepting it.
    """
    exe_name = "fastled.exe" if sys.platform == "win32" else "fastled"

    candidates: list[Path] = []

    # 1. Walk up from this test file looking for a Cargo workspace root.
    current = Path(__file__).resolve().parent
    for _ in range(10):
        if (current / "Cargo.toml").is_file():
            # Prefer target/debug and target/release (direct cargo build output)
            # over target/<triple>/debug which may contain maturin wrappers.
            for profile in ("debug", "release"):
                candidate = current / "target" / profile / exe_name
                if candidate.is_file():
                    candidates.append(candidate)
            # Then check platform-specific target dirs as fallback.
            target_dir = current / "target"
            if target_dir.is_dir():
                for arch_dir in target_dir.iterdir():
                    if arch_dir.is_dir() and not arch_dir.name.startswith("."):
                        for profile in ("debug", "release"):
                            c = arch_dir / profile / exe_name
                            if c.is_file():
                                candidates.append(c)
            break
        parent = current.parent
        if parent == current:
            break
        current = parent

    # 2. Sibling of the running interpreter.
    interpreter_sibling = Path(sys.executable).resolve().parent / exe_name
    if interpreter_sibling.is_file():
        candidates.append(interpreter_sibling)

    # Prefer larger binaries (the Rust debug build is ~100 MB; the maturin
    # wrapper is ~3 MB; the Python shim is ~40 KB).
    candidates.sort(key=lambda p: p.stat().st_size, reverse=True)

    for candidate in candidates:
        # The true Rust CLI debug build is 50+ MB.  Maturin wrappers and
        # Python shims are much smaller.
        if candidate.stat().st_size < 10_000_000:
            continue
        # Quick probe: the Rust binary help starts with "FastLED WASM".
        try:
            cp = subprocess.run(
                [str(candidate), "--help"],
                capture_output=True,
                timeout=5,
            )
            help_text = cp.stdout.decode("utf-8", errors="replace")
            if "FastLED WASM" in help_text and "--serve-dir" in help_text:
                return candidate
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
        except Exception:
            continue

    return None


_RUST_CLI = _find_rust_cli()


@pytest.mark.skipif(
    _RUST_CLI is None,
    reason="Rust fastled CLI binary not found",
)
class HttpServerSmokeTest(unittest.TestCase):
    """Basic smoke test: spawn the Rust HTTP server, fetch index.html, verify 200."""

    @pytest.mark.timeout(30)
    def test_serve_index_html(self) -> None:
        """Spawn the HTTP server in a temp directory and fetch index.html."""
        assert _RUST_CLI is not None  # guarded by skipif
        with tempfile.TemporaryDirectory() as tmpdir:
            index_path = Path(tmpdir) / "index.html"
            index_path.write_text(
                "<html><body><h1>FastLED HTTP smoke test</h1></body></html>",
                encoding="utf-8",
            )

            proc = subprocess.Popen(
                [str(_RUST_CLI), "--serve-dir", str(tmpdir)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

            try:
                # Parse the port the server chose from its stdout.
                actual_port: int | None = None
                assert proc.stdout is not None
                for _ in range(50):
                    raw = proc.stdout.readline()
                    if not raw:
                        break
                    line = raw.decode("utf-8", errors="replace")
                    m = re.search(r"http://[\d.]+:(\d+)", line)
                    if m:
                        actual_port = int(m.group(1))
                        break

                self.assertIsNotNone(
                    actual_port, "Failed to detect the server port from stdout"
                )
                assert actual_port is not None  # narrow type for pyright

                # Wait for the server to become responsive.
                deadline = time.time() + 10
                resp: httpx.Response | None = None
                while time.time() < deadline:
                    try:
                        resp = httpx.get(
                            f"http://localhost:{actual_port}/index.html", timeout=2
                        )
                        if resp.status_code == 200:
                            break
                    except httpx.HTTPError:
                        resp = None
                    time.sleep(0.2)

                self.assertIsNotNone(resp, "Server did not respond in time")
                assert resp is not None  # narrow type for pyright
                self.assertEqual(
                    resp.status_code, 200, f"Expected 200, got {resp.status_code}"
                )
                self.assertIn("FastLED HTTP smoke test", resp.text)
            finally:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except KeyboardInterrupt as ki:
                    handle_keyboard_interrupt(ki)
                except OSError:
                    proc.kill()


if __name__ == "__main__":
    unittest.main()
