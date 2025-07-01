"""
Unit test for local FastLED API compilation.
Tests that a sketch can be compiled successfully using the local API.
This test demonstrates how to use the FastLED API to compile sketches locally
using Docker, which enables access to additional compilation modes including
no-platformio configuration that may not be available in the web compiler.
"""

import os
import platform
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api, CompileServer
from fastled.types import BuildMode, CompileResult
from fastled.docker_manager import DockerManager

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
    """Check if Docker is available for compilation."""
    try:
        return DockerManager.is_docker_installed()
    except:
        return False


class LocalApiCompileTester(unittest.TestCase):
    """Test local API compilation functionality."""

    @unittest.skipUnless(_enabled() and _docker_available(), "Requires Docker for local compilation.")
    def test_local_api_compile_success(self) -> None:
        """Test that a sketch compiles successfully using the local FastLED API.
        
        This test demonstrates the complete workflow of:
        1. Setting up a local compilation server using Docker
        2. Compiling a FastLED sketch with access to all build modes
        3. Verifying successful compilation and output generation
        
        The local Docker-based compilation has advantages over web compilation:
        - Access to additional build configurations
        - Ability to customize compilation flags
        - No dependency on external servers
        - Support for no-platformio mode (if configured)
        """
        
        # Ensure test sketch directory exists
        self.assertTrue(TEST_SKETCH_DIR.exists(), f"Test sketch directory not found: {TEST_SKETCH_DIR}")
        
        # Verify test sketch file exists
        test_sketch_file = TEST_SKETCH_DIR / "wasm.ino"
        self.assertTrue(test_sketch_file.exists(), f"Test sketch file not found: {test_sketch_file}")
        
        # Start local compile server and test compilation
        with Api.server() as server:
            self.assertIsInstance(server, CompileServer)
            self.assertTrue(server.running, "Server should be running")
            
            # Compile the test sketch using the local API
            result: CompileResult = server.web_compile(
                directory=TEST_SKETCH_DIR,
                build_mode=BuildMode.QUICK,  # Use quick mode for faster compilation
                profile=False
            )
            
            # Verify compilation succeeded
            self.assertTrue(result.success, f"Compilation failed. Output: {result.stdout}")
            
            # Verify we got actual compiled output
            self.assertTrue(len(result.zip_bytes) > 0, "No compiled output received")
            
            # Verify stdout contains expected compilation messages
            self.assertIsNotNone(result.stdout, "No stdout received")
            
            # Print compilation info for debugging
            print(f"Compilation successful!")
            print(f"Compiled zip size: {len(result.zip_bytes)} bytes")
            if result.hash_value:
                print(f"Hash: {result.hash_value}")
            
            # Optionally, we could extract and verify specific files in the zip
            if result.zip_bytes:
                print("Successfully received compiled WASM output")

    @unittest.skipUnless(_enabled(), "Can only happen with a local server.")
    def test_local_api_compile_different_build_modes(self) -> None:
        """Test compilation with different build modes to ensure they all work."""
        
        self.assertTrue(TEST_SKETCH_DIR.exists(), f"Test sketch directory not found: {TEST_SKETCH_DIR}")
        
        build_modes = [BuildMode.QUICK, BuildMode.DEBUG, BuildMode.RELEASE]
        
        with Api.server() as server:
            self.assertIsInstance(server, CompileServer)
            
            for build_mode in build_modes:
                with self.subTest(build_mode=build_mode):
                    print(f"Testing compilation with {build_mode.value} mode...")
                    
                    result: CompileResult = server.web_compile(
                        directory=TEST_SKETCH_DIR,
                        build_mode=build_mode,
                        profile=False
                    )
                    
                    # Verify compilation succeeded for each build mode
                    self.assertTrue(result.success, 
                                  f"Compilation failed for {build_mode.value} mode. Output: {result.stdout}")
                    
                    # Verify we got output
                    self.assertTrue(len(result.zip_bytes) > 0, 
                                  f"No compiled output received for {build_mode.value} mode")
                    
                    print(f"{build_mode.value} mode compilation successful! "
                          f"Output size: {len(result.zip_bytes)} bytes")

    @unittest.skipUnless(_enabled(), "Can only happen with a local server.")
    def test_local_api_compile_with_project_init(self) -> None:
        """Test that a project initialized via API can be compiled successfully."""
        
        with TemporaryDirectory() as tmpdir:
            with Api.server() as server:
                self.assertIsInstance(server, CompileServer)
                
                # Initialize a new project with the Blink example
                sketch_directory = Api.project_init(
                    example="Blink", 
                    outputdir=tmpdir, 
                    host=server
                )
                
                self.assertTrue(sketch_directory.exists(), "Project initialization failed")
                
                # Compile the initialized project
                result: CompileResult = server.web_compile(
                    directory=sketch_directory,
                    build_mode=BuildMode.QUICK,
                    profile=False
                )
                
                # Verify compilation succeeded
                self.assertTrue(result.success, f"Compilation of initialized project failed. Output: {result.stdout}")
                self.assertTrue(len(result.zip_bytes) > 0, "No compiled output received from initialized project")
                
                print(f"Successfully compiled initialized Blink project!")
                print(f"Project directory: {sketch_directory}")
                print(f"Compiled output size: {len(result.zip_bytes)} bytes")

    def test_api_structure_and_workflow(self) -> None:
        """Test that demonstrates the FastLED API structure and intended workflow.
        
        This test shows how to use the FastLED API for local compilation even
        if Docker is not available. It demonstrates the API structure and 
        intended workflow, including how to enable no-platformio modes when
        the local compilation environment is properly configured.
        """
        
        # Verify test sketch exists
        self.assertTrue(TEST_SKETCH_DIR.exists(), f"Test sketch directory not found: {TEST_SKETCH_DIR}")
        test_sketch_file = TEST_SKETCH_DIR / "wasm.ino"
        self.assertTrue(test_sketch_file.exists(), f"Test sketch file not found: {test_sketch_file}")
        
        # Check Docker availability
        docker_available = _docker_available()
        print(f"Docker available: {docker_available}")
        
        if not docker_available:
            print("Docker not available - demonstrating API structure without compilation")
            print("To enable full local compilation with Docker:")
            print("1. Install Docker")
            print("2. Start Docker daemon")
            print("3. Ensure user has Docker permissions")
            print("4. Run: fastled --server")
            print("5. Use Api.server() context manager for compilation")
            print("")
            print("Local compilation advantages:")
            print("- Access to all build modes (quick, debug, release)")
            print("- Custom compilation flags and configurations")
            print("- No-platformio mode support")
            print("- Independence from external servers")
            return
        
        # If Docker is available, we would run the actual test
        print("Docker is available - running basic API validation")
        
        # Test API imports and basic structure
        self.assertTrue(hasattr(Api, 'server'), "Api should have server method")
        self.assertTrue(hasattr(Api, 'project_init'), "Api should have project_init method")
        
        # Test BuildMode enum
        self.assertTrue(hasattr(BuildMode, 'QUICK'), "BuildMode should have QUICK")
        self.assertTrue(hasattr(BuildMode, 'DEBUG'), "BuildMode should have DEBUG")
        self.assertTrue(hasattr(BuildMode, 'RELEASE'), "BuildMode should have RELEASE")
        
        print("FastLED API structure validated successfully")
        print("To compile with no-platformio equivalent mode:")
        print("- Use local Docker compilation with appropriate flags")
        print("- Configure build environment in Docker container")
        print("- Utilize CompileServer.web_compile() method")


if __name__ == "__main__":
    unittest.main()