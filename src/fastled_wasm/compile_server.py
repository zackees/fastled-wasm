import subprocess
import threading
import time
from typing import Optional

from fastled_wasm.config import Config
from fastled_wasm.docker_manager import DockerManager

CONTAINER_NAME = "fastled-wasm-compiler"
DOCKER = DockerManager(container_name=CONTAINER_NAME)
CONFIG: Config = Config()


class CompileServer:
    def __init__(self):
        self.running = False
        self.thread: Optional[threading.Thread] = None
        self.compile_queue = []  # Could be replaced with proper Queue
        self.docker_process: Optional[subprocess.Popen] = None
        self.start()

    def start(self):
        self.running = True
        # Ensure Docker is running and image exists
        if not DOCKER.is_running():
            if not DOCKER.start():
                print("Docker could not be started. Exiting.")
                return
        if not DOCKER.ensure_image_exists():
            print("Failed to ensure Docker image exists.")
            return

        # Start the Docker container in server mode
        docker_command = [
            "docker",
            "run",
            "--name",
            CONTAINER_NAME,
            "-it",  # Interactive mode with pseudo-TTY
            "fastled-wasm",
            "python",
            "/js/run.py",
            "server",
        ]

        self.docker_process = subprocess.Popen(
            docker_command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
        )

        self.thread = threading.Thread(target=self._server_loop)
        self.thread.daemon = True
        self.thread.start()
        print("Compile server started")

    def stop(self):
        self.running = False
        if self.docker_process:
            try:
                # Stop the Docker container
                subprocess.run(
                    ["docker", "stop", CONTAINER_NAME], capture_output=True, check=True
                )
                subprocess.run(
                    ["docker", "rm", CONTAINER_NAME], capture_output=True, check=True
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

    def _server_loop(self):
        while self.running:
            if self.docker_process:
                # Read Docker container output
                if self.docker_process.stdout:
                    line = self.docker_process.stdout.readline()
                    if line:
                        print(f"Docker server: {line.strip()}")

                # Check if Docker process is still running
                if self.docker_process.poll() is not None:
                    print("Docker server stopped unexpectedly")
                    self.running = False
                    break

            if self.compile_queue:
                print("Would process compile request here")
                # TODO: Implement actual compilation logic
            time.sleep(0.1)  # Prevent busy waiting


def start_compile_server() -> CompileServer:
    server = CompileServer()
    return server
