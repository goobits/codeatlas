#!/usr/bin/python3
"""Deterministic OCI CLI boundary fixture; it is never capability evidence outside tests."""

import base64
from contextlib import contextmanager
import json
import os
import signal
import socket
import ssl
import sys
import threading
import time
from pathlib import Path


RUNTIME = Path(__file__)
STATE = RUNTIME.with_suffix(".state.json")
MODE = RUNTIME.with_suffix(".mode")
LOG = RUNTIME.with_suffix(".log")
STARTED = RUNTIME.with_suffix(".started")
WORKLOAD_STARTED = RUNTIME.with_suffix(".workload-started")
TARGET_CALLS = RUNTIME.with_suffix(".target-calls")


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
    entrypoint = value_after(arguments, "--entrypoint")
    entrypoint_index = arguments.index("--entrypoint")
    image = arguments[entrypoint_index + 2]
    process_arguments = arguments[entrypoint_index + 3 :]
    if entrypoint == "/codeatlas/bin/isolation-conformance":
        kind = "probe"
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
    elif entrypoint == "/usr/local/bin/python3":
        kind = "workload"
        required_mounts = {
            "/codeatlas/workspace",
            "/codeatlas/runtime",
            "/codeatlas/scratch",
            "/tmp",
        }
        if not required_mounts.issubset(
            {mount["Destination"] for mount in mounts}
        ):
            fail("workload container is missing a required mount")
        sentinel_verified = True
    else:
        fail("unexpected container entrypoint")
    inspection = {
        "Config": {
            "Env": environment,
            "User": value_after(arguments, "--user"),
            "WorkingDir": value_after(arguments, "--workdir"),
            "Entrypoint": [entrypoint],
            "Cmd": process_arguments,
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
            "kind": kind,
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


def mount_source(state: dict, destination: str) -> Path:
    matches = [
        mount["Source"]
        for mount in state["inspection"]["Mounts"]
        if mount["Destination"] == destination
    ]
    if len(matches) != 1:
        fail(f"missing unique {destination} mount")
    return Path(matches[0])


def wait_for_path(path: Path, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while not path.exists():
        if time.monotonic() >= deadline:
            fail(f"timed out waiting for {path.name}")
        time.sleep(0.01)


@contextmanager
def unix_socket_address(path: Path):
    """Mirror the container's short mount path while the fake runtime runs on the host."""
    if not sys.platform.startswith("linux"):
        yield path
        return
    parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        yield Path(f"/proc/self/fd/{parent}") / path.name
    finally:
        os.close(parent)


def serve_managed_target(path: Path, stopped: threading.Event) -> None:
    if path.exists():
        path.unlink()
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    with unix_socket_address(path) as address:
        listener.bind(str(address))
    listener.settimeout(0.1)
    listener.listen()
    calls = 0
    try:
        while not stopped.is_set():
            try:
                connection, _ = listener.accept()
            except TimeoutError:
                continue
            with connection:
                request = bytearray()
                while b"\r\n\r\n" not in request:
                    chunk = connection.recv(4096)
                    if not chunk:
                        break
                    request.extend(chunk)
                calls += 1
                TARGET_CALLS.write_text(str(calls), encoding="utf-8")
                connection.sendall(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                )
    finally:
        listener.close()
        if path.exists():
            path.unlink()


def proxy_request(client_socket: Path, ca_path: Path) -> int:
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    with unix_socket_address(client_socket) as address:
        connection.connect(str(address))
    context = ssl.create_default_context(cafile=str(ca_path))
    with context.wrap_socket(connection, server_hostname="localhost") as secured:
        secured.sendall(
            b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:41001\r\n"
            b"X-CodeAtlas-Call-Category: generated_case\r\nConnection: close\r\n\r\n"
        )
        response = bytearray()
        while chunk := secured.recv(4096):
            response.extend(chunk)
    status_line = bytes(response).split(b"\r\n", 1)[0].split()
    if len(status_line) < 2:
        fail("proxy response has no status")
    return int(status_line[1])


def workload_secrets(runtime: Path) -> list[str]:
    secrets = []
    hooks_path = runtime / "secrets/request-hooks.json"
    if hooks_path.exists():
        hooks = json.loads(hooks_path.read_text(encoding="utf-8"))
        secrets.extend(
            header["value"]
            for header in hooks.get("headers", [])
            if isinstance(header.get("value"), str)
        )
    environment_path = runtime / "secrets/environment.json"
    if environment_path.exists():
        environment = json.loads(environment_path.read_text(encoding="utf-8"))
        secrets.extend(value for value in environment.values() if isinstance(value, str))
    return secrets


def write_workload_events(scratch: Path, status: int, secrets: list[str]) -> None:
    report = scratch / "reports/http"
    report.mkdir(parents=True, exist_ok=True)
    secret = secrets[0] if secrets else "fixture-public-value"
    events = [
        {"Initialize": {"seed": 42}},
        {
            "ScenarioFinished": {
                "phase": "Coverage",
                "status": "success",
                "is_final": False,
                "recorder": {
                    "label": "GET /health",
                    "cases": {
                        "positive": {
                            "value": {
                                "method": "GET",
                                "path": "/health",
                                "meta": {"generation": {"mode": "positive"}},
                            },
                            "is_transition_applied": False,
                        }
                    },
                    "checks": {"positive": [{"status": "success"}]},
                    "interactions": {
                        "positive": {
                            "request": {
                                "url": f"https://fixture.invalid/health?token={secret}",
                                "headers": {"Authorization": secret},
                                "body": secret,
                            },
                            "response": {"status_code": status},
                        }
                    },
                },
            }
        },
    ]
    (report / "events.ndjson").write_text(
        "".join(json.dumps(event, separators=(",", ":")) + "\n" for event in events),
        encoding="utf-8",
    )


def run_workload(state: dict, mode: str) -> None:
    scratch = mount_source(state, "/codeatlas/scratch")
    runtime = mount_source(state, "/codeatlas/runtime")
    protocol = json.loads((runtime / "workload.json").read_text(encoding="utf-8"))
    control = scratch / "control"
    transport = scratch / "transport"
    stopped = threading.Event()
    target = threading.Thread(
        target=serve_managed_target,
        args=(transport / "server.sock", stopped),
        daemon=True,
    )
    target.start()
    wait_for_path(transport / "server.sock")
    (control / "harness-ready").touch(mode=0o600)
    wait_for_path(control / "start-workload")
    WORKLOAD_STARTED.write_text("started", encoding="utf-8")
    if mode == "workload-hang":
        while True:
            signal.pause()
    wait_for_path(transport / "client.sock")
    statuses = [proxy_request(transport / "client.sock", runtime / "proxy-ca.pem")]
    if mode == "workload-budget":
        statuses.extend(
            proxy_request(transport / "client.sock", runtime / "proxy-ca.pem")
            for _ in range(5)
        )
    stopped.set()
    target.join(timeout=2.0)
    secrets = workload_secrets(runtime)
    write_workload_events(scratch, statuses[0], secrets)
    output = "fixture workload complete"
    if secrets:
        output += ":" + ":".join(secrets)
    result = {
        "schema_version": "codeatlas.execution-container-result/v1",
        "plan_id": protocol["plan_id"],
        "phase": "workload",
        "exit_code": 0,
        "reason": None,
        "output_exhausted": False,
        "output_base64": base64.b64encode(output.encode("utf-8")).decode("ascii"),
    }
    (control / "result.json").write_text(
        json.dumps(result, sort_keys=True, separators=(",", ":")),
        encoding="utf-8",
    )


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
        if command[2:4] == ["--format", "{{.State.Pid}}"]:
            pid = state.get("pid")
            if not isinstance(pid, int) or pid <= 0:
                fail("container peer PID is unavailable")
            print(pid)
            return
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
        if state.get("kind") == "workload":
            state["pid"] = os.getpid()
            save_state(state)
            run_workload(state, mode)
            return
        print(json.dumps(conformance_report(state), sort_keys=True))
        return
    if command[:2] == ["container", "rm"]:
        mode = MODE.read_text(encoding="utf-8").strip() if MODE.exists() else "pass"
        if mode == "workload-cleanup-total-fail" and state.get("kind") == "workload":
            raise SystemExit(9)
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
