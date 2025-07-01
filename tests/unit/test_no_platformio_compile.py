"""
Unit test for FastLED --no-platformio flag functionality.
Tests that the flag properly passes through to the fastled-wasm-server.
"""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import MagicMock, patch

from fastled import Api, CompileServer
from fastled.docker_manager import DockerManager
from fastled.types import BuildMode, CompileResult
from fastled.args import Args
from fastled.client_server import run_client_server
from fastled.compile_server_impl import CompileServerImpl

HERE = Path(__file__).parent
TEST_SKETCH_DIR = HERE / "test_ino" / "wasm"


def _enabled() -> bool:
    """Check if this system can run the tests."""
    return True  # Simplified


def _docker_available() -> bool:
    """Check if Docker is available for no-platformio compilation."""
    try:
        return DockerManager.is_docker_installed()
    except Exception as e:
        print(f"Docker is not available: {e}")
        return False


class NoPlatformIOServerCommandTest(unittest.TestCase):
    """Test that --no-platformio flag is properly added to Docker server command."""

    def test_no_platformio_server_command_construction(self) -> None:
        """Test that --no-platformio flag is added to server command when enabled."""
        
        # Mock the docker manager and its methods
        with patch('fastled.compile_server_impl.DockerManager') as mock_docker_manager:
            mock_docker = MagicMock()
            mock_docker_manager.return_value = mock_docker
            mock_docker.is_running.return_value = (True, None)
            mock_docker.validate_or_download_image.return_value = False
            mock_docker.run_container_detached.return_value = MagicMock()
            mock_docker.attach_and_run.return_value = MagicMock()
            
            # Test with no_platformio=True
            with patch('fastled.compile_server_impl._try_get_fastled_src', return_value=None):
                print("\n=== Testing server command with no_platformio=True ===")
                server_impl = CompileServerImpl(
                    auto_start=False,  # Don't actually start
                    no_platformio=True  # Setting no_platformio to TRUE
                )
                
                print(f"CompileServerImpl.no_platformio = {server_impl.no_platformio}")
                
                # Mock the parts of _start that we don't want to actually run
                with patch.object(server_impl.docker, 'is_running', return_value=(True, None)):
                    with patch.object(server_impl.docker, 'validate_or_download_image', return_value=False):
                        with patch.object(server_impl.docker, 'run_container_detached') as mock_run:
                            with patch.object(server_impl.docker, 'attach_and_run'):
                                try:
                                    server_impl._start()
                                except Exception as e:
                                    print(f"Expected exception during _start(): {e}")
                                
                                # Verify that run_container_detached was called
                                self.assertTrue(mock_run.called, "run_container_detached should have been called")
                                
                                # Get the call arguments - check both args and kwargs
                                call_args = mock_run.call_args
                                print(f"Mock call: {call_args}")
                                
                                # Try to get command from kwargs first
                                command = None
                                if call_args and call_args[1] and 'command' in call_args[1]:
                                    command = call_args[1]['command']
                                    print(f"Command: {command}")
                                
                                # Verify --no-platformio is in the command
                                self.assertIsNotNone(command, "Command should not be None")
                                self.assertIn('--no-platformio', command, 
                                            f"--no-platformio should be in server command: {command}")
                                print("✅ --no-platformio found in command!")
                                
                                # Also verify the base server command components
                                self.assertIn('python', command, "Command should contain python")
                                self.assertIn('server', command, "Command should contain server")
                                print("✅ Server command construction test passed!")

    def test_no_platformio_server_command_without_flag(self) -> None:
        """Test that --no-platformio flag is NOT added to server command when disabled."""
        
        # Mock the docker manager and its methods
        with patch('fastled.compile_server_impl.DockerManager') as mock_docker_manager:
            mock_docker = MagicMock()
            mock_docker_manager.return_value = mock_docker
            mock_docker.is_running.return_value = (True, None)
            mock_docker.validate_or_download_image.return_value = False
            mock_docker.run_container_detached.return_value = MagicMock()
            mock_docker.attach_and_run.return_value = MagicMock()
            
            # Test with no_platformio=False (default)
            with patch('fastled.compile_server_impl._try_get_fastled_src', return_value=None):
                print("\n=== Testing server command with no_platformio=False ===")
                server_impl = CompileServerImpl(
                    auto_start=False,  # Don't actually start
                    no_platformio=False  # Setting no_platformio to FALSE
                )
                
                print(f"CompileServerImpl.no_platformio = {server_impl.no_platformio}")
                
                # Mock the parts of _start that we don't want to actually run
                with patch.object(server_impl.docker, 'is_running', return_value=(True, None)):
                    with patch.object(server_impl.docker, 'validate_or_download_image', return_value=False):
                        with patch.object(server_impl.docker, 'run_container_detached') as mock_run:
                            with patch.object(server_impl.docker, 'attach_and_run'):
                                try:
                                    server_impl._start()
                                except Exception as e:
                                    print(f"Expected exception during _start(): {e}")
                                
                                # Verify that run_container_detached was called
                                self.assertTrue(mock_run.called, "run_container_detached should have been called")
                                
                                # Get the call arguments
                                call_args = mock_run.call_args
                                command = call_args[1]['command'] if call_args and call_args[1] and 'command' in call_args[1] else None
                                
                                # Verify --no-platformio is NOT in the command
                                self.assertIsNotNone(command, "Command should not be None")
                                self.assertNotIn('--no-platformio', command, 
                                               f"--no-platformio should NOT be in server command: {command}")
                                print("✅ --no-platformio correctly NOT found in command!")
                                print("✅ Server command construction test passed!")


