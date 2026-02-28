"""
Test header dump functionality for EMSDK headers export.
"""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Test
from fastled.compile_server import CompileServer
from fastled.header_dump import dump_emsdk_headers


def _enabled() -> bool:
    """Check if this system can run the tests."""
    return Test.can_run_local_docker_tests()


class HeaderDumpTester(unittest.TestCase):
    """Test header dump functionality."""

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_compile_server_header_dump(self) -> None:
        """Test that EMSDK headers can be dumped from a live CompileServer."""

        server = CompileServer(auto_start=True, port=0)
        try:
            with TemporaryDirectory() as tmpdir:
                output_path = Path(tmpdir) / "emsdk_headers.zip"

                # Test the server's get_emsdk_headers method directly
                server.get_emsdk_headers(output_path)

                # Verify the ZIP file was created
                self.assertTrue(
                    output_path.exists(), "EMSDK headers ZIP file was not created"
                )

                # Verify it's not empty
                self.assertGreater(
                    output_path.stat().st_size, 0, "EMSDK headers ZIP file is empty"
                )

                # Verify it's a valid ZIP file by checking the magic header
                with open(output_path, "rb") as f:
                    header = f.read(4)
                    self.assertEqual(
                        header[:2], b"PK", "File does not appear to be a valid ZIP file"
                    )
        finally:
            # Stop the server
            server.stop()

            # Verify server stopped
            self.assertFalse(server.running, "Server did not stop")

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_dump_with_server_url(self) -> None:
        """Test that EMSDK headers can be dumped using a specific server URL."""

        server = CompileServer(auto_start=True, port=0)
        try:
            with TemporaryDirectory() as tmpdir:
                output_path = Path(tmpdir) / "emsdk_headers_url.zip"

                # Use dump_emsdk_headers with the server's URL
                server_url = server.url()
                dump_emsdk_headers(output_path, server_url=server_url)

                # Verify the ZIP file was created
                self.assertTrue(
                    output_path.exists(), "EMSDK headers ZIP file was not created"
                )

                # Verify it's not empty
                self.assertGreater(
                    output_path.stat().st_size, 0, "EMSDK headers ZIP file is empty"
                )
        finally:
            # Stop the server
            server.stop()

            # Verify server stopped
            self.assertFalse(server.running, "Server did not stop")

    def test_filepath_validation(self) -> None:
        """Test that filepath validation works correctly."""

        with TemporaryDirectory() as tmpdir:
            invalid_path = Path(tmpdir) / "headers.txt"  # Wrong extension

            # Should raise ValueError for non-.zip files
            with self.assertRaises(ValueError) as context:
                dump_emsdk_headers(invalid_path, server_url="http://example.com")

            self.assertIn("must end with .zip", str(context.exception))

    @unittest.skipUnless(_enabled(), "Skipping test on non-Linux system on github")
    def test_dump_with_auto_fallback(self) -> None:
        """Test dump_emsdk_headers with None server_url (auto local server creation)."""

        with TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "emsdk_headers_auto.zip"

            # Test with None server_url - should create local server automatically
            dump_emsdk_headers(output_path, server_url=None)

            # Verify the ZIP file was created
            self.assertTrue(
                output_path.exists(), "EMSDK headers ZIP file was not created"
            )

            # Verify it's not empty
            self.assertGreater(
                output_path.stat().st_size, 0, "EMSDK headers ZIP file is empty"
            )


if __name__ == "__main__":
    unittest.main()
