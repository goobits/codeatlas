#!/usr/local/bin/python3
"""Shared private runtime primitives for CodeAtlas callable harnesses."""

from __future__ import annotations

import json
import os
import socket
import stat
from contextlib import contextmanager
from pathlib import Path
from typing import Any


RESULT_SCHEMA = "codeatlas.code-fuzz-harness-result/v1"
RESULT_PATH = "control/code-result.json"
PERMIT_SCHEMA = "codeatlas.execution-call-permit/v1"


class PermitDenied(Exception):
    pass


def _read_line(stream: Any) -> dict[str, Any]:
    line = stream.readline(513)
    if not line or len(line) > 512 or not line.endswith(b"\n") or b"\r" in line:
        raise RuntimeError("call-permit response is not one bounded line")
    value = json.loads(line)
    if not isinstance(value, dict) or value.get("schema_version") != PERMIT_SCHEMA:
        raise RuntimeError("call-permit response has the wrong schema")
    return value


@contextmanager
def call_permit(category: str):
    address = os.environ.get("CODEATLAS_CALL_PERMIT_SOCKET")
    if not address:
        raise RuntimeError("call-permit socket is unavailable")
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.connect(address)
    stream = connection.makefile("rwb", buffering=0)
    stream.write(
        json.dumps(
            {"schema_version": PERMIT_SCHEMA, "category": category},
            sort_keys=True,
            separators=(",", ":"),
        ).encode()
        + b"\n"
    )
    response = _read_line(stream)
    if response.get("status") == "denied":
        reason = response.get("reason", "unknown")
        stream.close()
        connection.close()
        raise PermitDenied(str(reason))
    if set(response) != {"schema_version", "status", "sequence"} or response.get(
        "status"
    ) != "granted":
        raise RuntimeError("call-permit grant is invalid")
    disposition = ["completed"]

    def set_disposition(value: str) -> None:
        if value not in {"completed", "failed", "rejected", "cancelled"}:
            raise ValueError("invalid call-permit disposition")
        disposition[0] = value

    try:
        yield set_disposition
    except BaseException:
        disposition[0] = "failed"
        raise
    finally:
        try:
            stream.write(
                json.dumps(
                    {"schema_version": PERMIT_SCHEMA, "disposition": disposition[0]},
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode()
                + b"\n"
            )
            acknowledgement = _read_line(stream)
            if (
                acknowledgement.get("status") != "recorded"
                or acknowledgement.get("sequence") != response.get("sequence")
            ):
                raise RuntimeError("call-permit completion was not recorded")
        finally:
            stream.close()
            connection.close()


def write_result(result: dict[str, Any]) -> None:
    scratch = Path(os.environ["CODEATLAS_SCRATCH"])
    final = scratch / RESULT_PATH
    root = final.parent
    try:
        mode = root.lstat().st_mode
    except FileNotFoundError as error:
        raise RuntimeError("kernel-created result directory is unavailable") from error
    if not stat.S_ISDIR(mode):
        raise RuntimeError("kernel-created result directory is not a directory")
    temporary = final.with_suffix(".json.tmp")
    with temporary.open("xb") as handle:
        os.chmod(temporary, 0o600)
        handle.write(
            json.dumps(
                result,
                ensure_ascii=False,
                allow_nan=False,
                sort_keys=True,
                separators=(",", ":"),
            ).encode("utf-8")
        )
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, final)
