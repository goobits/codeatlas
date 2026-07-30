from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse


if len(sys.argv) != 3:
    raise SystemExit("usage: server.py PORT OPENAPI_PATH")

port = int(sys.argv[1])
openapi = Path(sys.argv[2]).read_bytes()


class FixtureHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:
        path = urlparse(self.path).path
        if path == "/openapi.yaml":
            self._respond(200, openapi, "application/yaml")
            return
        if path == "/health":
            self._respond(204)
            return
        self._respond(404, b'{"error":"not_found"}')

    def do_POST(self) -> None:
        path = urlparse(self.path).path
        prefix = "/widgets/"
        if not path.startswith(prefix):
            self._respond(404, b'{"error":"not_found"}')
            return
        length = int(self.headers.get("content-length", "0"))
        try:
            body = json.loads(self.rfile.read(length))
        except (UnicodeDecodeError, json.JSONDecodeError):
            self._respond(400, b'{"error":"invalid_json"}')
            return
        if not isinstance(body, dict) or not isinstance(body.get("name"), str):
            self._respond(400, b'{"error":"invalid_body"}')
            return
        response = json.dumps(
            {"id": unquote(path[len(prefix) :]), "name": body["name"]},
            separators=(",", ":"),
        ).encode()
        self._respond(200, response)

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
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)


ThreadingHTTPServer(("127.0.0.1", port), FixtureHandler).serve_forever()
