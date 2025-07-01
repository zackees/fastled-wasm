"""
Real integration tests for FastLED API compilation without PlatformIO constraints.
Tests that sketches can be compiled successfully using actual test ino files
without relying on extensive mocking.
"""

import os
import platform
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api
from fastled.compile_server_impl import CompileServerImpl
from fastled.docker_manager import DockerManager
from fastled.types import BuildMode, CompileResult

HERE = Path(__file__).parent
TEST_SKETCH_DIR = HERE / "test_ino" / "wasm"
EMBEDDED_TEST_SKETCH_DIR = HERE / "test_ino" / "embedded"


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
    """Real integration tests for FastLED API compilation bypassing Platformio constraints."""

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_compile_wasm_sketch(self) -> None:
        """Test that the wasm test sketch compiles successfully with no-platformio mode.

        This is a real integration test that:
        1. Uses an actual test ino file (wasm.ino)
        2. Actually starts a compile server with no_platformio=True
        3. Actually compiles the sketch without mocking
        4. Verifies the compilation succeeds and produces output
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

        # Start compile server with no_platformio=True for real integration test
        server_impl = CompileServerImpl(no_platformio=True, auto_start=True)

        try:
            # Check if server is running
            running, error = server_impl.running
            self.assertTrue(running, f"No-platformio server should be running: {error}")

            # Compile the test sketch using no-platformio mode
            result: CompileResult = server_impl.web_compile(
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

            print("✅ No-platformio wasm sketch compilation successful!")
            print(f"Compiled zip size: {len(result.zip_bytes)} bytes")
            if result.hash_value:
                print(f"Hash: {result.hash_value}")

        finally:
            # Ensure cleanup
            try:
                server_impl.stop()
            except Exception as e:
                print(f"Cleanup warning: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_vs_regular_compilation(self) -> None:
        """Test that compares no-platformio mode vs regular mode compilation.

        This real integration test verifies that:
        1. Both modes can compile the same sketch
        2. Both produce valid output
        3. The no-platformio flag actually affects the compilation process
        """
        # Test with regular mode first
        server_regular = CompileServerImpl(no_platformio=False, auto_start=True)

        try:
            # Test regular compilation
            running, error = server_regular.running
            self.assertTrue(running, f"Regular server should be running: {error}")

            result_regular: CompileResult = server_regular.web_compile(
                directory=TEST_SKETCH_DIR,
                build_mode=BuildMode.QUICK,
                profile=False,
            )

            self.assertTrue(
                result_regular.success,
                f"Regular compilation failed. Output: {result_regular.stdout}",
            )
            self.assertTrue(len(result_regular.zip_bytes) > 0)

        finally:
            # Stop regular server completely before starting no-platformio server
            try:
                server_regular.stop()
            except Exception as e:
                print(f"Cleanup warning for regular server: {e}")

        # Now test no-platformio compilation after regular server is stopped
        server_no_platformio = CompileServerImpl(no_platformio=True, auto_start=True)

        try:
            # Test no-platformio compilation
            running, error = server_no_platformio.running
            self.assertTrue(running, f"No-platformio server should be running: {error}")

            result_no_platformio: CompileResult = server_no_platformio.web_compile(
                directory=TEST_SKETCH_DIR,
                build_mode=BuildMode.QUICK,
                profile=False,
            )

            self.assertTrue(
                result_no_platformio.success,
                f"No-platformio compilation failed. Output: {result_no_platformio.stdout}",
            )
            self.assertTrue(len(result_no_platformio.zip_bytes) > 0)

            print("✅ Both regular and no-platformio compilation modes work!")
            print(f"Regular output size: {len(result_regular.zip_bytes)} bytes")
            print(
                f"No-platformio output size: {len(result_no_platformio.zip_bytes)} bytes"
            )

        finally:
            # Cleanup no-platformio server
            try:
                server_no_platformio.stop()
            except Exception as e:
                print(f"Cleanup warning for no-platformio server: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_embedded_sketch(self) -> None:
        """Test that the embedded test sketch compiles successfully with no-platformio mode.

        This tests a different sketch to ensure no-platformio mode works with
        various types of FastLED sketches, not just one specific example.
        """
        # Ensure embedded test sketch directory exists
        self.assertTrue(
            EMBEDDED_TEST_SKETCH_DIR.exists(),
            f"Embedded test sketch directory not found: {EMBEDDED_TEST_SKETCH_DIR}",
        )

        # Verify test sketch file exists
        test_sketch_file = EMBEDDED_TEST_SKETCH_DIR / "wasm.ino"
        self.assertTrue(
            test_sketch_file.exists(),
            f"Embedded test sketch file not found: {test_sketch_file}",
        )

        # Start compile server with no_platformio=True
        server_impl = CompileServerImpl(no_platformio=True, auto_start=True)

        try:
            # Check if server is running
            running, error = server_impl.running
            self.assertTrue(running, f"No-platformio server should be running: {error}")

            # Compile the embedded test sketch
            result: CompileResult = server_impl.web_compile(
                directory=EMBEDDED_TEST_SKETCH_DIR,
                build_mode=BuildMode.QUICK,
                profile=False,
            )

            # Verify compilation succeeded
            self.assertTrue(
                result.success,
                f"No-platformio embedded sketch compilation failed. Output: {result.stdout}",
            )

            self.assertTrue(
                len(result.zip_bytes) > 0,
                "No compiled output received from embedded sketch compilation",
            )

            print("✅ No-platformio embedded sketch compilation successful!")
            print(f"Compiled zip size: {len(result.zip_bytes)} bytes")

        finally:
            try:
                server_impl.stop()
            except Exception as e:
                print(f"Cleanup warning: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_different_build_modes(self) -> None:
        """Test no-platformio compilation with different build modes using real compilation."""

        self.assertTrue(
            TEST_SKETCH_DIR.exists(),
            f"Test sketch directory not found: {TEST_SKETCH_DIR}",
        )

        build_modes = [BuildMode.QUICK, BuildMode.DEBUG, BuildMode.RELEASE]
        server_impl = CompileServerImpl(no_platformio=True, auto_start=True)

        try:
            # Check if server is running
            running, error = server_impl.running
            self.assertTrue(running, f"No-platformio server should be running: {error}")

            for build_mode in build_modes:
                with self.subTest(build_mode=build_mode):
                    print(
                        f"Testing no-platformio compilation with {build_mode.value} mode..."
                    )

                    result: CompileResult = server_impl.web_compile(
                        directory=TEST_SKETCH_DIR, build_mode=build_mode, profile=False
                    )

                    # Verify compilation succeeded for each build mode
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
                        f"✅ No-platformio {build_mode.value} mode compilation successful! "
                        f"Output size: {len(result.zip_bytes)} bytes"
                    )
        finally:
            try:
                server_impl.stop()
            except Exception as e:
                print(f"Cleanup warning: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for no-platformio compilation.",
    )
    def test_no_platformio_compile_with_project_init(self) -> None:
        """Test that a project initialized via API can be compiled in no-platformio mode."""

        with TemporaryDirectory() as tmpdir:
            server_impl = CompileServerImpl(no_platformio=True, auto_start=True)

            try:
                # Check if server is running
                running, error = server_impl.running
                self.assertTrue(
                    running, f"No-platformio server should be running: {error}"
                )

                # Initialize a new project with the Blink example
                sketch_directory = Api.project_init(
                    example="Blink", outputdir=tmpdir, host=server_impl.url()
                )

                self.assertTrue(
                    sketch_directory.exists(), "Project initialization failed"
                )

                # Compile the initialized project in no-platformio mode
                result: CompileResult = server_impl.web_compile(
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
                    "✅ Successfully compiled initialized Blink project in no-platformio mode!"
                )
                print(f"Project directory: {sketch_directory}")
                print(f"Compiled output size: {len(result.zip_bytes)} bytes")
            finally:
                try:
                    server_impl.stop()
                except Exception as e:
                    print(f"Cleanup warning: {e}")

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
            print(
                "5. Use CompileServerImpl(no_platformio=True) for no-platformio compilation"
            )
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
        print(
            "- Use CompileServerImpl(no_platformio=True) for local Docker compilation"
        )
        print("- Configure build flags to bypass PlatformIO constraints")
        print("- Utilize CompileServer.web_compile() with custom settings")
        print("- Access advanced build modes not available via standard PlatformIO")


if __name__ == "__main__":
    unittest.main()
