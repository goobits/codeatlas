#!/usr/local/bin/python3
"""Private Hypothesis adapter for one exact CodeAtlas callable plan."""

from __future__ import annotations

import asyncio
import base64
import importlib
import json
import math
import os
import random
import signal
import sys
from pathlib import Path
from typing import Any, Callable

from hypothesis import Phase, find, settings
from hypothesis import strategies as st
from hypothesis.errors import NoSuchExample
import hypothesis

from runtime_support import PermitDenied, RESULT_SCHEMA, call_permit, write_result


ADAPTER_SCHEMA = "codeatlas.python-fuzz-strategy/v1"
EXPECTED_HYPOTHESIS = "6.165.2"
STRATEGY_FIELDS = {
    "schema_version",
    "target_id",
    "callable_id",
    "module",
    "symbol",
    "is_async",
    "signature",
    "dimensions",
    "deterministic_prefix",
    "seed",
    "max_cases",
    "max_shrinks",
    "max_failures",
    "case_timeout_ms",
    "alternate_behavior",
    "replay_input",
}


class CaseTimeout(Exception):
    pass


def require_strategy(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != STRATEGY_FIELDS:
        raise ValueError("Python fuzz strategy is not strict")
    if value["schema_version"] != ADAPTER_SCHEMA:
        raise ValueError("unsupported Python fuzz strategy")
    for field in ("target_id", "callable_id", "module", "symbol", "seed"):
        if not isinstance(value[field], str) or not value[field]:
            raise ValueError(f"Python fuzz strategy {field} is invalid")
    for field in (
        "max_cases",
        "max_shrinks",
        "max_failures",
        "case_timeout_ms",
    ):
        if not isinstance(value[field], int) or isinstance(value[field], bool) or value[field] <= 0:
            raise ValueError(f"Python fuzz strategy {field} is invalid")
    if not isinstance(value["signature"], dict) or not isinstance(value["dimensions"], list):
        raise ValueError("Python fuzz signature evidence is invalid")
    if not isinstance(value["deterministic_prefix"], list):
        raise ValueError("Python deterministic prefix is invalid")
    if not isinstance(value["is_async"], bool) or not isinstance(
        value["alternate_behavior"], bool
    ):
        raise ValueError("Python fuzz booleans are invalid")
    if value["replay_input"] is not None and not isinstance(value["replay_input"], list):
        raise ValueError("Python replay input is invalid")
    int(value["seed"])
    return value


def integer_bounds(semantic_type: dict[str, Any]) -> tuple[int | None, int | None]:
    bits = semantic_type.get("bits")
    signed = semantic_type.get("signed")
    if bits is None:
        return None, None
    if signed is False:
        return 0, (1 << bits) - 1
    return -(1 << (bits - 1)), (1 << (bits - 1)) - 1


def literal_value(literal: dict[str, Any]) -> Any:
    kind = literal["kind"]
    value = literal.get("value")
    if kind == "boolean":
        return bool(value)
    if kind == "integer":
        return int(value)
    if kind == "float":
        return float(value)
    if kind == "string":
        return str(value)
    if kind == "null":
        return None
    raise ValueError("unsupported semantic literal")


def text_boundary(point: dict[str, Any], maximum: int | None) -> str:
    boundary = point["point"]
    if isinstance(boundary, dict):
        boundary = boundary["length"]
    if boundary == "empty":
        return ""
    if boundary == "one":
        return "a"
    if boundary == "ascii":
        return "CodeAtlas"
    if boundary == "unicode":
        return "🧭"
    if boundary == "combining":
        return "e\u0301"
    if boundary == "below_maximum":
        return "a" * max(0, (maximum or 1) - 1)
    if boundary == "maximum":
        return "a" * (maximum or 1)
    if boundary == "above_maximum":
        return "a" * ((maximum or 0) + 1)
    raise ValueError(f"unsupported text boundary {boundary}")


def materialize_point(semantic_type: dict[str, Any], point: dict[str, Any]) -> Any:
    kind = semantic_type["kind"]
    if kind in {"unit", "null"}:
        return None
    if kind == "literal":
        return literal_value(semantic_type["value"])
    if kind == "boolean":
        return bool(point["value"])
    if kind == "integer":
        minimum, maximum = integer_bounds(semantic_type)
        boundary = point["point"]
        values = {
            "minimum": minimum,
            "above_minimum": None if minimum is None else minimum + 1,
            "negative_one": -1,
            "zero": 0,
            "one": 1,
            "below_maximum": None if maximum is None else maximum - 1,
            "maximum": maximum,
        }
        value = values[boundary]
        if value is None:
            raise ValueError("unbounded integer requested a bounded edge")
        return value
    if kind == "float":
        boundary = point["point"]
        finite = 3.4028235e38 if semantic_type.get("bits") == 32 else sys.float_info.max
        return {
            "negative_infinity": -math.inf,
            "negative_finite_extreme": -finite,
            "negative_one": -1.0,
            "negative_zero": -0.0,
            "positive_zero": 0.0,
            "one": 1.0,
            "positive_finite_extreme": finite,
            "positive_infinity": math.inf,
            "nan": math.nan,
        }[boundary]
    if kind == "string":
        return text_boundary(point, semantic_type.get("max_length"))
    if kind == "bytes":
        boundary = point["point"]
        maximum = semantic_type.get("max_length")
        size = {
            "empty": 0,
            "one": 1,
            "below_maximum": max(0, (maximum or 1) - 1),
            "maximum": maximum or 1,
            "above_maximum": (maximum or 0) + 1,
        }[boundary]
        return b"a" * size
    raise ValueError(f"unsupported Python semantic type {kind}")


def encode_value(semantic_type: dict[str, Any], value: Any) -> dict[str, Any]:
    kind = semantic_type["kind"]
    if kind in {"unit", "null"} or value is None:
        return {"kind": "null"}
    if kind == "literal":
        kind = semantic_type["value"]["kind"]
    if kind == "boolean":
        return {"kind": "boolean", "value": bool(value)}
    if kind == "integer":
        return {"kind": "integer", "value": str(value)}
    if kind == "float":
        if math.isnan(value):
            encoded = "nan"
        elif math.isinf(value):
            encoded = "-infinity" if value < 0 else "infinity"
        elif value == 0 and math.copysign(1.0, value) < 0:
            encoded = "-0"
        else:
            encoded = repr(float(value))
        return {"kind": "float", "value": encoded}
    if kind == "string":
        return {"kind": "string", "value": value}
    if kind == "bytes":
        return {"kind": "bytes", "base64": base64.b64encode(value).decode("ascii")}
    raise ValueError(f"unsupported Python input envelope {kind}")


def decode_value(envelope: dict[str, Any]) -> Any:
    kind = envelope["kind"]
    if kind == "null":
        return None
    if kind == "boolean":
        return bool(envelope["value"])
    if kind == "integer":
        return int(envelope["value"])
    if kind == "float":
        special = {
            "nan": math.nan,
            "infinity": math.inf,
            "-infinity": -math.inf,
            "-0": -0.0,
        }
        return special[envelope["value"]] if envelope["value"] in special else float(
            envelope["value"]
        )
    if kind == "string":
        return envelope["value"]
    if kind == "bytes":
        return base64.b64decode(envelope["base64"], validate=True)
    raise ValueError("unsupported replay input envelope")


def deterministic_inputs(strategy: dict[str, Any]) -> list[list[dict[str, Any]]]:
    parameters = strategy["signature"]["parameters"]
    dimensions = {dimension["path"]: dimension for dimension in strategy["dimensions"]}
    inputs = []
    for selected in strategy["deterministic_prefix"]:
        indexes = {
            dimension["path"]: selected[index]
            for index, dimension in enumerate(strategy["dimensions"])
        }
        values = []
        for parameter in parameters:
            path = f"parameter:{parameter['position']}"
            point = dimensions[path]["points"][indexes[path]]
            value = materialize_point(parameter["semantic_type"], point)
            values.append(encode_value(parameter["semantic_type"], value))
        inputs.append(values)
    return inputs


def hypothesis_strategy(semantic_type: dict[str, Any]):
    kind = semantic_type["kind"]
    if kind in {"unit", "null"}:
        return st.none()
    if kind == "literal":
        return st.just(literal_value(semantic_type["value"]))
    if kind == "boolean":
        return st.booleans()
    if kind == "integer":
        minimum, maximum = integer_bounds(semantic_type)
        return st.integers(min_value=minimum, max_value=maximum)
    if kind == "float":
        return st.floats(
            width=semantic_type.get("bits") or 64,
            allow_nan=semantic_type["allows_special"],
            allow_infinity=semantic_type["allows_special"],
        )
    if kind == "string":
        return st.text(max_size=semantic_type.get("max_length") or 64)
    if kind == "bytes":
        return st.binary(max_size=semantic_type.get("max_length") or 64)
    raise ValueError(f"unsupported Hypothesis type {kind}")


def result_shape(semantic_type: dict[str, Any], value: Any) -> bool:
    kind = semantic_type["kind"]
    if kind in {"unit", "null"}:
        return value is None
    if kind == "literal":
        return value == literal_value(semantic_type["value"])
    if kind == "boolean":
        return type(value) is bool
    if kind == "integer":
        if type(value) is not int:
            return False
        minimum, maximum = integer_bounds(semantic_type)
        return (minimum is None or value >= minimum) and (maximum is None or value <= maximum)
    if kind == "float":
        return type(value) is float and (
            semantic_type["allows_special"] or math.isfinite(value)
        )
    if kind == "string":
        maximum = semantic_type.get("max_length")
        return isinstance(value, str) and (maximum is None or len(value) <= maximum)
    if kind == "bytes":
        maximum = semantic_type.get("max_length")
        return isinstance(value, bytes) and (maximum is None or len(value) <= maximum)
    return False


def timeout_handler(_signum: int, _frame: Any) -> None:
    raise CaseTimeout("case timeout")


def failure(kind: str, envelopes: list[dict[str, Any]], detail: Any) -> dict[str, Any]:
    return {
        "kind": kind,
        "input": envelopes,
        "detail": detail,
        "minimized": False,
    }


def load_target(strategy: dict[str, Any]) -> Callable[..., Any]:
    module = importlib.import_module(strategy["module"])
    target = getattr(module, strategy["symbol"])
    if not callable(target):
        raise TypeError("resolved Python fuzz target is not callable")
    return target


def evaluate(
    strategy: dict[str, Any],
    target: Callable[..., Any],
    envelopes: list[dict[str, Any]],
    category: str,
) -> dict[str, Any] | None:
    arguments = [decode_value(envelope) for envelope in envelopes]
    with call_permit(category) as set_disposition:
        previous = signal.signal(signal.SIGALRM, timeout_handler)
        signal.setitimer(signal.ITIMER_REAL, strategy["case_timeout_ms"] / 1000)
        try:
            result = target(*arguments)
            if strategy["is_async"]:
                result = asyncio.run(result)
            if not result_shape(strategy["signature"]["result"], result):
                set_disposition("failed")
                return failure(
                    "result_shape",
                    envelopes,
                    {"actual_type": type(result).__qualname__},
                )
            return None
        except CaseTimeout:
            set_disposition("failed")
            return failure("timeout", envelopes, {"timeout": True})
        except BaseException as error:
            set_disposition("failed")
            return failure(
                "panic_or_crash", envelopes, {"exception": type(error).__qualname__}
            )
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)


