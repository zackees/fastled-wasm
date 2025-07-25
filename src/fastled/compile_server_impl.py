import subprocess
import sys
import time
import traceback
import warnings
from datetime import datetime, timezone
from pathlib import Path

import httpx

from fastled.docker_manager import (
    DISK_CACHE,
    Container,
    DockerManager,
    RunningContainer,
    Volume,
)
from fastled.emoji_util import EMO
from fastled.settings import DEFAULT_CONTAINER_NAME, IMAGE_NAME, SERVER_PORT
from fastled.sketch import looks_like_fastled_repo
from fastled.types import BuildMode, CompileResult, CompileServerError
from fastled.util import port_is_free, print_banner

SERVER_OPTIONS = [
    "--allow-shutdown",  # Allow the server to be shut down without a force kill.
    "--no-auto-update",  # Don't auto live updates from the git repo.
]

LOCAL_DOCKER_ENV = {
    "ONLY_QUICK_BUILDS": "0"  # When running docker always allow release and debug.
}


def _try_get_fastled_src(path: Path) -> Path | None:
    fastled_src_dir: Path | None = None
    if looks_like_fastled_repo(path):
        print(
            "Looks like a FastLED repo, using it as the source directory and mapping it into the server."
        )
        fastled_src_dir = path / "src"
        return fastled_src_dir
    return None


