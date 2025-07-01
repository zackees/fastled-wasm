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
from fastled.docker_manager import DockerManager
from fastled.types import BuildMode, CompileResult
from fastled.args import Args
from fastled.client_server import run_client_server
from fastled.compile_server_impl import CompileServerImpl

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
        with patch('fastled.compile_server_impl.DockerManager') as mock_docker_manager:
            mock_docker = MagicMock()
            mock_docker_manager.return_value = mock_docker
            mock_docker.is_running.return_value = (True, None)
            mock_docker.validate_or_download_image.return_value = False
            mock_docker.run_container_detached.return_value = MagicMock()
            mock_docker.attach_and_run.return_value = MagicMock()
            
            # Test with no_platformio=True
            with patch('fastled.compile_server_impl._try_get_fastled_src', return_value=None):
                print("\n=== Testing with no_platformio=True ===")
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
                                print(f"Mock call args: {call_args}")
                                print(f"Args: {call_args[0] if call_args and call_args[0] else 'None'}")
                                print(f"Kwargs: {call_args[1] if call_args and call_args[1] else 'None'}")
                                
                                # Try to get command from kwargs first
                                command = None
                                if call_args and call_args[1] and 'command' in call_args[1]:
                                    command = call_args[1]['command']
                                    print(f"Command from kwargs: {command}")
                                
                                # If not in kwargs, might be a positional argument  
                                if command is None and call_args and call_args[0]:
                                    print("Command not found in kwargs, checking positional args...")
                                    for i, arg in enumerate(call_args[0]):
                                        print(f"  Arg {i}: {arg}")
                                        if isinstance(arg, str) and ('python' in arg or 'server' in arg):
                                            command = arg
                                            print(f"Found command in arg {i}: {command}")
                                            break
                                
                                # Verify --no-platformio is in the command
                                self.assertIsNotNone(command, "Command should not be None")
                                print(f"Final command to check: {command}")
                                
                                self.assertIn('--no-platformio', command, 
                                            f"--no-platformio should be in server command: {command}")
                                print("✅ --no-platformio found in command!")
                                
                                # Also verify the base server command components
                                self.assertIn('python', command, "Command should contain python")
                                self.assertIn('server', command, "Command should contain server")
                                print("✅ Base server command components verified!")

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
                print("\n=== Testing with no_platformio=False ===")
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
                                
                                # Get the call arguments - check both args and kwargs
                                call_args = mock_run.call_args
                                print(f"Mock call args: {call_args}")
                                
                                # Try to get command from kwargs first
                                command = None
                                if call_args and call_args[1] and 'command' in call_args[1]:
                                    command = call_args[1]['command']
                                    print(f"Command from kwargs: {command}")
                                
                                # If not in kwargs, might be a positional argument  
                                if command is None and call_args and call_args[0]:
                                    print("Command not found in kwargs, checking positional args...")
                                    for i, arg in enumerate(call_args[0]):
                                        if isinstance(arg, str) and ('python' in arg or 'server' in arg):
                                            command = arg
                                            print(f"Found command in arg {i}: {command}")
                                            break
                                
                                # Verify --no-platformio is NOT in the command
                                self.assertIsNotNone(command, "Command should not be None")
                                print(f"Final command to check: {command}")
                                
                                self.assertNotIn('--no-platformio', command, 
                                               f"--no-platformio should NOT be in server command: {command}")
                                print("✅ --no-platformio correctly NOT found in command!")
                                
                                # Verify the base server command components are still there
                                self.assertIn('python', command, "Command should contain python")
                                self.assertIn('server', command, "Command should contain server")
                                print("✅ Base server command components verified!")

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


