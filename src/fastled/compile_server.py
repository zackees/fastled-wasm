import socket
import subprocess
import time
from pathlib import Path

import httpx

from fastled.docker_manager import DockerManager
from fastled.sketch import looks_like_fastled_repo

_DEFAULT_CONTAINER_NAME = "fastled-wasm-compiler"

SERVER_PORT = 9021


def find_available_port(start_port: int = SERVER_PORT) -> int:
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
        self,
        container_name=_DEFAULT_CONTAINER_NAME,
        interactive: bool = False,
    ) -> None:

        cwd = Path(".").resolve()
        fastled_src_dir: Path | None = None
        if looks_like_fastled_repo(cwd):
            print(
                "Looks like a FastLED repo, using it as the source directory and mapping it into the server."
            )
            fastled_src_dir = cwd / "src"

        self.container_name = container_name
        self.docker = DockerManager(container_name=container_name)
        self.running = False
        self.running_process: subprocess.Popen | None = None
        self.fastled_src_dir: Path | None = fastled_src_dir
        self.interactive = interactive
        self._port = self._start()
        # fancy print
        if not interactive:
            msg = f"# FastLED Compile Server started at {self.url()} #"
            print("\n" + "#" * len(msg))
            print(msg)
            print("#" * len(msg) + "\n")

    def using_fastled_src_dir_volume(self) -> bool:
        return self.fastled_src_dir is not None

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
                response = httpx.get(
                    f"http://localhost:{self._port}", follow_redirects=True
                )
                if response.status_code < 400:
                    return True
            except KeyboardInterrupt:
                raise
            except Exception:
                pass
            time.sleep(0.1)
            if not self.running:
                return False
        return False

    def _start(self) -> int:
        print("Compiling server starting")
        self.running = True
        # Ensure Docker is running
        with self.docker.get_lock():
            if not self.docker.is_running():
                if not self.docker.start():
                    print("Docker could not be started. Exiting.")
                    raise RuntimeError("Docker could not be started. Exiting.")

            # Clean up any existing container with the same name
            try:
                container_exists = (
                    subprocess.run(
                        ["docker", "inspect", self.container_name],
                        capture_output=True,
                        text=True,
                    ).returncode
                    == 0
                )
                if container_exists:
                    print("Cleaning up existing container")
                    subprocess.run(
                        ["docker", "rm", "-f", self.container_name],
                        check=False,
                    )
            except KeyboardInterrupt:
                raise
            except Exception as e:
                print(f"Warning: Failed to remove existing container: {e}")

            print("Ensuring Docker image exists at latest version")
            if not self.docker.ensure_image_exists():
                print("Failed to ensure Docker image exists.")
                raise RuntimeError("Failed to ensure Docker image exists")

        print("Docker image now validated")
        port = find_available_port()
        print(f"Found an available port: {port}")
        if self.interactive:
            server_command = ["/bin/bash"]
        else:
            server_command = ["python", "/js/run.py", "server", "--allow-shutdown"]
        print(f"Started Docker container with command: {server_command}")
        ports = {port: 80}
        volumes = None
        if self.fastled_src_dir:
            print(
                f"Mounting FastLED source directory {self.fastled_src_dir} into container /host/fastled/src"
            )
            volumes = {
                str(self.fastled_src_dir): {"bind": "/host/fastled/src", "mode": "ro"}
            }
            if not self.interactive:
                # no auto-update because the source directory is mapped in.
                # This should be automatic now.
                server_command.append("--no-auto-update")  # stop git repo updates.
        self.running_process = self.docker.run_container(
            server_command, ports=ports, volumes=volumes, tty=self.interactive
        )
        print("Compile server starting")
        time.sleep(3)
        if self.running_process.poll() is not None:
            print("Server failed to start")
            self.running = False
            raise RuntimeError("Server failed to start")
        return port

    def proceess_running(self) -> bool:
        if self.running_process is None:
            return False
        return self.running_process.poll() is None

    def stop(self) -> None:
        print(f"Stopping server on port {self._port}")
        # # attempt to send a shutdown signal to the server
        # try:
        #     httpx.get(f"http://localhost:{self._port}/shutdown", timeout=2)
        # # except Exception:
        # except Exception as e:
        #     print(f"Failed to send shutdown signal: {e}")
        #     pass
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
            if self.running_process and self.running_process.stdout:
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
            if self.running_process:
                self.running_process.kill()
                self.running_process.wait()
        except KeyboardInterrupt:
            if self.running_process:
                self.running_process.kill()
                self.running_process.wait()
        except Exception as e:
            print(f"Error stopping Docker container: {e}")
        finally:
            self.running_process = None
            self.running = False
        print("Compile server stopped")
