#!/usr/local/bin/python3
"""Private CodeAtlas driver for one generated Rust callable harness."""

from __future__ import annotations

import glob
import json
import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Any

from runtime_support import PermitDenied, RESULT_SCHEMA, call_permit, write_result


ADAPTER_SCHEMA = "codeatlas.rust-fuzz-strategy/v1"
EXPECTED_PROPTEST = "1.11.0"
RUNTIME_ROOT = Path("/codeatlas/runtime/code-fuzz/rust")
SCRATCH_ROOT = Path("/codeatlas/scratch/code-fuzz-rust")
RESULT_PATH = Path(os.environ.get("CODEATLAS_SCRATCH", "/unavailable")) / "control/code-result.json"
CURRENT_INPUT = Path(os.environ.get("CODEATLAS_SCRATCH", "/unavailable")) / "control/rust-current-input.json"
PROJECT_FILES = ("Cargo.toml", "Cargo.lock", "src/main.rs")
STRATEGY_FIELDS = {
    "schema_version",
    "target_id",
    "callable_id",
    "seed",
    "alternate_behavior",
    "cargo",
}
COMMAND_FIELDS = {
    "owner",
    "executable",
    "arguments",
    "working_directory",
    "environment",
    "secret_environment_file",
}
EXPECTED_ENVIRONMENT = {
    "CARGO_HOME": "/usr/local/cargo",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_TARGET_DIR": "/codeatlas/scratch/code-fuzz-rust/target",
    "CARGO_TERM_COLOR": "never",
    "RUST_BACKTRACE": "0",
    "RUSTUP_HOME": "/usr/local/rustup",
}
EXPECTED_ARGUMENTS = [
    "build",
    "--offline",
    "--quiet",
    "--manifest-path",
    "/codeatlas/scratch/code-fuzz-rust/Cargo.toml",
]
MAX_RUNTIME_FILE_BYTES = 16 * 1024 * 1024
MAX_RESULT_BYTES = 16 * 1024 * 1024


