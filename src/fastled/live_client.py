import threading
import warnings
from pathlib import Path

from fastled.compile_server import CompileServer
from fastled.types import BuildMode


class LiveClient:
    """LiveClient class watches for changes and auto-triggeres rebuild."""

    def __init__(
        self,
        sketch_directory: Path,
        host: str | CompileServer | None = None,
        auto_start: bool = True,
        auto_updates: bool = True,
        open_web_browser: bool = True,
        http_port: (
            int | None
        ) = None,  # None means auto select a free port. -1 means no server.
        keep_running: bool = True,
        build_mode: BuildMode = BuildMode.QUICK,
        profile: bool = False,
        no_platformio: bool = False,
    ) -> None:
        self.sketch_directory = sketch_directory
        self.host = host
        self.open_web_browser = open_web_browser
        self.http_port = http_port
        self.keep_running = keep_running
        self.build_mode = build_mode
        self.profile = profile
        self.auto_start = auto_start
        self.shutdown = threading.Event()
        self.thread: threading.Thread | None = None
        self.auto_updates = auto_updates
        self.no_platformio = no_platformio
        if auto_start:
            self.start()
        if self.auto_updates is False:
            warnings.warn("Auto updates False are not supported yet.")

    def run(self) -> int:
        """Run the client."""
        from fastled.client_server import run_client  # avoid circular import

        rtn = run_client(
            directory=self.sketch_directory,
            host=self.host,
            open_web_browser=self.open_web_browser,
            keep_running=self.keep_running,
            build_mode=self.build_mode,
            profile=self.profile,
            shutdown=self.shutdown,
            http_port=self.http_port,
            no_platformio=self.no_platformio,
        )
        return rtn

    def url(self) -> str:
        """Get the URL of the server."""
        if isinstance(self.host, CompileServer):
            return self.host.url()
        if self.host is None:
            import warnings

            warnings.warn("TODO: use the actual host.")
            return "http://localhost:9021"
        return self.host

    @property
    def running(self) -> bool:
        return self.thread is not None and self.thread.is_alive()

    def start(self) -> None:
        """Start the client."""
        assert not self.running, "LiveClient is already running"
        self.shutdown.clear()
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def stop(self) -> None:
        """Stop the client."""
        self.shutdown.set()
        if self.thread:
            self.thread.join()
            self.thread = None

    def finalize(self) -> None:
        """Finalize the client."""
        self.stop()
        self.thread = None

    def __enter__(self) -> "LiveClient":
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.finalize()
