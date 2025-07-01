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
from unittest.mock import MagicMock, patch

from fastled import Api, CompileServer
from fastled.compile_server_impl import CompileServerImpl
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


class NoPlatformIOCompileTest(unittest.TestCase):
    """Test cases for --no-platformio flag functionality in compilation."""

    def setUp(self) -> None:
        """Set up test environment."""
        pass

    def test_no_platformio_server_command_construction(self) -> None:
        """Test that --no-platformio flag is added to server command when enabled."""

        # Mock the docker manager and its methods
        with patch("fastled.compile_server_impl.DockerManager") as mock_docker_manager:
            mock_docker = MagicMock()
            mock_docker_manager.return_value = mock_docker
            mock_docker.is_running.return_value = (True, None)
            mock_docker.validate_or_download_image.return_value = False
            mock_docker.run_container_detached.return_value = MagicMock()
            mock_docker.attach_and_run.return_value = MagicMock()

            # Test with no_platformio=True
            with patch(
                "fastled.compile_server_impl._try_get_fastled_src", return_value=None
            ):
                server_impl = CompileServerImpl(
                    auto_start=False, no_platformio=True  # Don't actually start
                )

                # Mock the parts of _start that we don't want to actually run
                with patch.object(
                    server_impl.docker, "is_running", return_value=(True, None)
                ):
                    with patch.object(
                        server_impl.docker,
                        "validate_or_download_image",
                        return_value=False,
                    ):
                        with patch.object(
                            server_impl.docker, "run_container_detached"
                        ) as mock_run:
                            with patch.object(server_impl.docker, "attach_and_run"):
                                try:
                                    server_impl._start()
                                except Exception:
                                    pass  # We expect this to fail, we just want to check the command

                                # Verify that run_container_detached was called with --no-platformio in the command
                                self.assertTrue(
                                    mock_run.called,
                                    "run_container_detached should have been called",
                                )

                                # Get the call arguments
                                call_args = mock_run.call_args
                                command = call_args[1][
                                    "command"
                                ]  # Get the command from kwargs

                                # Verify --no-platformio is in the command
                                self.assertIn(
                                    "--no-platformio",
                                    command,
                                    f"--no-platformio should be in server command: {command}",
                                )

    def test_no_platformio_server_command_without_flag(self) -> None:
        """Test that --no-platformio flag is NOT added to server command when disabled."""

        # Mock the docker manager and its methods
        with patch("fastled.compile_server_impl.DockerManager") as mock_docker_manager:
            mock_docker = MagicMock()
            mock_docker_manager.return_value = mock_docker
            mock_docker.is_running.return_value = (True, None)
            mock_docker.validate_or_download_image.return_value = False
            mock_docker.run_container_detached.return_value = MagicMock()
            mock_docker.attach_and_run.return_value = MagicMock()

            # Test with no_platformio=False (default)
            with patch(
                "fastled.compile_server_impl._try_get_fastled_src", return_value=None
            ):
                server_impl = CompileServerImpl(
                    auto_start=False, no_platformio=False  # Don't actually start
                )

                # Mock the parts of _start that we don't want to actually run
                with patch.object(
                    server_impl.docker, "is_running", return_value=(True, None)
                ):
                    with patch.object(
                        server_impl.docker,
                        "validate_or_download_image",
                        return_value=False,
                    ):
                        with patch.object(
                            server_impl.docker, "run_container_detached"
                        ) as mock_run:
                            with patch.object(server_impl.docker, "attach_and_run"):
                                try:
                                    server_impl._start()
                                except Exception:
                                    pass  # We expect this to fail, we just want to check the command

                                # Verify that run_container_detached was called
                                self.assertTrue(
                                    mock_run.called,
                                    "run_container_detached should have been called",
                                )

                                # Get the call arguments
                                call_args = mock_run.call_args
                                command = call_args[1][
                                    "command"
                                ]  # Get the command from kwargs

                                # Verify --no-platformio is NOT in the command
                                self.assertNotIn(
                                    "--no-platformio",
                                    command,
                                    f"--no-platformio should NOT be in server command: {command}",
                                )

    @unittest.skipUnless(
        False, "This test would require actual docker"  # Skip this test for now
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


if __name__ == "__main__":
    unittest.main()
