from __future__ import annotations

from flask import Blueprint, Response, jsonify, make_response, request

from fastled.debug_symbols import DebugSymbolResolver


def create_debug_blueprint(
    resolver: DebugSymbolResolver | None,
) -> Blueprint:
    debug = Blueprint("fastled_debug", __name__)

    @debug.post("/dwarfsource")
    def dwarfsource() -> Response:
        if resolver is None:
            return make_response(
                jsonify({"error": "debug source resolver unavailable"}), 400
            )

        payload = request.get_json(silent=True) or {}
        request_path = payload.get("path")
        if not isinstance(request_path, str) or not request_path.strip():
            return make_response(jsonify({"error": "missing path"}), 400)

        try:
            source_path = resolver.resolve(request_path, check_exists=True)
            return Response(source_path.read_text(), mimetype="text/plain")
        except FileNotFoundError as exc:
            return make_response(jsonify({"error": str(exc)}), 404)
        except ValueError as exc:
            return make_response(jsonify({"error": str(exc)}), 400)

    @debug.get("/debug/source-roots")
    def source_roots() -> Response:
        if resolver is None:
            return jsonify({"roots": []})

        return jsonify(
            {
                "roots": [
                    {"prefix": prefix, "path": str(path)}
                    for prefix, path in resolver.config.source_roots
                ]
            }
        )

    return debug
