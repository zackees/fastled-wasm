import argparse
import logging
import time
from datetime import datetime, timezone
from multiprocessing import Process
from pathlib import Path

# Logging configuration
_ENABLE_LOGGING = False


if _ENABLE_LOGGING:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    )
    logger = logging.getLogger("flask_server")
else:
    # Disable all logging
    logging.getLogger("flask_server").addHandler(logging.NullHandler())
    logging.getLogger("flask_server").propagate = False
    logger = logging.getLogger("flask_server")
    logger.disabled = True


def _check_certificate_expiration(
    certfile: Path,
) -> tuple[bool, int | None, str | None]:
    """
    Check if a certificate is expiring soon or has expired.

    Returns:
        Tuple of (is_valid, days_remaining, expiration_date_str)
    """
    try:
        from cryptography import x509
        from cryptography.hazmat.backends import default_backend

        with open(certfile, "rb") as f:
            cert_data = f.read()
            cert = x509.load_pem_x509_certificate(cert_data, default_backend())

        expiration_date = cert.not_valid_after_utc
        expiration_str = expiration_date.strftime("%Y-%m-%d")

        now = datetime.now(timezone.utc)
        days_remaining = (expiration_date - now).days

        is_valid = days_remaining > 30

        return is_valid, days_remaining, expiration_str

    except Exception as e:
        logger.warning(f"Failed to check certificate expiration: {e}")
        return True, None, None


