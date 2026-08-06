#!/usr/bin/python3
"""Deterministic OCI CLI boundary fixture; it is never capability evidence outside tests."""

import json
import signal
import sys
from pathlib import Path


RUNTIME = Path(__file__)
STATE = RUNTIME.with_suffix(".state.json")
MODE = RUNTIME.with_suffix(".mode")
LOG = RUNTIME.with_suffix(".log")
STARTED = RUNTIME.with_suffix(".started")


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(2)


def load_state() -> dict:
    if not STATE.exists():
        return {"exists": False}
    return json.loads(STATE.read_text(encoding="utf-8"))


def save_state(state: dict) -> None:
    STATE.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")


def value_after(arguments: list[str], name: str) -> str:
    positions = [index for index, value in enumerate(arguments) if value == name]
    if len(positions) != 1 or positions[0] + 1 >= len(arguments):
        fail(f"missing unique {name}")
    return arguments[positions[0] + 1]


def values_after(arguments: list[str], name: str) -> list[str]:
    return [
        arguments[index + 1]
        for index, value in enumerate(arguments)
        if value == name and index + 1 < len(arguments)
    ]


def parse_mount(value: str) -> dict:
    fields: dict[str, str | bool] = {}
    for field in value.split(","):
        if "=" in field:
            name, item = field.split("=", 1)
            fields[name] = item
        else:
            fields[field] = True
    return {
        "Type": fields.get("type", ""),
        "Source": fields.get("src", ""),
        "Destination": fields.get("dst", ""),
        "RW": "readonly" not in fields,
        "Propagation": "rprivate",
    }


def parse_ulimit(value: str) -> dict:
    name, limits = value.split("=", 1)
    soft, hard = limits.split(":", 1)
    return {"Name": name, "Soft": int(soft), "Hard": int(hard)}


def create_container(arguments: list[str]) -> None:
    if "--pid" in arguments:
        fail("private PID isolation must use the OCI runtime default")
    environment = values_after(arguments, "--env")
    environment_map = dict(variable.split("=", 1) for variable in environment)
    mounts = [parse_mount(value) for value in values_after(arguments, "--mount")]
    workspace_destination = environment_map.get("CODEATLAS_WORKSPACE")
    workspaces = [
        mount for mount in mounts if mount["Destination"] == workspace_destination
    ]
    if len(workspaces) != 1:
        fail("missing unique disposable conformance workspace")
    sentinel_name = environment_map.get("CODEATLAS_WORKSPACE_SENTINEL")
    if not sentinel_name:
        fail("missing disposable conformance workspace sentinel name")
    sentinel = Path(workspaces[0]["Source"]) / sentinel_name
    sentinel_verified = (
        sentinel.is_file()
        and sentinel.read_text(encoding="utf-8")
        == environment_map.get("CODEATLAS_CONFORMANCE_NONCE")
    )
    if not sentinel_verified:
        fail("disposable conformance workspace sentinel is invalid")
    image = arguments[-2]
    mode_argument = arguments[-1]
    inspection = {
        "Config": {
            "Env": environment,
            "User": value_after(arguments, "--user"),
            "WorkingDir": value_after(arguments, "--workdir"),
            "Entrypoint": [value_after(arguments, "--entrypoint")],
            "Cmd": [mode_argument],
            "Image": image,
        },
        "HostConfig": {
            "ReadonlyRootfs": "--read-only" in arguments,
            "Privileged": False,
            "NetworkMode": value_after(arguments, "--network"),
            "PidMode": "",
            "IpcMode": value_after(arguments, "--ipc"),
            "CapAdd": [],
            "CapDrop": values_after(arguments, "--cap-drop"),
            "Devices": [],
            "SecurityOpt": values_after(arguments, "--security-opt"),
            "PidsLimit": int(value_after(arguments, "--pids-limit")),
            "Memory": int(value_after(arguments, "--memory")),
            "MemorySwap": int(value_after(arguments, "--memory-swap")),
            "Ulimits": [parse_ulimit(value) for value in values_after(arguments, "--ulimit")],
            "LogConfig": {"Type": value_after(arguments, "--log-driver")},
            "OomKillDisable": False,
        },
        "Mounts": mounts,
    }
    save_state(
        {
            "exists": True,
            "id": "c" * 64,
            "name": value_after(arguments, "--name"),
            "inspection": inspection,
            "sentinel_verified": sentinel_verified,
        }
    )
    print("c" * 64)


