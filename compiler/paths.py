from pathlib import Path

UPLOAD_DIR = Path("/uploads")
TEMP_DIR = Path("/tmp")
OUTPUT_DIR = Path("/output")


COMPILER_ROOT = Path("/js")
SKETCH_SRC = COMPILER_ROOT / "src"
VOLUME_MAPPED_SRC = Path("/host/fastled/src")
RSYNC_DEST = COMPILER_ROOT / "fastled" / "src"
SKETCH_CACHE_FILE = OUTPUT_DIR / "compile_cache.db"
LIVE_GIT_FASTLED_DIR = Path("/git/fastled")

# COMPILER_DIR = Path("/js/compiler")
COMPIER_DIR = COMPILER_ROOT / "compiler"
FASTLED_COMPILER_DIR = COMPILER_ROOT / "fastled/src/platforms/wasm/compiler"
PIO_BUILD_DIR = COMPILER_ROOT / ".pio/build"
