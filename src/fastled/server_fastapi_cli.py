from multiprocessing import Process
from pathlib import Path

import uvicorn

# MAPPING = {
#     "js": "application/javascript",
#     "css": "text/css",
#     "wasm": "application/wasm",
#     "json": "application/json",
#     "png": "image/png",
#     "jpg": "image/jpeg",
#     "jpeg": "image/jpeg",
#     "gif": "image/gif",
#     "svg": "image/svg+xml",
#     "ico": "image/x-icon",
#     "html": "text/html",
# }


def _run_fastapi_server(
    port: int,
    cwd: Path,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    # Uvicorn “reload” will watch your Python files for changes.
    import os

    os.chdir(cwd)
    uvicorn.run(
        "fastled.server_fastapi:app",
        host="127.0.0.1",
        port=port,
        reload=False,
        # reload_includes=["index.html"],
        ssl_certfile=certfile,
        ssl_keyfile=keyfile,
    )


def run_fastapi_server_process(
    port: int,
    cwd: Path | None = None,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> Process:
    """Run the FastAPI server in a separate process."""
    cwd = cwd or Path(".")
    process = Process(
        target=_run_fastapi_server,
        args=(port, cwd, certfile, keyfile),
    )
    process.start()
    return process


if __name__ == "__main__":
    # Example usage
    proc = run_fastapi_server_process(port=8000)
    proc.join()
