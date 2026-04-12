import argparse
import logging
import mimetypes
from datetime import datetime, timezone
from multiprocessing import Process
from pathlib import Path

from flask import Flask, Response, send_from_directory
from flask_cors import CORS

from fastled.debug_routes import create_debug_blueprint
from fastled.debug_symbols import (
    DebugSymbolResolver,
    guess_emsdk_path,
    load_debug_symbol_config,
)
from fastled.interrupts import handle_keyboard_interrupt

_ENABLE_LOGGING = False

if _ENABLE_LOGGING:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    )
    logger = logging.getLogger("flask_server")
else:
    logging.getLogger("flask_server").addHandler(logging.NullHandler())
    logging.getLogger("flask_server").propagate = False
    logger = logging.getLogger("flask_server")
    logger.disabled = True


def _check_certificate_expiration(
    certfile: Path,
) -> tuple[bool, int | None, str | None]:
    try:
        from cryptography import x509
        from cryptography.hazmat.backends import default_backend

        with open(certfile, "rb") as handle:
            cert = x509.load_pem_x509_certificate(handle.read(), default_backend())

        expiration_date = cert.not_valid_after_utc
        expiration_str = expiration_date.strftime("%Y-%m-%d")
        days_remaining = (expiration_date - datetime.now(timezone.utc)).days
        return days_remaining > 30, days_remaining, expiration_str
    except KeyboardInterrupt as ki:
        handle_keyboard_interrupt(ki)
        raise
    except Exception as exc:
        logger.warning(f"Failed to check certificate expiration: {exc}")
        return True, None, None


def create_app(
    fastled_js: Path,
    sketch_dir: Path | None = None,
    fastled_dir: Path | None = None,
    emsdk_path: Path | None = None,
) -> Flask:
    app = Flask(__name__)
    CORS(
        app,
        resources={
            r"/*": {
                "origins": "*",
                "methods": ["GET", "POST", "PUT", "DELETE", "OPTIONS"],
                "allow_headers": [
                    "Content-Type",
                    "Authorization",
                    "X-Requested-With",
                ],
            }
        },
    )

    fastled_js = fastled_js.resolve()
    resolver = None
    if sketch_dir is not None:
        config = load_debug_symbol_config(
            sketch_dir=sketch_dir.resolve(),
            fastled_dir=fastled_dir.resolve() if fastled_dir else None,
            emsdk_path=emsdk_path.resolve() if emsdk_path else guess_emsdk_path(),
        )
        resolver = DebugSymbolResolver(config)

    app.register_blueprint(create_debug_blueprint(resolver))

    @app.after_request
    def add_security_headers(response: Response) -> Response:
        response.headers["Cross-Origin-Embedder-Policy"] = "credentialless"
        response.headers["Cross-Origin-Opener-Policy"] = "same-origin"
        response.headers["Cache-Control"] = "no-cache, no-store, must-revalidate"
        response.headers["Pragma"] = "no-cache"
        response.headers["Expires"] = "0"
        return response

    @app.route("/")
    def serve_index() -> Response:
        return send_from_directory(fastled_js, "index.html")

    @app.route("/<path:path>")
    def serve_files(path: str) -> Response:
        file_path = fastled_js / path
        if not file_path.exists():
            return Response(f"File not found: {path}", status=404)

        response = send_from_directory(fastled_js, path)
        content_type, _ = mimetypes.guess_type(str(file_path))
        if path.endswith(".js"):
            content_type = "text/javascript; charset=utf-8"
        elif path.endswith(".wasm"):
            content_type = "application/wasm"
        elif path.endswith(".svg"):
            content_type = "image/svg+xml"
        if content_type:
            response.headers["Content-Type"] = content_type
        return response

    return app


def _run_flask_server(
    fastled_js: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
    sketch_dir: Path | None = None,
    fastled_dir: Path | None = None,
    emsdk_path: Path | None = None,
) -> None:
    app = create_app(
        fastled_js=fastled_js,
        sketch_dir=sketch_dir,
        fastled_dir=fastled_dir,
        emsdk_path=emsdk_path,
    )

    ssl_context = None
    if certfile and keyfile:
        try:
            _check_certificate_expiration(certfile)
            ssl_context = (str(certfile), str(keyfile))
        except KeyboardInterrupt as ki:
            handle_keyboard_interrupt(ki)
        except Exception as exc:
            logger.warning(f"Failed to configure HTTPS server: {exc}")

    app.run(
        host="127.0.0.1",
        port=port,
        debug=False,
        use_reloader=False,
        ssl_context=ssl_context,
    )


def run_flask_in_thread(
    port: int,
    cwd: Path,
    certfile: Path | None = None,
    keyfile: Path | None = None,
    sketch_dir: Path | None = None,
    fastled_dir: Path | None = None,
    emsdk_path: Path | None = None,
) -> None:
    _run_flask_server(
        cwd,
        port,
        certfile,
        keyfile,
        sketch_dir=sketch_dir,
        fastled_dir=fastled_dir,
        emsdk_path=emsdk_path,
    )


def run_flask_server_process(
    port: int,
    cwd: Path,
    certfile: Path | None = None,
    keyfile: Path | None = None,
    sketch_dir: Path | None = None,
    fastled_dir: Path | None = None,
    emsdk_path: Path | None = None,
) -> Process:
    process = Process(
        target=run_flask_in_thread,
        args=(port, cwd, certfile, keyfile, sketch_dir, fastled_dir, emsdk_path),
    )
    process.start()
    return process


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
    )
    parser.add_argument("--port", "-p", type=int, required=True)
    parser.add_argument("--certfile", type=Path)
    parser.add_argument("--keyfile", type=Path)
    parser.add_argument("--sketch-dir", type=Path, default=None)
    parser.add_argument("--fastled-dir", type=Path, default=None)
    parser.add_argument("--emsdk-path", type=Path, default=None)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    run_flask_in_thread(
        args.port,
        args.fastled_js,
        args.certfile,
        args.keyfile,
        sketch_dir=args.sketch_dir,
        fastled_dir=args.fastled_dir,
        emsdk_path=args.emsdk_path,
    )


if __name__ == "__main__":
    main()
