"""CodeAtlas bridge from Schemathesis requests to a portable JSONL adapter."""

from __future__ import annotations

import atexit
import base64
import http.client
import json
import os
import queue
import subprocess
import threading
from collections.abc import Mapping
from decimal import Decimal, InvalidOperation
from http.cookies import SimpleCookie
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

import requests
import schemathesis
from schemathesis.openapi.checks import IgnoredAuth
from schemathesis.specs.openapi._auth_retry import (
    build_retry_transport_kwargs,
    get_security_parameters,
    remove_auth,
    set_auth_for_case,
)
from schemathesis.specs.openapi.adapter.security import has_effective_optional_auth
from schemathesis.specs.openapi.checks import (
    ignored_auth as _schemathesis_ignored_auth,
    negative_data_rejection as _schemathesis_negative_data_rejection,
)


API_VERSION = "codeatlas.http-request-adapter/v2"
CONFIG_ENVIRONMENT_VARIABLE = "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG"
RESPONSE_TIMEOUT_SECONDS = 15
STARTUP_RESPONSE_TIMEOUT_SECONDS = 90
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
    def __init__(
        self,
        config: dict[str, Any],
        static_headers: tuple[tuple[str, str], ...],
    ) -> None:
        self._config = config
        self._static_headers = {
            name.lower(): value for name, value in static_headers
        }
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
        if any(
            _is_security_parameter_override(request, "header", name)
            for name in headers
        ):
            raise RuntimeError(
                "CodeAtlas request adapters must return declared credentials as authentication"
            )
        if headers and _is_negative_component(request, "header") and not all(
            self._is_static_credential_override(request, name)
            for name in headers
        ):
            raise RuntimeError(
                "CodeAtlas request adapters must preserve negatively generated headers "
                "except unchanged static credential placeholders"
            )
        if "bodyBase64" in value and not (
            isinstance(value["bodyBase64"], str) or value["bodyBase64"] is None
        ):
            raise RuntimeError("CodeAtlas request adapter returned an invalid body override")
        if "bodyBase64" in value and _is_negative_component(request, "body"):
            raise RuntimeError(
                "CodeAtlas request adapters must preserve negatively generated bodies"
            )
        authentication = value.get("authentication", [])
        if (
            not isinstance(authentication, list)
            or len(authentication) > 32
            or not all(
                isinstance(parameter, dict)
                and set(parameter) == {"in", "name", "value"}
                and parameter["in"] in {"cookie", "header", "query"}
                and isinstance(parameter["name"], str)
                and bool(parameter["name"])
                and isinstance(parameter["value"], str)
                and not any(
                    character in parameter["name"] + parameter["value"]
                    for character in "\r\n\0"
                )
                for parameter in authentication
            )
        ):
            raise RuntimeError(
                "CodeAtlas request adapter returned invalid authentication parameters"
            )
        if "query" in value:
            query = value["query"]
            if not isinstance(query, dict) or not all(
                isinstance(name, str)
                and bool(name)
                and not any(character in name for character in "\r\n\0")
                and (
                    query_value is None
                    or (
                        isinstance(query_value, str)
                        and not any(
                            character in query_value for character in "\r\n\0"
                        )
                    )
                    or (
                        isinstance(query_value, list)
                        and bool(query_value)
                        and all(
                            isinstance(item, str)
                            and not any(character in item for character in "\r\n\0")
                            for item in query_value
                        )
                    )
                )
                for name, query_value in query.items()
            ):
                raise RuntimeError(
                    "CodeAtlas request adapter returned invalid query overrides"
                )
            if _is_negative_component(request, "query") and not _preserves_negative_query(
                request, query
            ):
                raise RuntimeError(
                    "CodeAtlas request adapters must preserve negatively generated query parameters"
                )
            if any(
                _is_security_parameter_override(request, "query", name)
                for name in query
            ):
                raise RuntimeError(
                    "CodeAtlas request adapters must return declared credentials as authentication"
                )
        value["headers"] = headers
        value["authentication"] = authentication
        return value

    def _is_static_credential_override(
        self, request: dict[str, Any], name: str
    ) -> bool:
        expected = self._static_headers.get(name.lower())
        request_headers = request.get("headers")
        if expected is None or not isinstance(request_headers, dict):
            return False
        for request_name, request_value in request_headers.items():
            if not isinstance(request_name, str) or request_name.lower() != name.lower():
                continue
            if isinstance(request_value, list) and len(request_value) == 1:
                request_value = request_value[0]
            return request_value == expected
        return False

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
_ADAPTER = (
    _Adapter(_CONFIG["adapter"], _STATIC_HEADERS)
    if _CONFIG["adapter"] is not None
    else None
)
if _ADAPTER is not None:
    _ADAPTER.start()
    atexit.register(_ADAPTER.close)


