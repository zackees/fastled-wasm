import argparse
import logging
import os
import time
from multiprocessing import Process
from pathlib import Path

import httpx
from livereload import Server

# Logging configuration
_ENABLE_LOGGING = os.environ.get("FLASK_SERVER_LOGGING", "0") == "1"


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


def _is_dwarf_source(path: str) -> bool:
    """Check if the path is a dwarf source file."""
    if "dwarfsource" in path:
        logger.debug(f"Path '{path}' contains 'dwarfsource'")
        return True
    # Check if the path starts with "fastledsource/" or "sketchsource/"
    return (
        path.startswith("fastledsource/")
        or path.startswith("sketchsource/")
        or path.startswith("/dwarfsource/")
        or path.startswith("dwarfsource/")
    )


def _run_flask_server(
    fastled_js: Path,
    port: int,
    compile_server_port: int,
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

        app = Flask(__name__)

        # Must be a full path or flask will fail to find the file.
        fastled_js = fastled_js.resolve()

        # logger.error(f"Server error: {e}")

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

        @app.route("/sourcefiles/<path:path>")
        def serve_source_files(path):
            """Proxy requests to /sourcefiles/* to the compile server"""
            from flask import request

            start_time = time.time()
            logger.info(f"Serving source file: {path}")

            # Forward the request to the compile server
            target_url = f"http://localhost:{compile_server_port}/sourcefiles/{path}"
            logger.info(f"Forwarding to: {target_url}")

            # Log request headers
            request_headers = {
                key: value for key, value in request.headers if key != "Host"
            }
            logger.debug(f"Request headers: {request_headers}")

            # Forward the request with the same method, headers, and body
            try:
                with httpx.Client() as client:
                    resp = client.request(
                        method=request.method,
                        url=target_url,
                        headers=request_headers,
                        content=request.get_data(),
                        cookies=request.cookies,
                        follow_redirects=True,
                    )

                logger.info(f"Response status: {resp.status_code}")
                logger.debug(f"Response headers: {dict(resp.headers)}")

                # Create a Flask Response object from the httpx response
                raw_data = resp.content
                logger.debug(f"Response size: {len(raw_data)} bytes")

                response = Response(
                    raw_data, status=resp.status_code, headers=dict(resp.headers)
                )

                elapsed_time = time.time() - start_time
                logger.info(f"Request completed in {elapsed_time:.3f} seconds")

                return response

            except Exception as e:
                logger.error(f"Error forwarding request: {e}", exc_info=True)
                return Response(f"Error: {str(e)}", status=500)

        def handle_fastledsource(path: str) -> Response:
            """Handle requests to
            /fastledsource/js/fastledsource/git/fastled/src/
            or
            /sketchsource/js/src/Blink.ino

            The names are a bit mangled due to the way C++ prefixing works near the root directory.
            """
            from flask import request

            start_time = time.time()
            logger.info(f"Processing request: {request.method} {request.url}")
            # Forward the request to the compile server
            target_url = f"http://localhost:{compile_server_port}/dwarfsource/{path}"
            logger.info(f"Requesting: {target_url}")
            logger.info(f"Processing dwarfsource request for {path}")

            # Log request headers
            request_headers = {
                key: value for key, value in request.headers if key != "Host"
            }
            logger.debug(f"Request headers: {request_headers}")

            try:
                # Forward the request with the same method, headers, and body
                with httpx.Client() as client:
                    resp = client.request(
                        method=request.method,
                        url=target_url,
                        headers=request_headers,
                        content=request.get_data(),
                        cookies=request.cookies,
                        follow_redirects=True,
                    )

                logger.info(f"Response status: {resp.status_code}")
                logger.debug(f"Response headers: {dict(resp.headers)}")

                # Create a Flask Response object from the httpx response
                payload = resp.content
                assert isinstance(payload, bytes)

                # Check if the payload is empty
                if len(payload) == 0:
                    logger.error("Empty payload received from compile server")
                    return Response("Empty payload", status=400)

                response = Response(
                    payload, status=resp.status_code, headers=dict(resp.headers)
                )

                elapsed_time = time.time() - start_time
                logger.info(f"Request completed in {elapsed_time:.3f} seconds")

                return response

            except Exception as e:
                logger.error(f"Error handling dwarfsource request: {e}", exc_info=True)
                return Response(f"Error: {str(e)}", status=500)

        def handle_sourcefile(path: str) -> Response:
            """Handle requests to /sourcefiles/*"""
            from flask import Response, request

            start_time = time.time()
            logger.info("\n##################################")
            logger.info(f"# Serving source file /sourcefiles/ {path}")
            logger.info("##################################\n")

            logger.info(f"Processing sourcefile request for {path}")

            # Forward the request to the compile server
            target_url = f"http://localhost:{compile_server_port}/{path}"
            logger.info(f"Forwarding to: {target_url}")

            # Log request headers
            request_headers = {
                key: value for key, value in request.headers if key != "Host"
            }
            logger.debug(f"Request headers: {request_headers}")

            try:
                # Forward the request with the same method, headers, and body
                with httpx.Client() as client:
                    resp = client.request(
                        method=request.method,
                        url=target_url,
                        headers=request_headers,
                        content=request.get_data(),
                        cookies=request.cookies,
                        follow_redirects=True,
                    )

                logger.info(f"Response status: {resp.status_code}")
                logger.debug(f"Response headers: {dict(resp.headers)}")

                # Create a Flask Response object from the httpx response
                raw_data = resp.content
                logger.debug(f"Response size: {len(raw_data)} bytes")

                response = Response(
                    raw_data, status=resp.status_code, headers=dict(resp.headers)
                )

                elapsed_time = time.time() - start_time
                logger.info(f"Request completed in {elapsed_time:.3f} seconds")

                return response

            except Exception as e:
                logger.error(f"Error handling sourcefile request: {e}", exc_info=True)
                return Response(f"Error: {str(e)}", status=500)

        def handle_local_file_fetch(path: str) -> Response:
            start_time = time.time()
            logger.info("\n##################################")
            logger.info(f"# Serving generic file {path}")
            logger.info("##################################\n")

            logger.info(f"Processing local file request for {path}")

            try:
                file_path = fastled_js / path
                logger.info(f"Full file path: {file_path}")
                logger.info(f"File exists: {file_path.exists()}")

                # Check if file exists before trying to serve it
                if not file_path.exists():
                    logger.warning(f"File not found: {file_path}")
                    return Response(f"File not found: {path}", status=404)

                response = send_from_directory(fastled_js, path)

                # Some servers don't set the Content-Type header for a bunch of files.
                content_type = None
                if path.endswith(".js"):
                    content_type = "application/javascript"
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

                # now also add headers to force no caching
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
                # Check if this is a FileNotFoundError (which can happen with send_from_directory)
                if isinstance(e, FileNotFoundError):
                    return Response(f"File not found: {path}", status=404)
                return Response(f"Error serving file: {str(e)}", status=500)

        @app.route("/fastapi")
        def server_backend_redirect():
            """Redirect to the compile server"""
            logger.info("Redirecting to compile server")
            target_url = f"http://localhost:{compile_server_port}/docs"
            logger.info(f"Redirecting to: {target_url}")
            return Response(
                f"Redirecting to compile server: <a href='{target_url}'>{target_url}</a>",
                status=302,
                headers={"Location": target_url},
            )

        @app.route("/<path:path>")
        def serve_files(path: str):
            logger.info(f"Received request for path: {path}")

            try:
                is_debug_src_code_request = _is_dwarf_source(path)
                logger.info(f"is debug_src_code_request: {is_debug_src_code_request}")
                if is_debug_src_code_request:
                    logger.info(f"Handling as drawfsource: {path}")
                    return handle_fastledsource(path)
                elif path.startswith("sourcefiles/"):
                    logger.info(f"Handling as sourcefiles: {path}")
                    return handle_sourcefile(path)
                else:
                    logger.info(f"Handling as local file: {path}")
                    return handle_local_file_fetch(path)
            except Exception as e:
                logger.error(f"Error in serve_files for {path}: {e}", exc_info=True)
                return Response(f"Server error: {str(e)}", status=500)

        @app.errorhandler(Exception)
        def handle_exception(e):
            """Log any uncaught exceptions"""
            logger.error(f"Unhandled exception: {e}", exc_info=True)
            return Response(f"Server error: {str(e)}", status=500)

        logger.info("Setting up livereload server")
        server = Server(app.wsgi_app)
        # Watch index.html for changes
        server.watch(str(fastled_js / "index.html"))
        # server.watch(str(fastled_js / "index.js"))
        # server.watch(str(fastled_js / "index.css"))
        # Start the server
        logger.info(f"Starting server on port {port}")
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
    compile_server_port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> None:
    """Run the Flask server."""
    try:
        if _ENABLE_LOGGING:
            logger.info(f"Starting Flask server thread on port {port}")
            logger.info(f"Serving files from {cwd}")
            logger.info(f"Compile server port: {compile_server_port}")
            if certfile:
                logger.info(f"Using SSL certificate: {certfile}")
            if keyfile:
                logger.info(f"Using SSL key: {keyfile}")

        _run_flask_server(cwd, port, compile_server_port, certfile, keyfile)
    except KeyboardInterrupt:
        logger.info("Flask server thread stopped by keyboard interrupt")
        import _thread

        _thread.interrupt_main()
        pass
    except Exception as e:
        logger.error(f"Error in Flask server thread: {e}", exc_info=True)


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


def run_flask_server_process(
    port: int,
    cwd: Path,
    compile_server_port: int,
    certfile: Path | None = None,
    keyfile: Path | None = None,
) -> Process:
    """Run the Flask server in a separate process."""
    if _ENABLE_LOGGING:
        logger.info(f"Starting Flask server process on port {port}")
        logger.info(f"Serving files from {cwd}")
        logger.info(f"Compile server port: {compile_server_port}")

    process = Process(
        target=run_flask_in_thread,
        args=(port, cwd, compile_server_port, certfile, keyfile),
    )
    process.start()
    if _ENABLE_LOGGING:
        logger.info(f"Flask server process started with PID {process.pid}")
    return process


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
        logger.warning("Note: main() is missing compile_server_port parameter")

    # Note: This call is missing the compile_server_port parameter
    # This is a bug in the original code
    run_flask_in_thread(args.port, args.fastled_js, 0, args.certfile, args.keyfile)
    if _ENABLE_LOGGING:
        logger.info("Main function completed")


if __name__ == "__main__":
    main()