def conformance_report(state: dict) -> dict:
    environment = {
        name: value
        for name, value in (
            variable.split("=", 1)
            for variable in state["inspection"]["Config"]["Env"]
        )
    }
    checks = {
        "checkout_write_blocked": True,
        "runtime_write_blocked": True,
        "scratch_write_succeeded": True,
        "scratch_traversal_blocked": True,
        "scratch_symlink_escape_blocked": True,
        "home_write_confined": True,
        "temp_write_confined": True,
        "external_network_blocked": True,
        "unplanned_process_blocked": True,
        "ambient_environment_absent": True,
        "control_socket_absent": True,
        "unexpected_mount_absent": True,
        "cpu_limit_enforced": True,
        "rss_limit_enforced": True,
        "process_limit_enforced": True,
        "descriptor_limit_enforced": True,
    }
    mode = MODE.read_text(encoding="utf-8").strip() if MODE.exists() else "pass"
    failed_check = {
        "checkout-write": "checkout_write_blocked",
        "runtime-write": "runtime_write_blocked",
        "scratch-write": "scratch_write_succeeded",
        "traversal-escape": "scratch_traversal_blocked",
        "symlink-escape": "scratch_symlink_escape_blocked",
        "home-escape": "home_write_confined",
        "temp-escape": "temp_write_confined",
        "network-leak": "external_network_blocked",
        "process-leak": "unplanned_process_blocked",
        "ambient-environment": "ambient_environment_absent",
        "control-socket": "control_socket_absent",
        "unexpected-mount": "unexpected_mount_absent",
        "cpu-unbounded": "cpu_limit_enforced",
        "rss-unbounded": "rss_limit_enforced",
        "pid-unbounded": "process_limit_enforced",
        "fd-unbounded": "descriptor_limit_enforced",
    }.get(mode)
    if failed_check is not None:
        checks[failed_check] = False
    limits = {
        "cpu_time_ms": int(environment["CODEATLAS_LIMIT_CPU_TIME_MS"]),
        "rss_bytes": int(environment["CODEATLAS_LIMIT_RSS_BYTES"]),
        "processes": int(environment["CODEATLAS_LIMIT_PROCESSES"]),
        "open_files": int(environment["CODEATLAS_LIMIT_OPEN_FILES"]),
    }
    if mode == "limit-mismatch":
        limits["rss_bytes"] += 1
    nonce = environment["CODEATLAS_CONFORMANCE_NONCE"]
    if mode == "nonce-mismatch":
        nonce = "stale"
    report = {
        "schema_version": environment["CODEATLAS_CONFORMANCE_SCHEMA"],
        "nonce": nonce,
        "checks": checks,
        "limits": limits,
        "usage": {
            "cpu_time_ms": min(1, limits["cpu_time_ms"]),
            "peak_rss_bytes": min(4096, limits["rss_bytes"]),
            "peak_processes": min(1, limits["processes"]),
            "peak_open_files": min(4, limits["open_files"]),
        },
    }
    if mode == "unknown-field":
        report["invented"] = True
    return report


def main() -> None:
    arguments = sys.argv[1:]
    LOG.write_text(
        LOG.read_text(encoding="utf-8") + json.dumps(arguments) + "\n"
        if LOG.exists()
        else json.dumps(arguments) + "\n",
        encoding="utf-8",
    )
    if len(arguments) < 5 or arguments[0] != "--config" or arguments[2] != "--host":
        fail("invalid runtime preamble")
    command = arguments[4:]
    if command[0] == "version":
        print(
            json.dumps(
                {
                    "version": "29.0",
                    "api_version": "1.50",
                    "os": "linux",
                    "arch": "amd64",
                },
                separators=(",", ":"),
            )
        )
        return
    if command[0] == "info":
        print(
            json.dumps(
                {
                    "security_options": ["name=seccomp", "name=rootless"],
                    "cgroup_version": "2",
                    "driver": "overlay2",
                },
                separators=(",", ":"),
            )
        )
        return
    if command[:2] == ["image", "inspect"]:
        image = command[-1]
        print(json.dumps({"RepoDigests": [image], "Id": "sha256:" + "b" * 64}))
        return
    if command[:2] == ["container", "create"]:
        create_container(command)
        return
    state = load_state()
    if command[:2] == ["container", "inspect"]:
        if not state.get("exists"):
            raise SystemExit(1)
        print(json.dumps(state["inspection"], sort_keys=True))
        return
    if command[:2] == ["container", "start"]:
        if not state.get("exists"):
            raise SystemExit(1)
        mode = MODE.read_text(encoding="utf-8").strip() if MODE.exists() else "pass"
        STARTED.write_text("started", encoding="utf-8")
        if mode == "start-fail":
            raise SystemExit(9)
        if mode == "hang":
            while True:
                signal.pause()
        if mode == "output-exhausted":
            sys.stdout.write("x" * (2 * 1024 * 1024))
            sys.stdout.flush()
            return
        print(json.dumps(conformance_report(state), sort_keys=True))
        return
    if command[:2] == ["container", "rm"]:
        mode = MODE.read_text(encoding="utf-8").strip() if MODE.exists() else "pass"
        if mode == "cleanup-primary-fail" and not state.get("cleanup_failed"):
            state["cleanup_failed"] = True
            save_state(state)
            raise SystemExit(9)
        state["exists"] = False
        save_state(state)
        print(state.get("name", "missing"))
        return
    if command[:2] == ["container", "ls"]:
        if state.get("exists"):
            print(state.get("id", "c" * 64))
        return
    fail("unsupported fake runtime command")


if __name__ == "__main__":
    main()