@schemathesis.check
def codeatlas_auth_rejection(context: Any, response: Any, case: Any) -> bool | None:
    """Accept privacy-preserving auth rejection statuses in Schemathesis probes."""
    if _ADAPTER is not None:
        return _check_adapter_auth_rejection(context, response, case)
    try:
        return _schemathesis_ignored_auth(context, response, case)
    except IgnoredAuth as error:
        if "got `403 " in error.message or "got `404 " in error.message:
            return None
        raise


def _check_adapter_auth_rejection(
    context: Any, response: Any, case: Any
) -> bool | None:
    """Probe missing and invalid auth without reapplying adapter credentials."""
    if not 200 <= response.status_code < 300:
        return None
    operation = case.operation
    if has_effective_optional_auth(operation, operation.schema.raw_schema):
        return None
    security_parameters = get_security_parameters(operation)
    if not security_parameters:
        return None

    _send_auth_probe(context, case, security_parameters)
    for parameter in security_parameters:
        _send_auth_probe(context, case, security_parameters, parameter)
    return None


def _send_auth_probe(
    context: Any,
    case: Any,
    security_parameters: list[Mapping[str, Any]],
    invalid_parameter: Mapping[str, Any] | None = None,
) -> None:
    probe = remove_auth(case, security_parameters)
    if invalid_parameter is not None:
        set_auth_for_case(probe, invalid_parameter)
    kwargs = build_retry_transport_kwargs(
        context._transport_kwargs, security_parameters
    )
    # Preserve ordinary fixture adaptation and rotating static credentials, but
    # never reapply the declared credentials that this probe removed or changed.
    kwargs["auth"] = _RequestAdapterAuth(
        probe,
        apply_authentication=False,
        probe="authentication",
    )
    if case.operation.app is not None:
        kwargs.setdefault("app", case.operation.app)
    context._record_case(parent_id=case.id, case=probe)
    probe_response = case.operation.schema.transport.send(probe, **kwargs)
    context._record_response(case_id=probe.id, response=probe_response)
    _observe_response(probe, probe_response, probe="authentication")
    if probe_response.status_code in {401, 403, 404}:
        return

    scenario = "invalid" if invalid_parameter is not None else "missing"
    reason = http.client.responses.get(probe_response.status_code, "Unknown")
    raise IgnoredAuth(
        operation=case.operation.label,
        title=f"API accepts requests with {scenario} authentication",
        message=(
            f"Expected 401, 403, or 404, got `{probe_response.status_code} {reason}` "
            f"for `{case.operation.label}`"
        ),
        case_id=probe.id,
    )


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
    if _stateful_numeric_query_type_round_trip(case):
        return None
    return _schemathesis_negative_data_rejection(context, response, case)


@schemathesis.check
def codeatlas_no_internal_server_error(
    _context: Any, response: Any, _case: Any
) -> bool:
    """Reject unhandled HTTP 500 responses without treating intentional 5xx states as crashes."""
    return response.status_code != 500


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