def run(strategy: dict[str, Any]) -> dict[str, Any]:
    if os.environ.get("CODEATLAS_FUZZ") != "1":
        raise RuntimeError("planned fuzz marker is unavailable")
    plan_id = os.environ.get("CODEATLAS_PLAN_ID")
    if not plan_id:
        raise RuntimeError("planned execution identity is unavailable")
    with call_permit("readiness"):
        target = load_target(strategy)
    failures = []
    deterministic_count = 0
    adaptive_count = 0
    retry_count = 0

    if strategy["replay_input"] is not None:
        replay = strategy["replay_input"]
        observed = evaluate(strategy, target, replay, "generated_case")
        adaptive_count = 1
        if observed is not None:
            failures.append(observed)
    else:
        for envelopes in deterministic_inputs(strategy):
            if len(failures) >= strategy["max_failures"]:
                break
            observed = evaluate(strategy, target, envelopes, "generated_case")
            deterministic_count += 1
            if observed is not None:
                if retry_count >= strategy["max_failures"]:
                    break
                retry_count += 1
                confirmation = evaluate(strategy, target, envelopes, "retry")
                if confirmation is not None and confirmation["kind"] == observed["kind"]:
                    failures.append(confirmation)

        remaining = max(0, strategy["max_cases"] - deterministic_count)
        if not failures and remaining and retry_count < strategy["max_failures"]:
            parameter_types = [
                parameter["semantic_type"]
                for parameter in strategy["signature"]["parameters"]
            ]
            tuple_strategy = st.tuples(
                *(hypothesis_strategy(item) for item in parameter_types)
            )
            first_failure = False
            last_failure: dict[str, Any] | None = None

            def predicate(values: tuple[Any, ...]) -> bool:
                nonlocal adaptive_count, first_failure, last_failure
                envelopes = [
                    encode_value(item, value)
                    for item, value in zip(parameter_types, values, strict=True)
                ]
                category = "reduction" if first_failure else "generated_case"
                observed = evaluate(strategy, target, envelopes, category)
                if category == "generated_case":
                    adaptive_count += 1
                if observed is not None:
                    first_failure = True
                    last_failure = observed
                    return True
                return False

            try:
                found = find(
                    tuple_strategy,
                    predicate,
                    settings=settings(
                        max_examples=remaining,
                        phases=(Phase.generate, Phase.shrink),
                        database=None,
                        deadline=None,
                        derandomize=False,
                    ),
                    random=random.Random(int(strategy["seed"])),
                )
                envelopes = [
                    encode_value(item, value)
                    for item, value in zip(parameter_types, found, strict=True)
                ]
                retry_count += 1
                confirmation = evaluate(strategy, target, envelopes, "retry")
                if confirmation is not None:
                    confirmation["minimized"] = True
                    failures.append(confirmation)
            except NoSuchExample:
                pass
            except PermitDenied:
                if last_failure is not None:
                    failures.append(last_failure)

    return {
        "schema_version": RESULT_SCHEMA,
        "plan_id": plan_id,
        "target_id": strategy["target_id"],
        "callable_id": strategy["callable_id"],
        "seed": strategy["seed"],
        "deterministic_cases": deterministic_count,
        "adaptive_cases": adaptive_count,
        "alternate_behavior": strategy["alternate_behavior"],
        "failures": failures[: strategy["max_failures"]],
    }


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--version":
        if hypothesis.__version__ != EXPECTED_HYPOTHESIS:
            return 1
        print(EXPECTED_HYPOTHESIS)
        return 0
    if len(sys.argv) != 2:
        return 2
    strategy = require_strategy(Path(sys.argv[1]))
    result = run(strategy)
    write_result(result)
    return 1 if result["failures"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