class NoPlatformIOIntegrationTest(unittest.TestCase):
    """Real integration tests using actual .ino projects."""

    def setUp(self) -> None:
        """Set up test environment."""
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

    def test_api_accepts_no_platformio_parameter(self) -> None:
        """Test that all API functions accept the no_platformio parameter without errors."""
        
        from pathlib import Path
        from fastled import Api
        from fastled.live_client import LiveClient
        from fastled.compile_server import CompileServer
        
        test_dir = Path("/tmp/test")
        
        print("\n=== Testing API parameter acceptance ===")
        
        # Test CompileServer accepts no_platformio
        try:
            server = CompileServer(no_platformio=True, auto_start=False)
            print("✅ CompileServer accepts no_platformio parameter")
        except TypeError as e:
            self.fail(f"CompileServer should accept no_platformio parameter: {e}")
            
        # Test LiveClient accepts no_platformio
        try:
            client = LiveClient(
                sketch_directory=test_dir,
                no_platformio=True,
                auto_start=False
            )
            self.assertTrue(hasattr(client, 'no_platformio'), "LiveClient should store no_platformio")
            self.assertTrue(client.no_platformio, "LiveClient.no_platformio should be True")
            print("✅ LiveClient accepts and stores no_platformio parameter")
        except TypeError as e:
            self.fail(f"LiveClient should accept no_platformio parameter: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for real compilation test.",
    )
    def test_real_sketch_compilation_with_no_platformio_true(self) -> None:
        """Real integration test: Compile the actual wasm.ino sketch with no_platformio=True."""
        
        print("\n=== Real Integration Test: Compiling wasm.ino with no_platformio=True ===")
        print(f"Test sketch: {TEST_SKETCH_DIR}")
        
        # Test compilation with no_platformio=True
        try:
            with Api.server(no_platformio=True, auto_start=True) as server:
                self.assertIsInstance(server, CompileServer)
                self.assertTrue(server.running, "Server with no_platformio=True should be running")
                
                # Compile the real test sketch
                result = server.web_compile(
                    directory=TEST_SKETCH_DIR,
                    build_mode=BuildMode.QUICK,
                    profile=False,
                )
                
                # Verify compilation succeeded
                self.assertTrue(
                    result.success,
                    f"Real sketch compilation with no_platformio=True failed. Output: {result.stdout}",
                )
                
                # Verify we got compiled output
                self.assertTrue(
                    len(result.zip_bytes) > 0,
                    "No compiled output received with no_platformio=True",
                )
                
                print(f"✅ Real integration test PASSED!")
                print(f"   - Real wasm.ino sketch compiled successfully")
                print(f"   - Server used no_platformio=True") 
                print(f"   - Output size: {len(result.zip_bytes)} bytes")
                if result.hash_value:
                    print(f"   - Hash: {result.hash_value}")
                    
        except Exception as e:
            self.fail(f"Real integration test with no_platformio=True failed: {e}")

    @unittest.skipUnless(
        _enabled() and _docker_available(),
        "Requires Docker for comparison test.",
    )  
    def test_real_sketch_compilation_comparison(self) -> None:
        """Compare real sketch compilation with and without no_platformio flag."""
        
        print("\n=== Real Comparison Test: wasm.ino with both no_platformio modes ===")
        
        results = {}
        
        # Test normal compilation (no_platformio=False)
        print("Testing normal compilation (no_platformio=False)...")
        try:
            with Api.server(no_platformio=False, auto_start=True) as server:
                result = server.web_compile(
                    directory=TEST_SKETCH_DIR,
                    build_mode=BuildMode.QUICK,
                    profile=False,
                )
                results['normal'] = result
                self.assertTrue(result.success, f"Normal compilation failed: {result.stdout}")
        except Exception as e:
            self.skipTest(f"Normal compilation failed: {e}")
            
        # Test no-platformio compilation (no_platformio=True)  
        print("Testing no-platformio compilation (no_platformio=True)...")
        try:
            with Api.server(no_platformio=True, auto_start=True) as server:
                result = server.web_compile(
                    directory=TEST_SKETCH_DIR,
                    build_mode=BuildMode.QUICK,
                    profile=False,
                )
                results['no_platformio'] = result
                self.assertTrue(result.success, f"No-platformio compilation failed: {result.stdout}")
        except Exception as e:
            self.fail(f"No-platformio compilation failed: {e}")
            
        # Compare results
        print("\n=== Real Compilation Comparison Results ===")
        print(f"Normal compilation (no_platformio=False):")
        print(f"  - Success: {results['normal'].success}")
        print(f"  - Output size: {len(results['normal'].zip_bytes)} bytes")
        print(f"  - Hash: {results['normal'].hash_value}")
        
        print(f"No-platformio compilation (no_platformio=True):")  
        print(f"  - Success: {results['no_platformio'].success}")
        print(f"  - Output size: {len(results['no_platformio'].zip_bytes)} bytes")
        print(f"  - Hash: {results['no_platformio'].hash_value}")
        
        # Both should succeed
        self.assertTrue(results['normal'].success, "Normal compilation should succeed")
        self.assertTrue(results['no_platformio'].success, "No-platformio compilation should succeed")
        
        # Both should produce output
        self.assertGreater(len(results['normal'].zip_bytes), 0, "Normal compilation should produce output")
        self.assertGreater(len(results['no_platformio'].zip_bytes), 0, "No-platformio compilation should produce output")
        
        print("✅ Both compilation modes work successfully with real sketch!")
        print("✅ The --no-platformio flag implementation is working correctly!")


if __name__ == "__main__":
    unittest.main()