def _stateful_numeric_query_type_round_trip(case: Any) -> bool:
    """Ignore type mutations that become valid numeric strings on the wire."""
    metadata = case.meta
    if metadata is None or _enum_value(getattr(metadata.phase, "name", None)) != "stateful":
        return False

    phase_data = metadata.phase.data
    if _enum_value(getattr(phase_data, "parameter_location", None)) != "query":
        return False
    mutations = getattr(phase_data, "mutations", ())
    if (
        len(mutations) != 1
        or _enum_value(getattr(mutations[0], "operator", None)) != "change_type"
    ):
        return False

    for component_location, component in metadata.components.items():
        if (
            _enum_value(component_location) != "query"
            and _enum_value(getattr(component, "mode", None)) == "negative"
        ):
            return False

    parameter_name = getattr(phase_data, "parameter", None)
    query = getattr(case, "query", None)
    operation_query = getattr(case.operation, "query", None)
    if (
        not isinstance(parameter_name, str)
        or not isinstance(query, dict)
        or operation_query is None
    ):
        return False
    value = query.get(parameter_name)
    parameter = operation_query.get(parameter_name)
    definition = getattr(parameter, "definition", None)
    schema = definition.get("schema") if isinstance(definition, dict) else None
    expected = schema.get("type") if isinstance(schema, dict) else None
    expected_types = {expected} if isinstance(expected, str) else set(expected or ())
    if not isinstance(value, str) or not expected_types.intersection({"integer", "number"}):
        return False

    try:
        numeric = Decimal(value)
    except InvalidOperation:
        return False
    if not numeric.is_finite():
        return False
    return "number" in expected_types or numeric == numeric.to_integral_value()


def _is_negative_component(request: dict[str, Any], component: str) -> bool:
    generation = request.get("generation")
    components = generation.get("components") if isinstance(generation, dict) else None
    return isinstance(components, dict) and components.get(component) == "negative"


def _preserves_negative_query(
    request: dict[str, Any], overrides: dict[str, Any]
) -> bool:
    generation = request.get("generation")
    parameters = (
        generation.get("negativeParameters")
        if isinstance(generation, dict)
        else None
    )
    query_parameters = parameters.get("query") if isinstance(parameters, dict) else None
    return (
        isinstance(query_parameters, list)
        and bool(query_parameters)
        and all(isinstance(name, str) and name for name in query_parameters)
        and not set(query_parameters).intersection(overrides)
    )


def _is_security_parameter_override(
    request: dict[str, Any], location: str, name: str
) -> bool:
    parameters = request.get("securityParameters")
    if not isinstance(parameters, list):
        return False
    if location == "header" and name.lower() == "cookie":
        return any(
            isinstance(parameter, dict) and parameter.get("in") == "cookie"
            for parameter in parameters
        )
    match_name = name.lower() if location == "header" else name
    return any(
        isinstance(parameter, dict)
        and parameter.get("in") == location
        and (
            str(parameter.get("name", "")).lower()
            if location == "header"
            else parameter.get("name")
        )
        == match_name
        for parameter in parameters
    )


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


def _public_security_parameters(case: Any) -> list[dict[str, str]]:
    return [
        {"in": parameter["in"], "name": parameter["name"]}
        for parameter in get_security_parameters(case.operation)
    ]


def _apply_authentication(
    prepared: requests.PreparedRequest,
    authentication: list[dict[str, str]],
    security_parameters: list[dict[str, str]],
) -> None:
    allowed = {
        (
            parameter["in"],
            parameter["name"].lower()
            if parameter["in"] == "header"
            else parameter["name"],
        )
        for parameter in security_parameters
    }
    for parameter in authentication:
        location = parameter["in"]
        name = parameter["name"]
        match_name = name.lower() if location == "header" else name
        if (location, match_name) not in allowed:
            raise RuntimeError(
                "CodeAtlas request adapter returned authentication that is not "
                "declared by the operation"
            )
        value = parameter["value"]
        if location == "header":
            prepared.headers[name] = value
        elif location == "cookie":
            cookies: SimpleCookie = SimpleCookie()
            cookies.load(prepared.headers.get("Cookie", ""))
            cookies[name] = value
            prepared.headers["Cookie"] = "; ".join(
                f"{key}={morsel.coded_value}" for key, morsel in cookies.items()
            )
            hostname = urlsplit(prepared.url).hostname
            if not hostname:
                raise RuntimeError(
                    "CodeAtlas cannot scope cookie authentication to a URL without a host"
                )
            prepared._cookies.set(name, value, domain=hostname, path="/")
        else:
            parts = urlsplit(prepared.url)
            query = [
                (key, query_value)
                for key, query_value in parse_qsl(
                    parts.query, keep_blank_values=True
                )
                if key != name
            ]
            query.append((name, value))
            prepared.url = urlunsplit(
                (parts.scheme, parts.netloc, parts.path, urlencode(query), parts.fragment)
            )


