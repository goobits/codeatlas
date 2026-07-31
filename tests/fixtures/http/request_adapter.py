"""Deterministic JSONL request-adapter fixture for the managed fuzzer smoke."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


API_VERSION = "codeatlas.http-request-adapter/v1"

if len(sys.argv) != 2:
    raise SystemExit("usage: request_adapter.py AUDIT_LOG")

audit_log = Path(sys.argv[1])


def append_audit(entry: dict[str, Any]) -> None:
    with audit_log.open("a", encoding="utf-8") as output:
        output.write(json.dumps(entry, separators=(",", ":")) + "\n")


def header_value(headers: dict[str, Any], name: str) -> Any:
    value = headers.get(name)
    if isinstance(value, list) and len(value) == 1:
        return value[0]
    return value


try:
    for line in sys.stdin:
        message = json.loads(line)
        kind = message.get("kind")
        message_id = message.get("id")
        if message.get("apiVersion") != API_VERSION or kind not in {
            "request",
            "response",
        }:
            raise RuntimeError("invalid CodeAtlas fixture adapter message")

        reply: dict[str, Any] = {
            "apiVersion": API_VERSION,
            "kind": kind,
            "id": message_id,
        }
        if kind == "request":
            headers = {
                name.lower(): value
                for name, value in message.get("headers", {}).items()
            }
            components = message.get("generation", {}).get("components", {})
            negative_header = components.get("header") == "negative"
            negative_body = components.get("body") == "negative"
            has_body = message.get("bodyBase64") is not None
            reply["headers"] = (
                {} if negative_header else {"X-CodeAtlas-Adapter": "fixture-adapter"}
            )
            if has_body and not negative_body:
                reply["bodyBase64"] = message["bodyBase64"]
            append_audit(
                {
                    "bodyGeneration": components.get("body"),
                    "bodyOverride": has_body and not negative_body,
                    "headerGeneration": components.get("header"),
                    "headerOverride": not negative_header,
                    "id": message_id,
                    "kind": kind,
                    "staticHeader": header_value(headers, "x-codeatlas-static")
                    == "fixture-static-token",
                }
            )
        else:
            headers = {
                name.lower(): value
                for name, value in message.get("headers", {}).items()
            }
            append_audit(
                {
                    "adapterSeen": header_value(headers, "x-codeatlas-adapter-seen")
                    == "true",
                    "id": message_id,
                    "kind": kind,
                    "status": message.get("status"),
                }
            )

        print(json.dumps(reply, separators=(",", ":")), flush=True)
finally:
    append_audit({"kind": "closed"})
