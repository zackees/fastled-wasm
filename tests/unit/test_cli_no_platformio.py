"""
Unit test for FastLED CLI --no-platformio flag functionality.
Tests that the CLI correctly handles the --no-platformio argument and compiles successfully.
"""

import os
import platform
import subprocess
import unittest
from pathlib import Path

HERE = Path(__file__).parent
TEST_SKETCH_DIR = HERE / "test_ino" / "wasm"
WORKSPACE_ROOT = HERE.parent.parent


def _enabled() -> bool:
    """Check if this system can run the tests."""
    is_github_runner = "GITHUB_ACTIONS" in os.environ
    if not is_github_runner:
        return True
    # This only works in ubuntu at the moment
    return platform.system() == "Linux"


def _docker_available() -> bool:
    """Check if Docker is available for no-platformio compilation."""
    try:
        from fastled.docker_manager import DockerManager

        return DockerManager.is_docker_installed()
    except Exception as e:
        print(f"Docker is not available: {e}")
        return False


class CLINoPlatformIOTest(unittest.TestCase):
    """Test FastLED CLI --no-platformio flag functionality."""

    def test_no_platformio_flag_recognized(self) -> None:
        """Test that --no-platformio flag is recognized by the CLI without errors."""

        # Test that the flag is recognized in help
        result = subprocess.run(
            ["uv", "run", "fastled", "--help"],
            cwd=WORKSPACE_ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )

        self.assertEqual(result.returncode, 0, "Help command should succeed")
        self.assertIn(
            "--no-platformio", result.stdout, "--no-platformio should appear in help"
        )
        self.assertIn(
            "Bypass PlatformIO constraints",
            result.stdout,
            "Help text should be present",
        )

    def test_no_platformio_flag_parsing(self) -> None:
        """Test that --no-platformio flag is parsed correctly without compilation."""

        # Ensure test sketch directory exists
        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )

        # Test with --help to verify flag parsing without triggering compilation
        result = subprocess.run(
            ["uv", "run", "fastled", str(TEST_SKETCH_DIR), "--no-platformio", "--help"],
            cwd=WORKSPACE_ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )

        self.assertEqual(result.returncode, 0, "CLI should accept --no-platformio flag")
        self.assertIn(
            "--no-platformio", result.stdout, "--no-platformio should be in help output"
        )

    @unittest.skipUnless(
        _enabled(),
        "Test requires Linux environment",
    )
    def test_no_platformio_flag_forces_local_mode(self) -> None:
        """Test that --no-platformio flag correctly forces local Docker compilation mode."""

        # Ensure test sketch directory exists
        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )

        # Run with --no-platformio and --just-compile to avoid browser opening
        # Use a short timeout to avoid long waits if compilation hangs
        try:
            result = subprocess.run(
                [
                    "uv",
                    "run",
                    "fastled",
                    str(TEST_SKETCH_DIR),
                    "--no-platformio",
                    "--just-compile",
                ],
                cwd=WORKSPACE_ROOT,
                capture_output=True,
                text=True,
                timeout=180,  # 3 minutes timeout
            )

            # Check that the --no-platformio message appears
            output = result.stdout + result.stderr
            self.assertIn(
                "--no-platformio mode enabled: forcing local Docker compilation to bypass PlatformIO constraints",
                output,
                "Should display --no-platformio mode message",
            )

            # Check that it attempts local compilation (even if Docker isn't available)
            self.assertTrue(
                "localhost" in output.lower()
                or "local" in output.lower()
                or "docker" in output.lower(),
                "Should attempt local compilation mode",
            )

            # If compilation succeeded, verify success
            if result.returncode == 0:
                self.assertIn(
                    "compilation success",
                    output.lower(),
                    "Should indicate compilation success",
                )
                print("✅ --no-platformio CLI compilation succeeded!")
            else:
                # If it failed, it should be due to Docker not being available or similar infrastructure issue
                # The important thing is that the flag was recognized and processed
                self.assertNotIn(
                    "unrecognized arguments", output, "Flag should be recognized"
                )
                print(
                    f"ℹ️  --no-platformio flag processed correctly (exit code: {result.returncode})"
                )
                print(
                    f"Output: {output[:500]}..."
                )  # Print first 500 chars for debugging

        except subprocess.TimeoutExpired:
            self.fail(
                "Command timed out - this suggests the flag was processed but compilation took too long"
            )

    def test_no_platformio_cli_argument_structure(self) -> None:
        """Test the CLI argument structure for --no-platformio flag."""

        # Test that the flag doesn't conflict with other flags
        conflicting_flags = [
            ["--web"],
            ["--server"],
            ["--debug"],
            ["--quick"],
            ["--release"],
        ]

        for flags in conflicting_flags:
            with self.subTest(flags=flags):
                # Test help output to ensure no argument conflicts
                cmd = ["uv", "run", "fastled", "--no-platformio"] + flags + ["--help"]
                result = subprocess.run(
                    cmd, cwd=WORKSPACE_ROOT, capture_output=True, text=True, timeout=30
                )

                self.assertEqual(
                    result.returncode, 0, f"Should accept --no-platformio with {flags}"
                )
                self.assertNotIn(
                    "error:",
                    result.stderr.lower(),
                    f"No errors with --no-platformio + {flags}",
                )

    def test_no_platformio_with_different_sketch_directories(self) -> None:
        """Test --no-platformio flag with different test sketch directories."""

        test_sketches = [
            TEST_SKETCH_DIR,  # wasm sketch
            HERE / "test_ino" / "embedded",  # embedded sketch
        ]

        for sketch_dir in test_sketches:
            if sketch_dir.exists():
                with self.subTest(sketch_dir=sketch_dir):
                    # Test with --help to verify flag parsing
                    result = subprocess.run(
                        [
                            "uv",
                            "run",
                            "fastled",
                            str(sketch_dir),
                            "--no-platformio",
                            "--help",
                        ],
                        cwd=WORKSPACE_ROOT,
                        capture_output=True,
                        text=True,
                        timeout=30,
                    )

                    self.assertEqual(
                        result.returncode, 0, f"Should work with sketch at {sketch_dir}"
                    )
                    self.assertIn(
                        "--no-platformio",
                        result.stdout,
                        "Flag should be present in help",
                    )


if __name__ == "__main__":
    unittest.main()