def _replace_query_values(
    url: str | None, overrides: dict[str, str | list[str] | None]
) -> str:
    if url is None:
        raise RuntimeError("CodeAtlas cannot adapt a request without a URL")
    try:
        parsed = urlsplit(url)
        overridden_names = set(overrides)
        query = [
            (name, value)
            for name, value in parse_qsl(parsed.query, keep_blank_values=True)
            if name not in overridden_names
        ]
        for name, value in overrides.items():
            if isinstance(value, list):
                query.extend((name, item) for item in value)
            elif value is not None:
                query.append((name, value))
        return urlunsplit(parsed._replace(query=urlencode(query)))
    except ValueError as error:
        raise RuntimeError(
            "CodeAtlas request adapter could not update the request query"
        ) from error


def _generation(case: Any) -> dict[str, Any]:
    metadata = case.meta
    if metadata is None:
        return {"mode": "unknown", "components": {}, "negativeParameters": {}}
    components = {
        location.value: component.mode.value
        for location, component in metadata.components.items()
    }
    phase_data = metadata.phase.data
    scenario = _enum_value(getattr(phase_data, "scenario", None))
    location = _enum_value(getattr(phase_data, "parameter_location", None))
    is_positive_coverage = scenario in POSITIVE_COVERAGE_SCENARIOS
    if is_positive_coverage and isinstance(location, str):
        components[location] = "positive"
    parameter = getattr(phase_data, "parameter", None)
    negative_parameters: dict[str, list[str]] = {}
    if (
        not is_positive_coverage
        and isinstance(location, str)
        and isinstance(parameter, str)
        and parameter
        and components.get(location) == "negative"
    ):
        negative_parameters[location] = [parameter]
    return {
        "mode": "positive" if is_positive_coverage else metadata.generation.mode.value,
        "components": components,
        "negativeParameters": negative_parameters,
    }


class _RequestAdapterAuth(requests.auth.AuthBase):
    def __init__(
        self,
        case: Any,
        *,
        apply_authentication: bool = True,
        probe: str | None = None,
    ) -> None:
        self._id = case.id
        self._operation = case.operation.label
        self._media_type = case.media_type
        self._generation = _generation(case)
        self._prior_auth = getattr(case, "_auth", None)
        self._security_parameters = _public_security_parameters(case)
        self._apply_authentication = apply_authentication
        self._probe = probe

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
            "securityParameters": self._security_parameters,
        }
        if self._probe is not None:
            request["probe"] = self._probe
        overrides = _ADAPTER.adapt(request)
        for name, value in overrides["headers"].items():
            prepared.headers[name] = value
        if "query" in overrides:
            prepared.url = _replace_query_values(prepared.url, overrides["query"])
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
        if self._apply_authentication:
            _apply_authentication(
                prepared,
                overrides["authentication"],
                self._security_parameters,
            )
        return prepared


@schemathesis.hook
def before_call(_context: Any, case: Any, kwargs: dict[str, Any]) -> None:
    if _ADAPTER is not None:
        kwargs["auth"] = _RequestAdapterAuth(case)


@schemathesis.hook
def after_call(_context: Any, case: Any, response: Any) -> None:
    _observe_response(case, response)


def _observe_response(case: Any, response: Any, *, probe: str | None = None) -> None:
    if _ADAPTER is None:
        return
    message = {
        "apiVersion": API_VERSION,
        "kind": "response",
        "id": case.id,
        "operation": case.operation.label,
        "status": response.status_code,
        "headers": dict(response.headers),
        "bodyBase64": base64.b64encode(response.content).decode("ascii"),
    }
    if probe is not None:
        message["probe"] = probe
    _ADAPTER.observe(message)
