from pathlib import Path

from fastled.types import BuildMode, CompileResult, Platform


class CompileServer:

    # May throw CompileServerError if auto_start is True.
    def __init__(
        self,
        interactive: bool = False,
        auto_updates: bool | None = None,
        mapped_dir: Path | None = None,
        auto_start: bool = True,
        container_name: str | None = None,
        platform: Platform = Platform.WASM,
    ) -> None:
        from fastled.compile_server_impl import (  # avoid circular import
            CompileServerImpl,
        )

        assert platform == Platform.WASM, "Only WASM platform is supported right now."

        self.impl = CompileServerImpl(
            container_name=container_name,
            interactive=interactive,
            auto_updates=auto_updates,
            mapped_dir=mapped_dir,
            auto_start=auto_start,
        )

    # May throw CompileServerError if server could not be started.
    def start(self, wait_for_startup=True) -> None:
        # from fastled.compile_server_impl import CompileServerImpl  # avoid circular import
        self.impl.start(wait_for_startup=wait_for_startup)

    def web_compile(
        self,
        directory: Path | str,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,
    ) -> CompileResult:
        return self.impl.web_compile(
            directory=directory, build_mode=build_mode, profile=profile
        )

    def project_init(
        self, example: str | None = None, outputdir: Path | None = None
    ) -> None:
        from fastled.project_init import project_init  # avoid circular import

        project_init(example=example, outputdir=outputdir)

    @property
    def running(self) -> bool:
        return self.impl.running

    @property
    def fastled_src_dir(self) -> Path | None:
        return self.impl.fastled_src_dir

    def using_fastled_src_dir_volume(self) -> bool:
        return self.impl.using_fastled_src_dir_volume()

    def port(self) -> int:
        return self.impl.port()

    def url(self) -> str:
        return self.impl.url()

    def ping(self) -> bool:
        return self.impl.ping()

    # by default this is automatically called by the constructor, unless
    # auto_start is set to False.
    def wait_for_startup(self, timeout: int = 100) -> bool:
        """Wait for the server to start up."""
        return self.impl.wait_for_startup(timeout=timeout)

    def _start(self) -> int:
        return self.impl._start()

    def stop(self) -> None:
        return self.impl.stop()

    def process_running(self) -> bool:
        return self.impl.process_running()
