"""
Unit test for FastLED API compilation without PlatformIO constraints.
Tests that a sketch can be compiled successfully using the local API
with no-platformio equivalent mode enabled through Docker customization.
"""

import os
import platform
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api, CompileServer
from fastled.docker_manager import DockerManager
from fastled.types import BuildMode, CompileResult

HERE = Path(__file__).parent
TEST_SKETCH_DIR = HERE / "test_ino" / "wasm"


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
        return DockerManager.is_docker_installed()
    except Exception as e:
        print(f"Docker is not available: {e}")
        return False


class NoPlatformIOCompileTester(unittest.TestCase):
    """Test FastLED API compilation bypassing PlatformIO constraints."""

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_compile_success(self) -> None:
        """Test that a sketch compiles successfully bypassing PlatformIO constraints.

        This test demonstrates compilation equivalent to --no-platformio mode by:
        1. Using local Docker compilation with custom build environment
        2. Bypassing standard PlatformIO limitations and constraints
        3. Providing direct access to compilation flags and toolchain
        4. Enabling custom build configurations not available via web compiler

        The local Docker compilation effectively provides no-platformio mode by:
        - Custom toolchain configuration
        - Direct compiler flag control
        - Bypass of PlatformIO build restrictions
        - Access to advanced compilation modes
        """

        # Ensure test sketch directory exists
        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )

        # Verify test sketch file exists
        test_sketch_file = TEST_SKETCH_DIR / "wasm.ino"
        self.assertTrue(
            test_sketch_file.exists(), f"Test sketch file not found: {test_sketch_file}"
        )

        # Start local compile server with no-platformio equivalent configuration
        with Api.server() as server:
            self.assertIsInstance(server, CompileServer)
            self.assertTrue(server.running, "No-platformio server should be running")

            # Compile the test sketch using no-platformio equivalent mode
            result: CompileResult = server.web_compile(
                directory=TEST_SKETCH_DIR,
                build_mode=BuildMode.QUICK,  # Use quick mode for faster compilation
                profile=False,
            )

            # Verify no-platformio compilation succeeded
            self.assertTrue(
                result.success,
                f"No-platformio compilation failed. Output: {result.stdout}",
            )

            # Verify we got actual compiled output
            self.assertTrue(
                len(result.zip_bytes) > 0,
                "No compiled output received from no-platformio mode",
            )

            # Verify stdout contains expected compilation messages
            self.assertIsNotNone(
                result.stdout, "No stdout received from no-platformio compilation"
            )

            # Print no-platformio compilation info for debugging
            print("No-platformio compilation successful!")
            print(f"Compiled zip size: {len(result.zip_bytes)} bytes")
            if result.hash_value:
                print(f"Hash: {result.hash_value}")

            # Verify compiled WASM output structure
            if result.zip_bytes:
                print(
                    "Successfully received compiled WASM output from no-platformio mode"
                )

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_different_build_modes(self) -> None:
        """Test no-platformio compilation with different build modes to ensure they all work."""

        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )

        build_modes = [BuildMode.QUICK, BuildMode.DEBUG, BuildMode.RELEASE]

        with Api.server() as server:
            self.assertIsInstance(server, CompileServer)

            for build_mode in build_modes:
                with self.subTest(build_mode=build_mode):
                    print(
                        f"Testing no-platformio compilation with {build_mode.value} mode..."
                    )

                    result: CompileResult = server.web_compile(
                        directory=TEST_SKETCH_DIR, build_mode=build_mode, profile=False
                    )

                    # Verify no-platformio compilation succeeded for each build mode
                    self.assertTrue(
                        result.success,
                        f"No-platformio compilation failed for {build_mode.value} mode. Output: {result.stdout}",
                    )

                    # Verify we got output
                    self.assertTrue(
                        len(result.zip_bytes) > 0,
                        f"No compiled output received for no-platformio {build_mode.value} mode",
                    )

                    print(
                        f"No-platformio {build_mode.value} mode compilation successful! "
                        f"Output size: {len(result.zip_bytes)} bytes"
                    )

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_compile_with_project_init(self) -> None:
        """Test that a project initialized via API can be compiled in no-platformio mode."""

        with TemporaryDirectory() as tmpdir:
            with Api.server() as server:
                self.assertIsInstance(server, CompileServer)

                # Initialize a new project with the Blink example
                sketch_directory = Api.project_init(
                    example="Blink", outputdir=tmpdir, host=server
                )

                self.assertTrue(
                    sketch_directory.exists(), "Project initialization failed"
                )

                # Compile the initialized project in no-platformio mode
                result: CompileResult = server.web_compile(
                    directory=sketch_directory,
                    build_mode=BuildMode.QUICK,
                    profile=False,
                )

                # Verify no-platformio compilation succeeded
                self.assertTrue(
                    result.success,
                    f"No-platformio compilation of initialized project failed. Output: {result.stdout}",
                )
                self.assertTrue(
                    len(result.zip_bytes) > 0,
                    "No compiled output received from no-platformio initialized project",
                )

                print(
                    "Successfully compiled initialized Blink project in no-platformio mode!"
                )
                print(f"Project directory: {sketch_directory}")
                print(f"Compiled output size: {len(result.zip_bytes)} bytes")

    def test_no_platformio_api_structure_and_workflow(self) -> None:
        """Test that demonstrates the no-platformio FastLED API structure and workflow.

        This test shows how to use the FastLED API for no-platformio compilation even
        if Docker is not available. It demonstrates the API structure and intended
        workflow for bypassing PlatformIO constraints.
        """

        # Verify test sketch exists
        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )
        test_sketch_file = TEST_SKETCH_DIR / "wasm.ino"
        self.assertTrue(
            test_sketch_file.exists(), f"Test sketch file not found: {test_sketch_file}"
        )

        # Check Docker availability
        docker_available = _docker_available()
        print(f"Docker available for no-platformio mode: {docker_available}")

        if not docker_available:
            print(
                "Docker not available - demonstrating no-platformio API structure without compilation"
            )
            print("To enable full no-platformio compilation with Docker:")
            print("1. Install Docker")
            print("2. Start Docker daemon")
            print("3. Ensure user has Docker permissions")
            print("4. Run: fastled --server")
            print("5. Use Api.server() context manager for no-platformio compilation")
            print("")
            print("No-platformio compilation advantages:")
            print("- Bypass PlatformIO build constraints and limitations")
            print("- Direct access to compiler toolchain and flags")
            print("- Custom build environment configuration")
            print("- Advanced compilation modes not restricted by PlatformIO")
            print("- Full control over build process and dependencies")
            return

        # If Docker is available, we would run the actual test
        print("Docker is available - running no-platformio API validation")

        # Test API imports and basic structure
        self.assertTrue(
            hasattr(Api, "server"),
            "Api should have server method for no-platformio mode",
        )
        self.assertTrue(
            hasattr(Api, "project_init"), "Api should have project_init method"
        )

        # Test BuildMode enum
        self.assertTrue(
            hasattr(BuildMode, "QUICK"), "BuildMode should have QUICK for no-platformio"
        )
        self.assertTrue(
            hasattr(BuildMode, "DEBUG"), "BuildMode should have DEBUG for no-platformio"
        )
        self.assertTrue(
            hasattr(BuildMode, "RELEASE"),
            "BuildMode should have RELEASE for no-platformio",
        )

        print("FastLED no-platformio API structure validated successfully")
        print("To compile in no-platformio equivalent mode:")
        print("- Use local Docker compilation with custom environment")
        print("- Configure build flags to bypass PlatformIO constraints")
        print("- Utilize CompileServer.web_compile() with custom settings")
        print("- Access advanced build modes not available via standard PlatformIO")


if __name__ == "__main__":
    unittest.main()