class CompileServerImpl:
    def __init__(
        self,
        interactive: bool = False,
        auto_updates: bool | None = None,
        mapped_dir: Path | None = None,
        auto_start: bool = True,
        container_name: str | None = None,
        remove_previous: bool = False,
        no_platformio: bool = False,
        allow_libcompile: bool = True,
    ) -> None:
        container_name = container_name or DEFAULT_CONTAINER_NAME
        if interactive and not mapped_dir:
            raise ValueError(
                "Interactive mode requires a mapped directory point to a sketch"
            )
        if not interactive and mapped_dir:
            warnings.warn(
                f"Mapped directory {mapped_dir} is ignored in non-interactive mode"
            )
        self.container_name = container_name
        self.mapped_dir = mapped_dir
        self.docker = DockerManager()
        self.fastled_src_dir: Path | None = _try_get_fastled_src(Path(".").resolve())
        self.interactive = interactive
        self.running_container: RunningContainer | None = None
        self.auto_updates = auto_updates
        self.remove_previous = remove_previous
        self.no_platformio = no_platformio

        # Guard: libfastled compilation requires volume source mapping
        # If we don't have fastled_src_dir (not in FastLED repo), disable libcompile
        if allow_libcompile and self.fastled_src_dir is None:
            print(
                f"{EMO('⚠️', 'WARNING:')}  libfastled compilation disabled: volume source mapping not available"
            )
            print("   (not running in FastLED repository)")
            allow_libcompile = False

        self.allow_libcompile = allow_libcompile
        self._port = 0  # 0 until compile server is started
        if auto_start:
            self.start()

    def start(self, wait_for_startup=True) -> None:
        if not DockerManager.is_docker_installed():
            raise CompileServerError("Docker is not installed")
        if self._port != 0:
            warnings.warn("Server has already been started")
        self._port = self._start()
        if wait_for_startup:
            ok = self.wait_for_startup()
            if not ok:
                if self.interactive:
                    print("Exited from container.")
                    sys.exit(0)
                    return
                raise CompileServerError("Server did not start")
        if not self.interactive:
            msg = f"# FastLED Compile Server started at {self.url()} #"
            print("\n" + "#" * len(msg))
            print(msg)
            print("#" * len(msg) + "\n")

    def web_compile(
        self,
        directory: Path | str,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,
    ) -> CompileResult:
        from fastled.web_compile import web_compile  # avoid circular import

        if not self._port:
            raise RuntimeError("Server has not been started yet")
        if not self.ping():
            raise RuntimeError("Server is not running")
        out: CompileResult = web_compile(
            directory,
            host=self.url(),
            build_mode=build_mode,
            profile=profile,
            no_platformio=self.no_platformio,
            allow_libcompile=self.allow_libcompile,
        )
        return out

    def project_init(
        self, example: str | None = None, outputdir: Path | None = None
    ) -> None:
        from fastled.project_init import project_init  # avoid circular import

        project_init(example=example, outputdir=outputdir)

    @property
    def running(self) -> tuple[bool, Exception | None]:
        if not self._port:
            return False, Exception("Docker hasn't been initialzed with a port yet")
        if not DockerManager.is_docker_installed():
            return False, Exception("Docker is not installed")
        docker_running, err = self.docker.is_running()
        if not docker_running:
            IS_MAC = sys.platform == "darwin"
            if IS_MAC:
                if "FileNotFoundError" in str(err):
                    traceback.print_exc()
                    print("\n\nNone fatal debug print for MacOS\n")
            return False, err
        ok: bool = self.docker.is_container_running(self.container_name)
        if ok:
            return True, None
        else:
            return False, Exception("Docker is not running")

    def using_fastled_src_dir_volume(self) -> bool:
        out = self.fastled_src_dir is not None
        if out:
            print(f"Using FastLED source directory: {self.fastled_src_dir}")
        return out

    def port(self) -> int:
        if self._port == 0:
            warnings.warn("Server has not been started yet")
        return self._port

    def url(self) -> str:
        if self._port == 0:
            warnings.warn("Server has not been started yet")
        return f"http://localhost:{self._port}"

    def ping(self) -> bool:
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
        return False

    # by default this is automatically called by the constructor, unless
    # auto_start is set to False.
    def wait_for_startup(self, timeout: int = 100) -> bool:
        """Wait for the server to start up."""
        start_time = time.time()
        while time.time() - start_time < timeout:
            # ping the server to see if it's up
            if not self._port:
                return False
            # use httpx to ping the server
            # if successful, return True
            if self.ping():
                return True
            time.sleep(0.1)
            if not self.docker.is_container_running(self.container_name):
                return False
        return False

    def _start(self) -> int:
        print("Compiling server starting")

        # Ensure Docker is running
        running: bool
        # err: Exception | None
        running, _ = self.docker.is_running()
        if not running:
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
        updated = self.docker.validate_or_download_image(
            image_name=IMAGE_NAME, tag="latest", upgrade=upgrade
        )
        DISK_CACHE.put("last-update", now_str)
        INTERNAL_DOCKER_PORT = 80

        print("Docker image now validated")
        port = SERVER_PORT
        if self.interactive:
            server_command = ["/bin/bash"]
        else:
            server_command = ["python", "/js/run.py", "server"] + SERVER_OPTIONS
            if self.no_platformio:
                server_command.append("--no-platformio")
        if self.interactive:
            print("Disabling port forwarding in interactive mode")
            ports = {}
        else:
            ports = {INTERNAL_DOCKER_PORT: port}
        volumes = []
        if self.fastled_src_dir:
            msg = f"FastLED REPO updates enabled!!\n\nMounting FastLED source directory\n{self.fastled_src_dir} into container\n/host/fastled/src"
            print_banner(msg)
            volumes.append(
                Volume(
                    host_path=str(self.fastled_src_dir),
                    container_path="/host/fastled/src",
                    mode="ro",
                )
            )
        if self.interactive:
            # add the mapped directory to the container
            print(f"Mounting {self.mapped_dir} into container /mapped")
            assert self.mapped_dir is not None
            dir_name = self.mapped_dir.name
            if not volumes:
                volumes = []
            volumes.append(
                Volume(
                    host_path=str(self.mapped_dir),
                    container_path=f"/mapped/{dir_name}",
                    mode="rw",
                )
            )
            if self.fastled_src_dir is not None:
                # to allow for interactive compilation
                # interactive_sources = list(INTERACTIVE_SOURCES)
                interactive_sources: list[tuple[Path, str]] = []
                src_host: Path
                dst_container: str
                for src_host, dst_container in interactive_sources:
                    src_path = Path(src_host).absolute()
                    if src_path.exists():
                        print(f"Mounting {src_host} into container")
                        volumes.append(
                            Volume(
                                host_path=str(src_path),
                                container_path=dst_container,
                                mode="rw",
                            )
                        )
                    else:
                        print(f"Could not find {src_path}")

        cmd_str = subprocess.list2cmdline(server_command)
        if not self.interactive:
            container: Container = self.docker.run_container_detached(
                image_name=IMAGE_NAME,
                tag="latest",
                container_name=self.container_name,
                command=cmd_str,
                ports=ports,
                volumes=volumes,
                remove_previous=self.interactive or self.remove_previous or updated,
                environment=LOCAL_DOCKER_ENV,
            )
            self.running_container = self.docker.attach_and_run(container)
            assert self.running_container is not None, "Container should be running"
            print("Compile server starting")
            return port
        else:
            client_port_mapped = INTERNAL_DOCKER_PORT in ports
            free_port = port_is_free(INTERNAL_DOCKER_PORT)
            if client_port_mapped and free_port:
                warnings.warn(
                    f"Can't expose port {INTERNAL_DOCKER_PORT}, disabling port forwarding in interactive mode"
                )
                ports = {}
            self.docker.run_container_interactive(
                image_name=IMAGE_NAME,
                tag="latest",
                container_name=self.container_name,
                command=cmd_str,
                ports=ports,
                volumes=volumes,
                environment=LOCAL_DOCKER_ENV,
            )

            print("Exiting interactive mode")
            return port

    def process_running(self) -> bool:
        return self.docker.is_container_running(self.container_name)

    def stop(self) -> None:
        if self.docker.is_suspended:
            return
        if self.running_container:
            self.running_container.detach()
            self.running_container = None
        self.docker.suspend_container(self.container_name)
        self._port = 0
        print("Compile server stopped")
