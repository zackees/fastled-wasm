#!/usr/bin/env bash
# Installs the built wheel from dist/ into a fresh venv and runs the packaged
# console script outside this checkout, in the shipped viewer. It exercises the
# wheel's bundled Rust binary and frontend, rather than development fallbacks
# such as CARGO_MANIFEST_DIR.
#
# Set FASTLED_SAFARI_SMOKE=1 (macOS only) to also load the compiled sketch in
# real Safari through safaridriver while the asset origin is still running.
set -euo pipefail

SMOKE_ROOT="$RUNNER_TEMP/fastled-wheel-smoke"
rm -rf "$SMOKE_ROOT"
mkdir -p "$SMOKE_ROOT/home" "$SMOKE_ROOT/sketch/data" "$SMOKE_ROOT/artifacts" "$SMOKE_ROOT/asset-origin"
uv venv "$SMOKE_ROOT/venv" --python "$UV_PYTHON"
uv pip install --python "$SMOKE_ROOT/venv/bin/python" dist/*.whl
test -x "$SMOKE_ROOT/venv/bin/uv"
BASE_PYTHON_DIR="$("$SMOKE_ROOT/venv/bin/python" -c 'from pathlib import Path; import sys; print(Path(sys.executable).resolve().parent)')"
test ! -e "$BASE_PYTHON_DIR/uv"
cp -R "$GITHUB_WORKSPACE/tests/fixtures/wasm_vfs/." "$SMOKE_ROOT/sketch/"
cp "$GITHUB_WORKSPACE/tests/fixtures/wasm_vfs_origin/probe.txt" "$SMOKE_ROOT/asset-origin/probe.txt"
cat > "$SMOKE_ROOT/sketch/data/probe.txt.lnk" <<'EOF'
http://127.0.0.1:18765/probe.txt
sha256=2da77f7c1452125633beeeb43f366350018643c799306b4d24d8610d98f02b55
size_bytes=15
storage=vfs
EOF
cat > "$SMOKE_ROOT/asset_server.py" <<'PY'
import http.server
import sys

class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        super().end_headers()

    def log_message(self, *_args):
        pass

handler = lambda *args, **kwargs: Handler(*args, directory=sys.argv[1], **kwargs)
http.server.ThreadingHTTPServer(("127.0.0.1", 18765), handler).serve_forever()
PY
"$SMOKE_ROOT/venv/bin/python" "$SMOKE_ROOT/asset_server.py" "$SMOKE_ROOT/asset-origin" &
ASSET_SERVER_PID=$!
trap 'kill "$ASSET_SERVER_PID" 2>/dev/null || true' EXIT
for _attempt in {1..20}; do
  if curl --fail --silent http://127.0.0.1:18765/probe.txt >/dev/null; then break; fi
  sleep 0.25
done
curl --fail --silent http://127.0.0.1:18765/probe.txt >/dev/null

# The installed launcher must supply absolute wheel-venv uv, Python,
# and frontend paths to the Rust binary. Deliberately omit ambient
# uv, node, and bare python so this cannot accidentally pass by
# resolving a developer tool from the runner image.
cd "$SMOKE_ROOT"
env -i \
  HOME="$SMOKE_ROOT/home" \
  PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
  /bin/bash -ceu '
    for tool in uv node python; do
      if command -v "$tool" >/dev/null 2>&1; then
        echo "unexpected ambient $tool on restricted PATH" >&2
        exit 1
      fi
    done
  '
env -i \
  HOME="$SMOKE_ROOT/home" \
  PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
  "$SMOKE_ROOT/venv/bin/fastled" "$SMOKE_ROOT/sketch" \
    --check \
    --test-wait-secs=2 \
    --test-timeout-secs=240 \
    --test-ready-timeout-secs=45 \
    --test-screenshot="$SMOKE_ROOT/artifacts/screenmap.png" \
    --test-log="$SMOKE_ROOT/artifacts/viewer.log"
grep -F "Asset 'data/probe.txt' sha256 verified" "$SMOKE_ROOT/artifacts/viewer.log"
grep -F "All 1 filesystem asset(s) loaded completely before setup()." "$SMOKE_ROOT/artifacts/viewer.log"
"$SMOKE_ROOT/venv/bin/python" - "$SMOKE_ROOT/artifacts/screenmap.png" <<'PY'
from pathlib import Path
import sys

image = Path(sys.argv[1]).read_bytes()
if image[:8] != b"\x89PNG\r\n\x1a\n":
    raise SystemExit("viewer screenshot is not a PNG")
if image[12:16] != b"IHDR" or len(image) < 24:
    raise SystemExit("viewer screenshot is missing its IHDR header")
width = int.from_bytes(image[16:20], "big")
height = int.from_bytes(image[20:24], "big")
if width <= 0 or height <= 0:
    raise SystemExit(f"viewer screenshot has invalid dimensions: {width}x{height}")
PY

if [ "${FASTLED_SAFARI_SMOKE:-0}" = "1" ]; then
  # The viewer run above left the compiled sketch in sketch/fastled_js; the
  # asset origin is still serving, so Safari loads the same program.
  uv run --no-project --with pillow python "$GITHUB_WORKSPACE/ci/safari_smoke.py" \
    --fastled "$SMOKE_ROOT/venv/bin/fastled" \
    --serve-dir "$SMOKE_ROOT/sketch/fastled_js" \
    --artifacts "$SMOKE_ROOT/artifacts"
fi
