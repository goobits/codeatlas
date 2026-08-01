"""CodeAtlas bridge from Schemathesis requests to a portable JSONL adapter."""

from __future__ import annotations

import atexit
import base64
import json
import os
import queue
import subprocess
import threading
from typing import Any

import requests
import schemathesis
from schemathesis.openapi.checks import IgnoredAuth
from schemathesis.specs.openapi.checks import (
    ignored_auth as _schemathesis_ignored_auth,
    negative_data_rejection as _schemathesis_negative_data_rejection,
)


API_VERSION = "codeatlas.http-request-adapter/v1"
CONFIG_ENVIRONMENT_VARIABLE = "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG"
RESPONSE_TIMEOUT_SECONDS = 5
STARTUP_RESPONSE_TIMEOUT_SECONDS = 30
POSITIVE_COVERAGE_SCENARIOS = frozenset(
    {
        "const_value",
        "default_positive_test",
        "default_value",
        "enum_value",
        "enum_value_items_array",
        "example_value",
        "maximum_items_array",
        "maximum_length_string",
        "maximum_value",
        "minimum_items_array",
        "minimum_length_string",
        "minimum_value",
        "near_boundary_items_array",
        "near_boundary_length_string",
        "near_boundary_number",
        "null_value",
        "object_additional_property",
        "object_only_required",
        "object_required_and_optional",
        "valid_array",
        "valid_boolean",
        "valid_number",
        "valid_object",
        "valid_string",
    }
)


def _read_config() -> dict[str, Any]:
    config_path = os.environ.pop(CONFIG_ENVIRONMENT_VARIABLE, None)
    if not config_path:
        raise RuntimeError(f"{CONFIG_ENVIRONMENT_VARIABLE} is required")
    try:
        with open(config_path, "r", encoding="utf-8") as config_file:
            value = json.load(config_file)
    except (OSError, ValueError) as error:
        raise RuntimeError("CodeAtlas request hook configuration is invalid") from error
    if not isinstance(value, dict) or value.get("apiVersion") != API_VERSION:
        raise RuntimeError(f"{CONFIG_ENVIRONMENT_VARIABLE} must use {API_VERSION}")
    headers = value.get("headers")
    if not isinstance(headers, list) or not all(
        isinstance(header, dict)
        and isinstance(header.get("name"), str)
        and bool(header["name"])
        and isinstance(header.get("value"), str)
        and not any(
            character in header["name"] + header["value"]
            for character in "\r\n\0"
        )
        for header in headers
    ):
        raise RuntimeError("CodeAtlas static request headers are invalid")
    adapter = value.get("adapter")
    if adapter is not None:
        if not isinstance(adapter, dict):
            raise RuntimeError("CodeAtlas request adapter configuration must be an object")
        if not isinstance(adapter.get("command"), str) or not adapter["command"]:
            raise RuntimeError(
                "CodeAtlas request adapter command must be a non-empty string"
            )
        if not isinstance(adapter.get("args"), list) or not all(
            isinstance(argument, str) for argument in adapter["args"]
        ):
            raise RuntimeError("CodeAtlas request adapter args must be strings")
        if not isinstance(adapter.get("cwd"), str) or not adapter["cwd"]:
            raise RuntimeError("CodeAtlas request adapter cwd must be a non-empty string")
    return value


