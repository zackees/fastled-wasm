#!/usr/bin/env python3
"""
Reverse‐proxy wrapper using proxy.py.

By default this will:
  • Listen on plaintext port 9999 (--reader-port)
  • (Optionally) Listen on TLS port      (--ssl-port, --certfile, --keyfile)
  • Forward **all** incoming requests (HTTP, HTTPS, WS, WSS) to your local service on port 8921
"""

import argparse
import os
import sys


def main():
    parser = argparse.ArgumentParser(description="proxy.py reverse‐proxy wrapper")
    parser.add_argument(
        "--reader-port", type=int, default=9999, help="Plain HTTP/WebSocket proxy port"
    )
    parser.add_argument("--ssl-port", type=int, help="TLS proxy port (WSS/HTTPS)")
    parser.add_argument("--certfile", help="PEM cert for TLS listener")
    parser.add_argument("--keyfile", help="PEM key  for TLS listener")
    parser.add_argument(
        "--upstream-host", default="127.0.0.1", help="Host your service is running on"
    )
    parser.add_argument(
        "--upstream-port",
        type=int,
        default=8921,
        help="Port your service is running on",
    )
    args = parser.parse_args()

    # Write out a tiny plugin for proxy.py
    plugin_code = f"""\
import re
from proxy.plugin.reverse_proxy import ReverseProxyPlugin

class CustomReverseProxy(ReverseProxyPlugin):
    def routes(self):
        # catch everything and forward to upstream service
        return [
            (r'.*', [b'http://{args.upstream_host}:{args.upstream_port}']),
        ]
"""
    plugin_path = os.path.join(os.getcwd(), "custom_reverse_proxy.py")
    with open(plugin_path, "w") as fp:
        fp.write(plugin_code)

    # Ensure proxy.py can import our plugin
    os.environ.setdefault("PYTHONPATH", os.getcwd())

    # Build the proxy CLI invocation
    cmd = [
        "proxy",
        "--hostname",
        "0.0.0.0",
        "--disable-http-proxy",  # disable CONNECT/TUNNEL proxy mode
        "--enable-web-server",  # enable HTTP/WebSocket reverse proxy
        "--enable-reverse-proxy",  # turn on reverse‐proxy core
        "--plugins",
        "custom_reverse_proxy.CustomReverseProxy",
    ]

    # plaintext listener
    if args.reader_port:
        cmd += ["--port", str(args.reader_port)]

    # optional TLS listener
    if args.ssl_port:
        if not (args.certfile and args.keyfile):
            print(
                "Error: --ssl-port requires --certfile and --keyfile", file=sys.stderr
            )
            sys.exit(1)
        cmd += [
            "--port",
            str(args.ssl_port),
            "--certfile",
            args.certfile,
            "--keyfile",
            args.keyfile,
        ]

    print("▶️  Starting proxy.py with:")
    print("    " + " \\\n    ".join(cmd))
    os.execvp(cmd[0], cmd)


if __name__ == "__main__":
    main()
