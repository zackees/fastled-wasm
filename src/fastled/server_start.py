from pathlib import Path

from fastled.server_flask import run_flask_in_thread


def start_server_in_thread(
    port: int,
    fastled_js: Path,
    compile_server_port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Start the server in a separate thread."""

    run_flask_in_thread(
        port=port,
        cwd=fastled_js,
        compile_server_port=compile_server_port,
        certfile=certfile,
        keyfile=keyfile,
    )