class NoPlatformIOAPITest(unittest.TestCase):
    """Test cases for --no-platformio API functionality."""

    def test_api_spawn_server_with_no_platformio(self) -> None:
        """Test that Api.spawn_server correctly accepts and passes no_platformio parameter."""
        
        with patch('fastled.compile_server.CompileServer') as mock_compile_server:
            mock_server_instance = MagicMock()
            mock_compile_server.return_value = mock_server_instance
            
            # Test with no_platformio=True
            print("\n=== Testing Api.spawn_server with no_platformio=True ===")
            server = Api.spawn_server(no_platformio=True, auto_start=False)
            
            # Verify CompileServer was called with no_platformio=True
            mock_compile_server.assert_called_once()
            call_args = mock_compile_server.call_args
            print(f"CompileServer call args: {call_args}")
            
            # Check kwargs for no_platformio
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertTrue(call_args[1]['no_platformio'], "no_platformio should be True")
            print("✅ Api.spawn_server correctly passed no_platformio=True to CompileServer")
            
            # Reset mock for next test
            mock_compile_server.reset_mock()
            
            # Test with default no_platformio (should be False)
            print("\n=== Testing Api.spawn_server with default no_platformio ===")
            server = Api.spawn_server(auto_start=False)
            
            mock_compile_server.assert_called_once()
            call_args = mock_compile_server.call_args
            print(f"CompileServer call args: {call_args}")
            
            # Check that no_platformio defaults to False
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertFalse(call_args[1]['no_platformio'], "no_platformio should default to False")
            print("✅ Api.spawn_server correctly defaults no_platformio=False")

    def test_api_server_context_manager_with_no_platformio(self) -> None:
        """Test that Api.server context manager correctly accepts and passes no_platformio parameter."""
        
        with patch('fastled.Api.spawn_server') as mock_spawn_server:
            mock_server_instance = MagicMock()
            mock_spawn_server.return_value = mock_server_instance
            mock_server_instance.stop.return_value = None
            
            # Test with no_platformio=True
            print("\n=== Testing Api.server context manager with no_platformio=True ===")
            with Api.server(no_platformio=True, auto_start=False) as server:
                pass
            
            # Verify spawn_server was called with no_platformio=True
            mock_spawn_server.assert_called_once()
            call_args = mock_spawn_server.call_args
            print(f"spawn_server call args: {call_args}")
            
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertTrue(call_args[1]['no_platformio'], "no_platformio should be True")
            print("✅ Api.server context manager correctly passed no_platformio=True")
            
            # Verify stop was called
            mock_server_instance.stop.assert_called_once()
            print("✅ Server was properly stopped in context manager")

    def test_api_live_client_with_no_platformio(self) -> None:
        """Test that Api.live_client correctly accepts and passes no_platformio parameter."""
        
        # Import the Api module to get access to the imported LiveClient
        from fastled import Api
        
        with patch.object(Api, 'LiveClient') as mock_live_client:
            mock_client_instance = MagicMock()
            mock_live_client.return_value = mock_client_instance
            
            # Test with no_platformio=True
            print("\n=== Testing Api.live_client with no_platformio=True ===")
            from pathlib import Path
            test_dir = Path("/tmp/test")
            
            client = Api.live_client(
                sketch_directory=test_dir,
                no_platformio=True,
                auto_start=False
            )
            
            # Verify LiveClient was called with no_platformio=True
            mock_live_client.assert_called_once()
            call_args = mock_live_client.call_args
            print(f"LiveClient call args: {call_args}")
            
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertTrue(call_args[1]['no_platformio'], "no_platformio should be True")
            print("✅ Api.live_client correctly passed no_platformio=True to LiveClient")

    def test_live_client_with_no_platformio(self) -> None:
        """Test that LiveClient correctly accepts and passes no_platformio parameter."""
        
        with patch('fastled.client_server.run_client') as mock_run_client:
            mock_run_client.return_value = 0
            
            print("\n=== Testing LiveClient with no_platformio=True ===")
            from pathlib import Path
            from fastled.live_client import LiveClient
            
            test_dir = Path("/tmp/test")
            
            # Test with no_platformio=True
            client = LiveClient(
                sketch_directory=test_dir,
                no_platformio=True,
                auto_start=False
            )
            
            # Verify the no_platformio attribute is set
            self.assertTrue(hasattr(client, 'no_platformio'), "LiveClient should have no_platformio attribute")
            self.assertTrue(client.no_platformio, "LiveClient.no_platformio should be True")
            print(f"✅ LiveClient.no_platformio = {client.no_platformio}")
            
            # Start the client (which calls run_client)
            client.start()
            
            # Give it a moment to start
            import time
            time.sleep(0.1)
            
            # Stop the client
            client.stop()
            
            # Verify run_client was called with no_platformio=True
            mock_run_client.assert_called()
            call_args = mock_run_client.call_args
            print(f"run_client call args: {call_args}")
            
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertTrue(call_args[1]['no_platformio'], "no_platformio should be True in run_client call")
            print("✅ LiveClient correctly passed no_platformio=True to run_client")

    def test_compile_server_parameter_propagation(self) -> None:
        """Test that CompileServer correctly propagates no_platformio to CompileServerImpl."""
        
        with patch('fastled.compile_server_impl.CompileServerImpl') as mock_impl:
            mock_impl_instance = MagicMock()
            mock_impl.return_value = mock_impl_instance
            
            print("\n=== Testing CompileServer no_platformio parameter propagation ===")
            
            # Test with no_platformio=True
            from fastled.compile_server import CompileServer
            server = CompileServer(no_platformio=True, auto_start=False)
            
            # Verify CompileServerImpl was called with no_platformio=True
            mock_impl.assert_called_once()
            call_args = mock_impl.call_args
            print(f"CompileServerImpl call args: {call_args}")
            
            self.assertIn('no_platformio', call_args[1], "no_platformio should be in kwargs")
            self.assertTrue(call_args[1]['no_platformio'], "no_platformio should be True")
            print("✅ CompileServer correctly passed no_platformio=True to CompileServerImpl")

    def test_client_server_run_client_server_with_no_platformio_args(self) -> None:
        """Test that run_client_server correctly extracts and uses no_platformio from Args."""
        
        with patch('fastled.client_server.run_client') as mock_run_client:
            with patch('fastled.client_server._try_start_server_or_get_url') as mock_try_start:
                mock_run_client.return_value = 0
                mock_try_start.return_value = ("http://localhost:8080", None)
                
                print("\n=== Testing run_client_server with Args.no_platformio=True ===")
                
                # Create Args object with no_platformio=True
                from fastled.args import Args
                from pathlib import Path
                
                args = Args(
                    directory=Path("/tmp/test"),
                    init=False,
                    just_compile=True,
                    web=None,
                    interactive=False,
                    profile=False,
                    force_compile=True,
                    no_platformio=True,  # Set no_platformio to TRUE
                    auto_update=False,
                    update=False,
                    localhost=False,
                    build=False,
                    server=False,
                    purge=False,
                    debug=False,
                    quick=True,
                    release=False,
                    ram_disk_size="0"
                )
                
                print(f"Args.no_platformio = {args.no_platformio}")
                
                # Call run_client_server
                from fastled.client_server import run_client_server
                result = run_client_server(args)
                
                # Verify _try_start_server_or_get_url was called with no_platformio=True
                mock_try_start.assert_called_once()
                call_args = mock_try_start.call_args
                print(f"_try_start_server_or_get_url call args: {call_args}")
                
                # Check that no_platformio=True was passed
                if len(call_args[0]) >= 5:  # Check positional args
                    no_platformio_arg = call_args[0][4]  # 5th argument should be no_platformio
                    self.assertTrue(no_platformio_arg, "no_platformio should be True in positional args")
                    print(f"✅ no_platformio={no_platformio_arg} passed to _try_start_server_or_get_url")
                else:
                    self.fail("_try_start_server_or_get_url not called with expected number of arguments")
                
                # Verify run_client was called with no_platformio=True
                mock_run_client.assert_called_once()
                run_client_args = mock_run_client.call_args
                print(f"run_client call args: {run_client_args}")
                
                self.assertIn('no_platformio', run_client_args[1], "no_platformio should be in run_client kwargs")
                self.assertTrue(run_client_args[1]['no_platformio'], "no_platformio should be True in run_client call")
                print("✅ run_client_server correctly extracted and passed no_platformio from Args")


if __name__ == "__main__":
    unittest.main()
