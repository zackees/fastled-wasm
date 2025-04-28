from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse

# your existing MIME mapping, e.g.
MAPPING = {
    "js": "application/javascript",
    "css": "text/css",
    "wasm": "application/wasm",
    "json": "application/json",
    "png": "image/png",
    "jpg": "image/jpeg",
    "jpeg": "image/jpeg",
    "gif": "image/gif",
    "svg": "image/svg+xml",
    "ico": "image/x-icon",
    "html": "text/html",
}


"""Run FastAPI server with live reload or HTTPS depending on args."""
app = FastAPI(debug=True)
base = Path(".")


def no_cache_headers() -> dict[str, str]:
    return {
        "Cache-Control": "no-cache, no-store, must-revalidate",
        "Pragma": "no-cache",
        "Expires": "0",
    }


@app.get("/")
async def serve_index():
    index_path = base / "index.html"
    if not index_path.exists():
        raise HTTPException(status_code=404, detail="index.html not found")
    return FileResponse(
        index_path,
        media_type=MAPPING.get("html"),
        headers=no_cache_headers(),
    )


@app.get("/{path:path}")
async def serve_files(path: str):
    file_path = base / path
    if not file_path.exists() or not file_path.is_file():
        raise HTTPException(status_code=404, detail=f"{path} not found")

    ext = path.rsplit(".", 1)[-1].lower()
    media_type = MAPPING.get(ext)

    return FileResponse(
        file_path,
        media_type=media_type,
        headers=no_cache_headers(),
    )
