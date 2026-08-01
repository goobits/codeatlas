from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse


if len(sys.argv) not in {3, 4}:
    raise SystemExit("usage: server.py PORT OPENAPI_PATH [STATIC_HEADER]")

port = int(sys.argv[1])
openapi = Path(sys.argv[2]).read_bytes()
required_static_header = sys.argv[3] if len(sys.argv) == 4 else None


class FixtureHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:
        if not self._has_static_header():
            self._respond(401, b'{"error":"missing_static_header"}')
            return
        path = urlparse(self.path).path
        if path == "/openapi.yaml":
            self._respond(200, openapi, "application/yaml")
            return
        if path == "/health":
            self._respond(204)
            return
        if path.startswith("/widgets/"):
            response = json.dumps(
                {"id": unquote(path[len("/widgets/") :]), "name": "fixture"},
                separators=(",", ":"),
            ).encode()
            self._respond(200, response)
            return
        self._respond(404, b'{"error":"not_found"}')

    def do_POST(self) -> None:
        if not self._has_static_header():
            self._respond(401, b'{"error":"missing_static_header"}')
            return
        path = urlparse(self.path).path
        prefix = "/widgets/"
        if path == "/health":
            self._method_not_allowed("GET")
            return
        if not path.startswith(prefix):
            self._respond(404, b'{"error":"not_found"}')
            return
        length = int(self.headers.get("content-length", "0"))
        try:
            body = json.loads(self.rfile.read(length))
        except (UnicodeDecodeError, json.JSONDecodeError):
            self._respond(400, b'{"error":"invalid_json"}')
            return
        if (
            not isinstance(body, dict)
            or set(body) != {"name"}
            or not isinstance(body.get("name"), str)
            or len(body["name"]) > 128
        ):
            self._respond(400, b'{"error":"invalid_body"}')
            return
        response = json.dumps(
            {"id": unquote(path[len(prefix) :]), "name": body["name"]},
            separators=(",", ":"),
        ).encode()
        self._respond(200, response)

    def do_DELETE(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def do_HEAD(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def do_OPTIONS(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def do_PATCH(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def do_PUT(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def do_TRACE(self) -> None:
        self._method_not_allowed(self._allowed_method())

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _respond(
        self,
        status: int,
        body: bytes = b"",
        content_type: str = "application/json",
    ) -> None:
        self.send_response(status)
        if body:
            self.send_header("content-type", content_type)
        self.send_header(
            "x-codeatlas-adapter-seen",
            str(self.headers.get("x-codeatlas-adapter") == "fixture-adapter").lower(),
        )
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def _allowed_method(self) -> str:
        return "GET, POST" if urlparse(self.path).path.startswith("/widgets/") else "GET"

    def _has_static_header(self) -> bool:
        return required_static_header is None or (
            self.headers.get("x-codeatlas-static") == required_static_header
        )

    def _method_not_allowed(self, allowed: str) -> None:
        length = int(self.headers.get("content-length", "0"))
        if length:
            self.rfile.read(length)
        self.send_response(405)
        self.send_header("allow", allowed)
        self.send_header(
            "x-codeatlas-adapter-seen",
            str(self.headers.get("x-codeatlas-adapter") == "fixture-adapter").lower(),
        )
        self.send_header("content-length", "0")
        self.end_headers()


ThreadingHTTPServer(("127.0.0.1", port), FixtureHandler).serve_forever()
