import socket
import subprocess
import threading
import time
from typing import Optional

import httpx

from fastled_wasm.docker_manager import DockerManager

_DEFAULT_CONTAINER_NAME = "fastled-wasm-compiler"

_DEFAULT_START_PORT = 9021
_DISABLE_AUTO_CLEAN = False  # Massive speed boost by assuming clean state.


def _find_available_port(start_port: int = _DEFAULT_START_PORT) -> int:
    """Find an available port starting from the given port."""
    port = start_port
    end_port = start_port + 1000
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            if s.connect_ex(("localhost", port)) != 0:
                return port
            port += 1
            if port >= end_port:
                raise RuntimeError("No available ports found")


class CompileServer:
    def __init__(self, container_name=_DEFAULT_CONTAINER_NAME) -> None:
        self.container_name = container_name
        self.docker = DockerManager(container_name=container_name)
        self.running = False
        self.thread: Optional[threading.Thread] = None
        self.docker_process: Optional[subprocess.Popen] = None
        self._port = self.start()

    def port(self) -> int:
        return self._port

    def url(self) -> str:
        return f"http://localhost:{self._port}"

    def wait_for_startup(self, timeout: int = 100) -> bool:
        """Wait for the server to start up."""
        start_time = time.time()
        while time.time() - start_time < timeout:
            # ping the server to see if it's up
            if not self._port:
                return False
            # use httpx to ping the server
            # if successful, return True
            try:
                response = httpx.get(f"http://localhost:{self._port}")
                if response.status_code < 400:
                    return True
            except Exception:
                pass
            time.sleep(0.1)
            if not self.running:
                return False
        return False

    def start(self) -> int:
        self.running = True
        # Ensure Docker is running and image exists
        if not self.docker.is_running():
            if not self.docker.start():
                print("Docker could not be started. Exiting.")
                raise RuntimeError("Docker could not be started. Exiting.")
        if not self.docker.ensure_image_exists():
            print("Failed to ensure Docker image exists.")
            raise RuntimeError("Failed to ensure Docker image exists")

        if self.docker.container_exists():
            if not self.docker.remove_container():
                print("Failed to remove existing container")
                raise RuntimeError("Failed to remove existing container")
        # Remove the image to force a fresh download
        subprocess.run(["docker", "rmi", "fastled-wasm"], capture_output=True)
        print("All clean")

        port = _find_available_port()
        server_command = ["python", "/js/run.py", "server"]
        if _DISABLE_AUTO_CLEAN:
            server_command.append("--disable-auto-clean")

        full_container_name = self.docker.full_container_name()

        # Start the Docker container in server mode
        docker_command = [
            "docker",
            "run",
            "--name",
            self.container_name,
            "-p",  # Port mapping flag
            f"{port}:80",  # Map dynamic host port to container port 80
            "--expose",  # Explicitly expose the port
            "80",  # Expose port 80 in container
            full_container_name,
        ] + server_command

        cmd_str = subprocess.list2cmdline(docker_command)
        print(f"Starting Docker container with command: {cmd_str}")

        self.docker_process = subprocess.Popen(docker_command, text=True)

        self.thread = threading.Thread(target=self._server_loop)
        self.thread.daemon = True
        self.thread.start()
        print("Compile server started")
        return port

    def stop(self) -> None:
        self.running = False
        if self.docker_process:
            try:
                # Stop the Docker container
                subprocess.run(
                    ["docker", "stop", self.container_name],
                    capture_output=True,
                    check=True,
                )
                subprocess.run(
                    ["docker", "rm", self.container_name],
                    capture_output=True,
                    check=True,
                )

                # Close the stdout pipe
                if self.docker_process.stdout:
                    self.docker_process.stdout.close()

                # Wait for the process to fully terminate with a timeout
                self.docker_process.wait(timeout=10)
                if self.docker_process.returncode is None:
                    # kill
                    self.docker_process.kill()
                if self.docker_process.returncode is not None:
                    print(
                        f"Server stopped with return code {self.docker_process.returncode}"
                    )

            except subprocess.TimeoutExpired:
                # Force kill if it doesn't stop gracefully
                self.docker_process.kill()
                self.docker_process.wait()
            except Exception as e:
                print(f"Error stopping Docker container: {e}")
            finally:
                self.docker_process = None

        if self.thread:
            self.thread.join(timeout=10)  # Wait up to 10 seconds for thread to finish
            if self.thread.is_alive():
                print("Warning: Server thread did not terminate properly")

        print("Compile server stopped")

    def _server_loop(self) -> None:
        while self.running:
            if self.docker_process:
                # Read Docker container output
                # Check if Docker process is still running
                if self.docker_process.poll() is not None:
                    print("Docker server stopped unexpectedly")
                    self.running = False
                    break

            time.sleep(0.1)  # Prevent busy waiting


def start_compile_server() -> CompileServer:
    server = CompileServer()
    return server