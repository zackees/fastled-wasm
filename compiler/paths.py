from pathlib import Path

VOLUME_MAPPED_SRC = Path("/host/fastled/src")
RSYNC_DEST = Path("/js/fastled/src")
UPLOAD_DIR = Path("/uploads")
TEMP_DIR = Path("/tmp")
OUTPUT_DIR = Path("/output")
SKETCH_CACHE_FILE = OUTPUT_DIR / "compile_cache.db"
LIVE_GIT_FASTLED_DIR = Path("/git/fastled")

COMPILER_DIR = Path("/js/compiler")
FASTLED_COMPILER_DIR = Path("/js/fastled/src/platforms/wasm/compiler")