def _run_flask_server(
    fastled_js: Path,
    port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run Flask server with live reload in a subprocess

    Args:
        fastled_js: Path to the fastled_js directory
        port: Port to run the server on
        certfile: Path to the SSL certificate file
        keyfile: Path to the SSL key file
    """
    try:
        from flask import Flask, Response, request, send_from_directory
        from flask_cors import CORS
        from livereload import Server

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

        @app.after_request
        def add_security_headers(response):
            """Add security headers required for cross-origin isolation and audio worklets"""
            response.headers["Cross-Origin-Embedder-Policy"] = "credentialless"
            response.headers["Cross-Origin-Opener-Policy"] = "same-origin"
            return response

        @app.before_request
        def log_request_info():
            """Log details of each request before processing"""
            if _ENABLE_LOGGING:
                logger.info("=" * 80)
                logger.info(f"Request: {request.method} {request.url}")
                logger.info(f"Headers: {dict(request.headers)}")
                logger.info(f"Args: {dict(request.args)}")
                if request.is_json:
                    logger.info(f"JSON: {request.json}")
                if request.form:
                    logger.info(f"Form: {dict(request.form)}")
                logger.info(f"Remote addr: {request.remote_addr}")
                logger.info(f"User agent: {request.user_agent}")

        @app.route("/")
        def serve_index():
            if _ENABLE_LOGGING:
                logger.info("Serving index.html")
            response = send_from_directory(fastled_js, "index.html")
            if _ENABLE_LOGGING:
                logger.info(f"Index response status: {response.status_code}")
            return response

        @app.route("/<path:path>")
        def serve_files(path: str):
            logger.info(f"Received request for path: {path}")

            try:
                start_time = time.time()
                logger.info(f"Processing local file request for {path}")

                file_path = fastled_js / path
                logger.info(f"Full file path: {file_path}")
                logger.info(f"File exists: {file_path.exists()}")

                if not file_path.exists():
                    logger.warning(f"File not found: {file_path}")
                    return Response(f"File not found: {path}", status=404)

                response = send_from_directory(fastled_js, path)

                content_type = None
                if path.endswith(".js"):
                    content_type = "text/javascript; charset=utf-8"
                elif path.endswith(".css"):
                    content_type = "text/css"
                elif path.endswith(".wasm"):
                    content_type = "application/wasm"
                elif path.endswith(".json"):
                    content_type = "application/json"
                elif path.endswith(".png"):
                    content_type = "image/png"
                elif path.endswith(".jpg") or path.endswith(".jpeg"):
                    content_type = "image/jpeg"
                elif path.endswith(".gif"):
                    content_type = "image/gif"
                elif path.endswith(".svg"):
                    content_type = "image/svg+xml"
                elif path.endswith(".ico"):
                    content_type = "image/x-icon"
                elif path.endswith(".html"):
                    content_type = "text/html"

                if content_type:
                    logger.info(f"Setting Content-Type to {content_type}")
                    response.headers["Content-Type"] = content_type

                response.headers["Cache-Control"] = (
                    "no-cache, no-store, must-revalidate"
                )
                response.headers["Pragma"] = "no-cache"
                response.headers["Expires"] = "0"

                logger.info(f"Response status: {response.status_code}")
                logger.debug(f"Response headers: {dict(response.headers)}")

                elapsed_time = time.time() - start_time
                logger.info(f"Request completed in {elapsed_time:.3f} seconds")

                return response

            except Exception as e:
                logger.error(f"Error serving local file {path}: {e}", exc_info=True)
                if isinstance(e, FileNotFoundError):
                    return Response(f"File not found: {path}", status=404)
                return Response(f"Error serving file: {str(e)}", status=500)

        @app.errorhandler(Exception)
        def handle_exception(e):
            """Log any uncaught exceptions"""
            logger.error(f"Unhandled exception: {e}", exc_info=True)
            return Response(f"Server error: {str(e)}", status=500)

        logger.info("Setting up livereload server")
        server = Server(app.wsgi_app)
        server.watch(str(fastled_js / "index.html"))
        logger.info(f"Starting server on port {port}")

        ssl_enabled = False
        if certfile and keyfile:
            try:
                logger.info(
                    f"Configuring SSL with certfile: {certfile}, keyfile: {keyfile}"
                )

                is_valid, days_remaining, expiration_date = (
                    _check_certificate_expiration(certfile)
                )
                if days_remaining is not None:
                    if days_remaining < 0:
                        logger.warning(
                            f"WARNING: SSL certificate has EXPIRED (expired on {expiration_date}). "
                            "Please regenerate certificates to ensure continued HTTPS functionality."
                        )
                    elif days_remaining <= 30:
                        logger.warning(
                            f"WARNING: SSL certificate expires in {days_remaining} days (on {expiration_date}). "
                            "Please regenerate certificates soon."
                        )
                    else:
                        logger.info(
                            f"SSL certificate valid until {expiration_date} ({days_remaining} days remaining)"
                        )

                import ssl

                from tornado import httpserver
                from tornado.ioloop import IOLoop
                from tornado.wsgi import WSGIContainer

                wsgi_container = WSGIContainer(server.app)  # type: ignore[arg-type]

                ssl_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                ssl_ctx.load_cert_chain(str(certfile), str(keyfile))
                ssl_ctx.check_hostname = False
                ssl_ctx.verify_mode = ssl.CERT_NONE

                http_server = httpserver.HTTPServer(wsgi_container, ssl_options=ssl_ctx)
                http_server.listen(port)
                logger.info(f"HTTPS server started on port {port}")
                ssl_enabled = True
                IOLoop.current().start()
            except Exception as ssl_error:
                logger.warning(
                    f"Failed to start HTTPS server: {ssl_error}. "
                    "Falling back to HTTP. "
                    "WARNING: Microphone access may not work without HTTPS."
                )
                ssl_enabled = False

        if not ssl_enabled:
            server.serve(port=port, debug=True)
    except KeyboardInterrupt:
        logger.info("Server stopped by keyboard interrupt")
        import _thread

        _thread.interrupt_main()
    except Exception as e:
        logger.error(f"Failed to run Flask server: {e}", exc_info=True)
        logger.info("Flask server thread running")
        import _thread

        _thread.interrupt_main()


def run_flask_in_thread(
    port: int,
    cwd: Path,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run the Flask server."""
    try:
        if _ENABLE_LOGGING:
            logger.info(f"Starting Flask server thread on port {port}")
            logger.info(f"Serving files from {cwd}")
            if certfile:
                logger.info(f"Using SSL certificate: {certfile}")
            if keyfile:
                logger.info(f"Using SSL key: {keyfile}")

        _run_flask_server(cwd, port, certfile, keyfile)
    except KeyboardInterrupt:
        logger.info("Flask server thread stopped by keyboard interrupt")
        import _thread

        _thread.interrupt_main()
        pass
    except Exception as e:
        logger.error(f"Error in Flask server thread: {e}", exc_info=True)


def run_flask_server_process(
    port: int,
    cwd: Path,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> Process:
    """Run the Flask server in a separate process."""
    if _ENABLE_LOGGING:
        logger.info(f"Starting Flask server process on port {port}")
        logger.info(f"Serving files from {cwd}")

    process = Process(
        target=run_flask_in_thread,
        args=(port, cwd, certfile, keyfile),
    )
    process.start()
    if _ENABLE_LOGGING:
        logger.info(f"Flask server process started with PID {process.pid}")
    return process


def parse_args() -> argparse.Namespace:
    """Parse the command line arguments."""
    parser = argparse.ArgumentParser(
        description="Open a browser to the fastled_js directory"
    )
    parser.add_argument(
        "fastled_js", type=Path, help="Path to the fastled_js directory"
    )
    parser.add_argument(
        "--port",
        "-p",
        type=int,
        required=True,
        help="Port to run the server on (default: %(default)s)",
    )
    parser.add_argument(
        "--certfile",
        type=Path,
        help="Path to the SSL certificate file for HTTPS",
    )
    parser.add_argument(
        "--keyfile",
        type=Path,
        help="Path to the SSL key file for HTTPS",
    )
    return parser.parse_args()


def main() -> None:
    """Main function."""
    if _ENABLE_LOGGING:
        logger.info("Starting main function")
    args = parse_args()
    if _ENABLE_LOGGING:
        logger.info(f"Arguments: port={args.port}, fastled_js={args.fastled_js}")
        if args.certfile:
            logger.info(f"Using SSL certificate: {args.certfile}")
        if args.keyfile:
            logger.info(f"Using SSL key: {args.keyfile}")

    run_flask_in_thread(args.port, args.fastled_js, args.certfile, args.keyfile)
    if _ENABLE_LOGGING:
        logger.info("Main function completed")


if __name__ == "__main__":
    main()
