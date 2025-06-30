"""
Unit test for local FastLED API compilation.
Tests that a sketch can be compiled successfully using the local API.
"""

import os
import platform
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from fastled import Api, CompileServer
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


class LocalApiCompileTester(unittest.TestCase):
    """Test local API compilation functionality."""

    @unittest.skipUnless(_enabled(), "Can only happen with a local server.")
    def test_local_api_compile_success(self) -> None:
        """Test that a sketch compiles successfully using the local FastLED API."""
        
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


if __name__ == "__main__":
    unittest.main()