import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

import httpx

from fastled.docker_manager import (
    DISK_CACHE,
    Container,
    DockerManager,
    RunningContainer,
)
from fastled.sketch import looks_like_fastled_repo

_IMAGE_NAME = "niteris/fastled-wasm"
_DEFAULT_CONTAINER_NAME = "fastled-wasm-compiler"

SERVER_PORT = 9021

SERVER_OPTIONS = ["--allow-shutdown", "--no-auto-update"]


class CompileServer:
    def __init__(
        self,
        container_name=_DEFAULT_CONTAINER_NAME,
        interactive: bool = False,
        auto_updates: bool | None = None,
        mapped_dir: Path | None = None,
    ) -> None:
        if interactive and not mapped_dir:
            raise ValueError(
                "Interactive mode requires a mapped directory point to a sketch"
            )
        if not interactive and mapped_dir:
            raise ValueError("Mapped directory is only used in interactive mode")
        cwd = Path(".").resolve()
        fastled_src_dir: Path | None = None
        if looks_like_fastled_repo(cwd):
            print(
                "Looks like a FastLED repo, using it as the source directory and mapping it into the server."
            )
            fastled_src_dir = cwd / "src"

        self.container_name = container_name
        self.mapped_dir = mapped_dir
        self.docker = DockerManager()
        self.fastled_src_dir: Path | None = fastled_src_dir
        self.interactive = interactive
        self.running_container: RunningContainer | None = None
        self.auto_updates = auto_updates
        self._port = self._start()
        # fancy print
        if not interactive:
            msg = f"# FastLED Compile Server started at {self.url()} #"
            print("\n" + "#" * len(msg))
            print(msg)
            print("#" * len(msg) + "\n")

    @property
    def running(self) -> bool:
        if not self._port:
            return False
        if not DockerManager.is_docker_installed():
            return False
        if not DockerManager.is_running():
            return False
        return self.docker.is_container_running(self.container_name)

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
            if not self.docker.is_container_running(self.container_name):
                return False
        return False

    def _start(self) -> int:
        print("Compiling server starting")

        # Ensure Docker is running
        if not self.docker.is_running():
            if not self.docker.start():
                print("Docker could not be started. Exiting.")
                raise RuntimeError("Docker could not be started. Exiting.")
        now = datetime.now(timezone.utc)
        now_str = now.strftime("%Y-%m-%d")

        upgrade = False
        if self.auto_updates is None:
            prev_date_str = DISK_CACHE.get("last-update")
            if prev_date_str != now_str:
                print("One day has passed, checking docker for updates")
                upgrade = True
        else:
            upgrade = self.auto_updates
        self.docker.validate_or_download_image(
            image_name=_IMAGE_NAME, tag="main", upgrade=upgrade
        )
        DISK_CACHE.put("last-update", now_str)

        print("Docker image now validated")
        port = SERVER_PORT
        if self.interactive:
            server_command = ["/bin/bash"]
        else:
            server_command = ["python", "/js/run.py", "server"] + SERVER_OPTIONS
        ports = {80: port}
        volumes = None
        if self.fastled_src_dir:
            print(
                f"Mounting FastLED source directory {self.fastled_src_dir} into container /host/fastled/src"
            )
            volumes = {
                str(self.fastled_src_dir): {"bind": "/host/fastled/src", "mode": "ro"}
            }
        if self.interactive:
            # add the mapped directory to the container
            print(f"Mounting {self.mapped_dir} into container /mapped")
            # volumes = {str(self.mapped_dir): {"bind": "/mapped", "mode": "rw"}}
            # add it
            assert self.mapped_dir is not None
            dir_name = self.mapped_dir.name
            if not volumes:
                volumes = {}
            volumes[str(self.mapped_dir)] = {
                "bind": f"/mapped/{dir_name}",
                "mode": "rw",
            }

        cmd_str = subprocess.list2cmdline(server_command)
        if not self.interactive:
            container: Container = self.docker.run_container_detached(
                image_name=_IMAGE_NAME,
                tag="main",
                container_name=self.container_name,
                command=cmd_str,
                ports=ports,
                volumes=volumes,
                remove_previous=self.interactive,
            )
            self.running_container = self.docker.attach_and_run(container)
            assert self.running_container is not None, "Container should be running"
            print("Compile server starting")
            return port
        else:
            self.docker.run_container_interactive(
                image_name=_IMAGE_NAME,
                tag="main",
                container_name=self.container_name,
                command=cmd_str,
                ports=ports,
                volumes=volumes,
            )

            print("Exiting interactive mode")
            return port

    def proceess_running(self) -> bool:
        return self.docker.is_container_running(self.container_name)

    def stop(self) -> None:
        # print(f"Stopping server on port {self._port}")
        if self.running_container:
            self.running_container.stop()
        self.docker.suspend_container(self.container_name)
        print("Compile server stopped")
