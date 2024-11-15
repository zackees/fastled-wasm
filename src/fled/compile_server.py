import socket
import subprocess
import threading
import time
from typing import Optional

import httpx

from fled.docker_manager import DockerManager

_DEFAULT_CONTAINER_NAME = "fastled-wasm-compiler"

_DEFAULT_START_PORT = 9021


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
    def __init__(
        self, container_name=_DEFAULT_CONTAINER_NAME, disable_auto_clean: bool = False
    ) -> None:
        self.container_name = container_name
        self.disable_auto_clean = disable_auto_clean
        self.docker = DockerManager(container_name=container_name)
        self.running = False
        self.thread: Optional[threading.Thread] = None
        self.running_process: subprocess.Popen | None = None
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
        if self.disable_auto_clean:
            server_command.append("--disable-auto-clean")
        print(f"Started Docker container with command: {server_command}")
        ports = {port: 80}
        self.running_process = self.docker.run_container(server_command, ports=ports)
        time.sleep(3)
        if self.running_process.poll() is not None:
            print("Server failed to start")
            self.running = False
            raise RuntimeError("Server failed to start")
        self.thread = threading.Thread(target=self._server_loop)
        self.thread.start()
        print("Compile server started")
        return port

    def stop(self) -> None:
        print(f"Stopping server on port {self._port}")
        if self.running_process:
            try:
                # Stop the Docker container
                cp: subprocess.CompletedProcess
                cp = subprocess.run(
                    ["docker", "stop", self.container_name],
                    capture_output=True,
                    text=True,
                    check=True,
                )
                if cp.returncode != 0:
                    print(f"Failed to stop Docker container: {cp.stderr}")

                cp = subprocess.run(
                    ["docker", "rm", self.container_name],
                    capture_output=True,
                    text=True,
                    check=True,
                )
                if cp.returncode != 0:
                    print(f"Failed to remove Docker container: {cp.stderr}")

                # Close the stdout pipe
                if self.running_process.stdout:
                    self.running_process.stdout.close()

                # Wait for the process to fully terminate with a timeout
                self.running_process.wait(timeout=10)
                if self.running_process.returncode is None:
                    # kill
                    self.running_process.kill()
                if self.running_process.returncode is not None:
                    print(
                        f"Server stopped with return code {self.running_process.returncode}"
                    )

            except subprocess.TimeoutExpired:
                # Force kill if it doesn't stop gracefully
                self.running_process.kill()
                self.running_process.wait()
            except Exception as e:
                print(f"Error stopping Docker container: {e}")
            finally:
                self.running_process = None
        # Signal the server thread to stop
        self.running = False
        if self.thread:
            self.thread.join(timeout=10)  # Wait up to 10 seconds for thread to finish
            if self.thread.is_alive():
                print("Warning: Server thread did not terminate properly")

        print("Compile server stopped")

    def _server_loop(self) -> None:
        while self.running:
            if self.running_process:
                # Read Docker container output
                # Check if Docker process is still running
                if self.running_process.poll() is not None:
                    print("Docker server stopped unexpectedly")
                    self.running = False
                    break

            time.sleep(0.1)  # Prevent busy waiting