def read_json(path: Path, maximum: int) -> Any:
    if not path.is_file() or path.is_symlink() or path.stat().st_size > maximum:
        raise ValueError(f"private Rust fuzz file is unavailable or exceeds its ceiling: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def require_strategy(path: Path) -> dict[str, Any]:
    value = read_json(path, MAX_RUNTIME_FILE_BYTES)
    if not isinstance(value, dict) or set(value) != STRATEGY_FIELDS:
        raise ValueError("Rust fuzz strategy does not match its strict schema")
    if value["schema_version"] != ADAPTER_SCHEMA:
        raise ValueError("unsupported Rust fuzz strategy schema")
    for field in ("target_id", "callable_id", "seed"):
        if not isinstance(value[field], str) or not value[field]:
            raise ValueError(f"Rust fuzz strategy {field} is invalid")
    if not value["seed"].isdigit() or not isinstance(value["alternate_behavior"], bool):
        raise ValueError("Rust fuzz strategy metadata is invalid")
    require_cargo(value["cargo"])
    return value


def require_cargo(value: Any) -> None:
    if not isinstance(value, dict) or set(value) != COMMAND_FIELDS:
        raise ValueError("Rust fuzz Cargo command does not match the managed-command contract")
    expected = {
        "owner": "code_fuzz_rust_cargo",
        "executable": "/usr/local/cargo/bin/cargo",
        "arguments": EXPECTED_ARGUMENTS,
        "working_directory": str(SCRATCH_ROOT),
        "environment": EXPECTED_ENVIRONMENT,
        "secret_environment_file": None,
    }
    if value != expected:
        raise ValueError("Rust fuzz Cargo command is not the exact adapter-owned command")


def verify_engine() -> None:
    manifests = [
        Path(path)
        for path in glob.glob(
            f"/usr/local/cargo/registry/src/*/proptest-{EXPECTED_PROPTEST}/Cargo.toml"
        )
    ]
    if len(manifests) != 1:
        raise RuntimeError("the workload image does not contain one exact proptest source")
    manifest = manifests[0]
    if manifest.stat().st_size > MAX_RUNTIME_FILE_BYTES:
        raise RuntimeError("the pinned proptest manifest exceeds its ceiling")
    source = manifest.read_text(encoding="utf-8")
    if 'name = "proptest"' not in source or f'version = "{EXPECTED_PROPTEST}"' not in source:
        raise RuntimeError("the workload image proptest identity does not match the plan")


def copy_project() -> None:
    SCRATCH_ROOT.mkdir(mode=0o700, parents=False, exist_ok=True)
    for relative in PROJECT_FILES:
        source = RUNTIME_ROOT / relative
        metadata = source.lstat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_RUNTIME_FILE_BYTES:
            raise RuntimeError(f"Rust fuzz runtime file is not one bounded regular file: {relative}")
        destination = SCRATCH_ROOT / relative
        destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with source.open("rb") as reader, destination.open("xb") as writer:
            os.chmod(destination, 0o600)
            shutil.copyfileobj(reader, writer, length=64 * 1024)


def empty_result(strategy: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_version": RESULT_SCHEMA,
        "plan_id": os.environ["CODEATLAS_PLAN_ID"],
        "target_id": strategy["target_id"],
        "callable_id": strategy["callable_id"],
        "seed": strategy["seed"],
        "deterministic_cases": 0,
        "adaptive_cases": 0,
        "alternate_behavior": strategy["alternate_behavior"],
        "failures": [],
    }


def run(strategy: dict[str, Any]) -> int:
    if os.environ.get("CODEATLAS_FUZZ") != "1" or not os.environ.get("CODEATLAS_PLAN_ID"):
        raise RuntimeError("planned Rust fuzz execution evidence is unavailable")
    verify_engine()
    copy_project()
    cargo = strategy["cargo"]
    try:
        with call_permit("readiness") as set_disposition:
            compiled = subprocess.run(
                [cargo["executable"], *cargo["arguments"]],
                cwd=cargo["working_directory"],
                env=dict(os.environ),
                stdin=subprocess.DEVNULL,
                check=False,
            )
            if compiled.returncode != 0:
                set_disposition("failed")
    except PermitDenied:
        write_result(empty_result(strategy))
        return 1
    if compiled.returncode != 0:
        write_result(empty_result(strategy))
        return 1

    executable = SCRATCH_ROOT / "target/debug/codeatlas-rust-fuzz-harness"
    metadata = executable.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o111 == 0:
        raise RuntimeError("Cargo did not produce the exact Rust fuzz harness executable")
    completed = subprocess.run(
        [str(executable)],
        cwd=SCRATCH_ROOT,
        env=dict(os.environ),
        stdin=subprocess.DEVNULL,
        check=False,
    )
    if RESULT_PATH.is_file() and not RESULT_PATH.is_symlink():
        if RESULT_PATH.stat().st_size > MAX_RESULT_BYTES:
            raise RuntimeError("Rust fuzz result exceeds the private driver ceiling")
        return completed.returncode

    result = empty_result(strategy)
    if CURRENT_INPUT.is_file() and not CURRENT_INPUT.is_symlink():
        current = read_json(CURRENT_INPUT, MAX_RESULT_BYTES)
        if not isinstance(current, dict) or set(current) != {"input"} or not isinstance(current["input"], list):
            raise RuntimeError("Rust fuzz crash input does not match its strict envelope")
        result["failures"] = [
            {
                "kind": "panic_or_crash",
                "input": current["input"],
                "detail": {"process_exit": completed.returncode},
                "minimized": False,
            }
        ]
    write_result(result)
    return 1


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--version":
        verify_engine()
        print(EXPECTED_PROPTEST)
        return 0
    if len(sys.argv) != 2:
        return 2
    return run(require_strategy(Path(sys.argv[1])))


if __name__ == "__main__":
    raise SystemExit(main())