class _Adapter:
    def __init__(self, config: dict[str, Any]) -> None:
        self._config = config
        self._lock = threading.Lock()
        self._responses: queue.Queue[str | None] = queue.Queue()
        self._process: subprocess.Popen[str] | None = None
        self._ready = False

    def start(self) -> subprocess.Popen[str]:
        if self._process is not None:
            return self._process
        responses: queue.Queue[str | None] = queue.Queue()
        process = subprocess.Popen(
            [self._config["command"], *self._config["args"]],
            cwd=self._config["cwd"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self._ready = False
        self._responses = responses
        self._process = process
        threading.Thread(
            target=self._read_responses,
            args=(process, responses),
            daemon=True,
        ).start()
        return process

    def adapt(self, request: dict[str, Any]) -> dict[str, Any]:
        value = self._exchange(request)
        headers = value.get("headers", {})
        if not isinstance(headers, dict) or not all(
            isinstance(name, str)
            and isinstance(header_value, str)
            and name
            and not any(character in name + header_value for character in "\r\n\0")
            for name, header_value in headers.items()
        ):
            raise RuntimeError("CodeAtlas request adapter returned invalid headers")
        if headers and _is_negative_component(request, "header"):
            raise RuntimeError(
                "CodeAtlas request adapters must preserve negatively generated headers"
            )
        if "bodyBase64" in value and not (
            isinstance(value["bodyBase64"], str) or value["bodyBase64"] is None
        ):
            raise RuntimeError("CodeAtlas request adapter returned an invalid body override")
        if "bodyBase64" in value and _is_negative_component(request, "body"):
            raise RuntimeError(
                "CodeAtlas request adapters must preserve negatively generated bodies"
            )
        value["headers"] = headers
        return value

    def observe(self, response: dict[str, Any]) -> None:
        self._exchange(response)

    def _exchange(self, message: dict[str, Any]) -> dict[str, Any]:
        with self._lock:
            process = self.start()
            assert process.stdin is not None
            try:
                process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
                process.stdin.flush()
            except (BrokenPipeError, OSError) as error:
                raise RuntimeError("CodeAtlas request adapter closed its input") from error
            timeout = (
                RESPONSE_TIMEOUT_SECONDS
                if self._ready
                else STARTUP_RESPONSE_TIMEOUT_SECONDS
            )
            try:
                line = self._responses.get(timeout=timeout)
            except queue.Empty as error:
                self.close()
                raise RuntimeError(
                    f"CodeAtlas request adapter did not respond within {timeout} seconds"
                ) from error
            if line is None:
                status = process.poll()
                raise RuntimeError(
                    f"CodeAtlas request adapter exited before replying (status {status})"
                )
            value = self._parse_response(line, message["kind"], message["id"])
            self._ready = True
            return value

    @staticmethod
    def _read_responses(
        process: subprocess.Popen[str], responses: queue.Queue[str | None]
    ) -> None:
        assert process.stdout is not None
        for line in process.stdout:
            responses.put(line)
        responses.put(None)

    @staticmethod
    def _parse_response(line: str, kind: str, message_id: str) -> dict[str, Any]:
        value = json.loads(line)
        if (
            not isinstance(value, dict)
            or value.get("apiVersion") != API_VERSION
            or value.get("kind") != kind
            or value.get("id") != message_id
        ):
            raise RuntimeError("CodeAtlas request adapter returned an invalid response envelope")
        return value

    def close(self) -> None:
        process = self._process
        self._process = None
        if process is None:
            return
        if process.stdin is not None:
            try:
                process.stdin.close()
            except OSError:
                pass
        try:
            process.wait(timeout=0.5)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


_CONFIG = _read_config()
_STATIC_HEADERS = tuple(
    (header["name"], header["value"]) for header in _CONFIG["headers"]
)
_ADAPTER = _Adapter(_CONFIG["adapter"]) if _CONFIG["adapter"] is not None else None
if _ADAPTER is not None:
    _ADAPTER.start()
    atexit.register(_ADAPTER.close)


@schemathesis.check
def codeatlas_auth_rejection(context: Any, response: Any, case: Any) -> bool | None:
    """Accept privacy-preserving auth rejection statuses in Schemathesis probes."""
    try:
        return _schemathesis_ignored_auth(context, response, case)
    except IgnoredAuth as error:
        if "got `403 " in error.message or "got `404 " in error.message:
            return None
        raise


@schemathesis.check
def codeatlas_negative_data_rejection(
    context: Any, response: Any, case: Any
) -> bool | None:
    """Do not require rejection of undeclared non-body parameters."""
    metadata = case.meta
    phase_data = metadata.phase.data if metadata is not None else None
    scenario = _enum_value(getattr(phase_data, "scenario", None))
    location = _enum_value(getattr(phase_data, "parameter_location", None))
    if scenario in POSITIVE_COVERAGE_SCENARIOS:
        return None
    if scenario == "object_unexpected_properties" and location in {
        "cookie",
        "header",
        "query",
    }:
        return None
    return _schemathesis_negative_data_rejection(context, response, case)


class _CodeAtlasAuthProvider:
    def get(self, _case: Any, _context: Any) -> tuple[tuple[str, str], ...]:
        return _STATIC_HEADERS

    def set(
        self,
        case: Any,
        headers: tuple[tuple[str, str], ...],
        _context: Any,
    ) -> None:
        case.headers = case.headers or {}
        for name, value in headers:
            case.headers[name] = value


if _STATIC_HEADERS:
    schemathesis.auth(refresh_interval=None, retry_on=[])(_CodeAtlasAuthProvider)


def _enum_value(value: Any) -> Any:
    return getattr(value, "value", value)


def _is_negative_component(request: dict[str, Any], component: str) -> bool:
    generation = request.get("generation")
    components = generation.get("components") if isinstance(generation, dict) else None
    return isinstance(components, dict) and components.get(component) == "negative"


def _prepared_body(value: Any) -> bytes | None:
    if value is None:
        return None
    if isinstance(value, bytes):
        return value
    if isinstance(value, str):
        return value.encode("utf-8")
    raise RuntimeError(
        f"CodeAtlas request adapters do not support streaming body type {type(value).__name__}"
    )


def _generation(case: Any) -> dict[str, Any]:
    metadata = case.meta
    if metadata is None:
        return {"mode": "unknown", "components": {}}
    return {
        "mode": metadata.generation.mode.value,
        "components": {
            location.value: component.mode.value
            for location, component in metadata.components.items()
        },
    }


class _RequestAdapterAuth(requests.auth.AuthBase):
    def __init__(self, case: Any) -> None:
        self._id = case.id
        self._operation = case.operation.label
        self._media_type = case.media_type
        self._generation = _generation(case)
        self._prior_auth = getattr(case, "_auth", None)

    def __call__(self, prepared: requests.PreparedRequest) -> requests.PreparedRequest:
        if self._prior_auth is not None:
            if not callable(self._prior_auth):
                raise RuntimeError("CodeAtlas cannot compose a non-callable request authenticator")
            prepared = self._prior_auth(prepared)
        assert _ADAPTER is not None
        body = _prepared_body(prepared.body)
        request = {
            "apiVersion": API_VERSION,
            "kind": "request",
            "id": self._id,
            "operation": self._operation,
            "method": prepared.method,
            "url": prepared.url,
            "headers": dict(prepared.headers),
            "bodyBase64": None if body is None else base64.b64encode(body).decode("ascii"),
            "mediaType": self._media_type,
            "generation": self._generation,
        }
        overrides = _ADAPTER.adapt(request)
        for name, value in overrides["headers"].items():
            prepared.headers[name] = value
        if "bodyBase64" in overrides:
            encoded = overrides["bodyBase64"]
            try:
                prepared.body = (
                    None
                    if encoded is None
                    else base64.b64decode(encoded, validate=True)
                )
            except ValueError as error:
                raise RuntimeError(
                    "CodeAtlas request adapter returned malformed base64 body data"
                ) from error
            prepared.headers.pop("Content-Length", None)
            prepared.prepare_content_length(prepared.body)
        return prepared


@schemathesis.hook
def before_call(_context: Any, case: Any, kwargs: dict[str, Any]) -> None:
    if _ADAPTER is not None:
        kwargs["auth"] = _RequestAdapterAuth(case)


@schemathesis.hook
def after_call(_context: Any, case: Any, response: Any) -> None:
    if _ADAPTER is None:
        return
    _ADAPTER.observe(
        {
            "apiVersion": API_VERSION,
            "kind": "response",
            "id": case.id,
            "operation": case.operation.label,
            "status": response.status_code,
            "headers": dict(response.headers),
            "bodyBase64": base64.b64encode(response.content).decode("ascii"),
        }
    )
