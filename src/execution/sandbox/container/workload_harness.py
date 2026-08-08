#!/usr/local/bin/python3
"""Private, no-shell supervisor for one CodeAtlas container workload."""

from __future__ import annotations

import asyncio
import base64
import json
import os
import signal
import sys
from pathlib import Path
from typing import Any


PROTOCOL_SCHEMA = "codeatlas.execution-container-workload/v2"
RESULT_SCHEMA = "codeatlas.execution-container-result/v1"
SCRATCH = Path(os.environ["CODEATLAS_SCRATCH"])
READY = SCRATCH / "control/harness-ready"
START = SCRATCH / "control/start-workload"
RESULT = SCRATCH / "control/result.json"
CLIENT_PROXY_SOCKET = SCRATCH / "transport/client.sock"
MANAGED_SERVER_SOCKET = SCRATCH / "transport/server.sock"
BASE_ENVIRONMENT = (
    "CODEATLAS_CALL_PERMIT_SOCKET",
    "CODEATLAS_FUZZ",
    "CODEATLAS_PLAN_ID",
    "CODEATLAS_SCRATCH",
    "HOME",
    "PATH",
    "TMPDIR",
    "XDG_CACHE_HOME",
)
MAX_READINESS_ATTEMPTS = 1024
RESERVED_ENVIRONMENT = {
    "CODEATLAS_WORKSPACE",
    *BASE_ENVIRONMENT,
}
TOP_LEVEL_FIELDS = {
    "schema_version",
    "plan_id",
    "engine_version",
    "engine_probe_arguments",
    "prepare",
    "delegated",
    "service",
    "workload",
    "client_proxy",
    "managed_server",
    "call_permit",
    "fuzz_marker",
    "startup_timeout_ms",
    "max_output_bytes",
}
COMMAND_FIELDS = {
    "owner",
    "executable",
    "arguments",
    "working_directory",
    "environment",
    "secret_environment_file",
}


def require_object(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != fields:
        raise ValueError(f"{label} does not match the strict workload protocol")
    return value


def require_command(value: Any, label: str) -> dict[str, Any]:
    command = require_object(value, COMMAND_FIELDS, label)
    if not isinstance(command["owner"], str) or not command["owner"]:
        raise ValueError(f"{label} owner is invalid")
    if not isinstance(command["executable"], str) or not command["executable"].startswith("/"):
        raise ValueError(f"{label} executable is invalid")
    if not isinstance(command["arguments"], list) or not all(
        isinstance(argument, str) for argument in command["arguments"]
    ):
        raise ValueError(f"{label} arguments are invalid")
    if not isinstance(command["working_directory"], str) or not command[
        "working_directory"
    ].startswith("/"):
        raise ValueError(f"{label} working directory is invalid")
    if not isinstance(command["environment"], dict) or not all(
        isinstance(name, str)
        and name not in RESERVED_ENVIRONMENT
        and isinstance(item, str)
        for name, item in command["environment"].items()
    ):
        raise ValueError(f"{label} environment is invalid")
    secret_environment_file = command["secret_environment_file"]
    if secret_environment_file is not None and (
        not isinstance(secret_environment_file, str)
        or not secret_environment_file.startswith("/codeatlas/runtime/secrets/")
    ):
        raise ValueError(f"{label} secret environment file is invalid")
    return command


def require_port(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or not 0 < value <= 65535:
        raise ValueError(f"{label} is invalid")
    return value


def load_protocol(path: Path) -> dict[str, Any]:
    protocol = require_object(
        json.loads(path.read_text(encoding="utf-8")), TOP_LEVEL_FIELDS, "workload"
    )
    if protocol["schema_version"] != PROTOCOL_SCHEMA:
        raise ValueError("unsupported workload protocol schema")
    if not isinstance(protocol["plan_id"], str) or not protocol["plan_id"].startswith("plan_"):
        raise ValueError("workload plan ID is invalid")
    if os.environ.get("CODEATLAS_PLAN_ID") != protocol["plan_id"]:
        raise ValueError("workload plan environment does not match the protocol")
    if not isinstance(protocol["engine_version"], str) or not protocol["engine_version"]:
        raise ValueError("workload engine version is invalid")
    if (
        not isinstance(protocol["engine_probe_arguments"], list)
        or not protocol["engine_probe_arguments"]
        or len(protocol["engine_probe_arguments"]) > 32
        or not all(
            isinstance(argument, str)
            and "\x00" not in argument
            and "\n" not in argument
            and "\r" not in argument
            for argument in protocol["engine_probe_arguments"]
        )
    ):
        raise ValueError("workload engine probe is invalid")
    if not isinstance(protocol["prepare"], list):
        raise ValueError("workload prepare list is invalid")
    protocol["prepare"] = [
        require_command(command, f"prepare[{index}]")
        for index, command in enumerate(protocol["prepare"])
    ]
    if not isinstance(protocol["delegated"], list):
        raise ValueError("workload delegated command list is invalid")
    protocol["delegated"] = [
        require_command(command, f"delegated[{index}]")
        for index, command in enumerate(protocol["delegated"])
    ]
    if protocol["service"] is not None:
        protocol["service"] = require_command(protocol["service"], "service")
    protocol["workload"] = require_command(protocol["workload"], "workload")
    if protocol["client_proxy"] is not None:
        bridge = require_object(
            protocol["client_proxy"], {"listen_port", "socket"}, "client proxy"
        )
        if bridge["socket"] != "/codeatlas/scratch/transport/client.sock":
            raise ValueError("client proxy socket is invalid")
        require_port(bridge["listen_port"], "client proxy listen port")
    if protocol["managed_server"] is not None:
        bridge = require_object(
            protocol["managed_server"], {"socket", "target_port"}, "managed server"
        )
        if bridge["socket"] != "/codeatlas/scratch/transport/server.sock":
            raise ValueError("managed server socket is invalid")
        require_port(bridge["target_port"], "managed server target port")
        if protocol["service"] is None:
            raise ValueError("managed server requires a service command")
    if protocol["call_permit"] is not None:
        bridge = require_object(protocol["call_permit"], {"socket"}, "call permit")
        if bridge["socket"] != "/codeatlas/scratch/transport/permit.sock":
            raise ValueError("call permit socket is invalid")
    if not isinstance(protocol["fuzz_marker"], bool):
        raise ValueError("workload fuzz marker is invalid")
    if protocol["fuzz_marker"] and protocol["call_permit"] is None:
        raise ValueError("workload fuzz marker requires call permits")
    if (
        protocol["client_proxy"] is not None
        and protocol["managed_server"] is not None
        and protocol["client_proxy"]["listen_port"]
        == protocol["managed_server"]["target_port"]
    ):
        raise ValueError("workload bridge ports must be distinct")
    for field in ("startup_timeout_ms", "max_output_bytes"):
        if not isinstance(protocol[field], int) or isinstance(protocol[field], bool) or protocol[field] <= 0:
            raise ValueError(f"workload {field} is invalid")
    return protocol


class OutputBudget:
    def __init__(self, maximum: int) -> None:
        self.maximum = maximum
        self.output = bytearray()
        self.exhausted = asyncio.Event()

    async def capture(self, stream: asyncio.StreamReader) -> None:
        while chunk := await stream.read(8192):
            remaining = self.maximum - len(self.output)
            self.output.extend(chunk[:remaining])
            if len(chunk) > remaining:
                self.exhausted.set()
                return


def command_environment(command: dict[str, Any]) -> dict[str, str]:
    environment = {
        name: os.environ[name] for name in BASE_ENVIRONMENT if name in os.environ
    }
    environment.update(command["environment"])
    if path := command["secret_environment_file"]:
        secret_environment = json.loads(Path(path).read_text(encoding="utf-8"))
        if not isinstance(secret_environment, dict) or not all(
            isinstance(name, str)
            and name not in RESERVED_ENVIRONMENT
            and isinstance(value, str)
            for name, value in secret_environment.items()
        ):
            raise ValueError("secret environment is invalid")
        environment.update(secret_environment)
    return environment


async def start_command(
    command: dict[str, Any], budget: OutputBudget
) -> tuple[asyncio.subprocess.Process, asyncio.Task[None]]:
    process = await asyncio.create_subprocess_exec(
        command["executable"],
        *command["arguments"],
        cwd=command["working_directory"],
        env=command_environment(command),
        stdin=asyncio.subprocess.DEVNULL,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
        start_new_session=True,
    )
    if process.stdout is None:
        raise RuntimeError("workload command has no captured output")
    return process, asyncio.create_task(budget.capture(process.stdout))


async def stop_process(process: asyncio.subprocess.Process) -> None:
    if process.returncode is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        await asyncio.wait_for(process.wait(), timeout=1.0)
    except asyncio.TimeoutError:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        await process.wait()


async def run_command(
    command: dict[str, Any],
    budget: OutputBudget,
    service: asyncio.subprocess.Process | None = None,
) -> tuple[int, str | None]:
    process, capture = await start_command(command, budget)
    process_wait = asyncio.create_task(process.wait())
    exhaustion_wait = asyncio.create_task(budget.exhausted.wait())
    service_wait = asyncio.create_task(service.wait()) if service is not None else None
    waits = {process_wait, exhaustion_wait}
    if service_wait is not None:
        waits.add(service_wait)
    done, pending = await asyncio.wait(waits, return_when=asyncio.FIRST_COMPLETED)
    reason = None
    if exhaustion_wait in done and budget.exhausted.is_set():
        reason = "output_exhausted"
        await stop_process(process)
    elif service_wait is not None and service_wait in done:
        reason = "service_exited"
        await stop_process(process)
    else:
        await process_wait
    for task in pending:
        task.cancel()
    await asyncio.gather(*pending, return_exceptions=True)
    await capture
    return process.returncode if process.returncode is not None else -1, reason


async def copy_stream(
    reader: asyncio.StreamReader, writer: asyncio.StreamWriter
) -> None:
    try:
        while chunk := await reader.read(64 * 1024):
            writer.write(chunk)
            await writer.drain()
    finally:
        try:
            writer.write_eof()
        except (AttributeError, OSError):
            pass


async def relay(
    reader: asyncio.StreamReader, writer: asyncio.StreamWriter, connector: Any
) -> None:
    try:
        upstream_reader, upstream_writer = await connector()
        await asyncio.gather(
            copy_stream(reader, upstream_writer),
            copy_stream(upstream_reader, writer),
        )
        upstream_writer.close()
        await upstream_writer.wait_closed()
    finally:
        writer.close()
        await writer.wait_closed()


async def start_bridges(protocol: dict[str, Any]) -> list[asyncio.AbstractServer]:
    servers: list[asyncio.AbstractServer] = []
    if client := protocol["client_proxy"]:
        async def client_handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            await relay(
                reader,
                writer,
                lambda: asyncio.open_unix_connection(CLIENT_PROXY_SOCKET),
            )

        servers.append(
            await asyncio.start_server(client_handler, "127.0.0.1", client["listen_port"])
        )
    if managed := protocol["managed_server"]:
        async def server_handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            await relay(
                reader,
                writer,
                lambda: asyncio.open_connection("127.0.0.1", managed["target_port"]),
            )

        servers.append(await asyncio.start_unix_server(server_handler, MANAGED_SERVER_SOCKET))
    return servers


async def wait_for_managed_service(
    timeout_ms: int,
    budget: OutputBudget,
    service: asyncio.subprocess.Process,
    target_port: int,
) -> str | None:
    loop = asyncio.get_running_loop()
    deadline = loop.time() + timeout_ms / 1000
    delay = 0.025
    for _attempt in range(MAX_READINESS_ATTEMPTS):
        if budget.exhausted.is_set():
            return "output_exhausted"
        if service.returncode is not None:
            return "service_exited"
        remaining = deadline - loop.time()
        if remaining <= 0:
            return "startup_timeout"
        try:
            _reader, writer = await asyncio.wait_for(
                asyncio.open_connection("127.0.0.1", target_port),
                timeout=min(0.2, remaining),
            )
        except (ConnectionError, OSError, asyncio.TimeoutError):
            remaining = deadline - loop.time()
            if remaining <= 0:
                return "startup_timeout"
            await asyncio.sleep(min(delay, remaining))
            delay = min(delay * 2, 1.0)
        else:
            writer.close()
            await writer.wait_closed()
            return None
    return "readiness_attempts_exhausted"


async def wait_for_start(
    timeout_ms: int,
    budget: OutputBudget,
    service: asyncio.subprocess.Process | None,
) -> str | None:
    deadline = asyncio.get_running_loop().time() + timeout_ms / 1000
    while not START.exists():
        if budget.exhausted.is_set():
            return "output_exhausted"
        if service is not None and service.returncode is not None:
            return "service_exited"
        if asyncio.get_running_loop().time() >= deadline:
            return "startup_timeout"
        await asyncio.sleep(0.01)
    return None


def write_result(
    protocol: dict[str, Any],
    phase: str,
    exit_code: int | None,
    reason: str | None,
    budget: OutputBudget,
) -> None:
    document = {
        "schema_version": RESULT_SCHEMA,
        "plan_id": protocol["plan_id"],
        "phase": phase,
        "exit_code": exit_code,
        "reason": reason,
        "output_exhausted": budget.exhausted.is_set(),
        "output_base64": base64.b64encode(bytes(budget.output)).decode("ascii"),
    }
    temporary = RESULT.with_suffix(".tmp")
    with temporary.open("xb") as handle:
        os.chmod(temporary, 0o600)
        handle.write(json.dumps(document, sort_keys=True, separators=(",", ":")).encode())
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, RESULT)


async def execute(protocol: dict[str, Any]) -> int:
    budget = OutputBudget(protocol["max_output_bytes"])
    service = None
    service_capture = None
    servers: list[asyncio.AbstractServer] = []
    phase = "engine"
    exit_code = None
    reason = None
    try:
        engine_probe = {
            **protocol["workload"],
            "arguments": protocol["engine_probe_arguments"],
            "environment": {},
            "secret_environment_file": None,
        }
        before_probe = len(budget.output)
        exit_code, reason = await run_command(engine_probe, budget)
        probe_output = bytes(budget.output[before_probe:]).decode("utf-8", errors="replace")
        version_tokens = {
            token.strip(",;()[]{}") for token in probe_output.split()
        }
        if exit_code != 0 or reason is not None or protocol["engine_version"] not in version_tokens:
            write_result(
                protocol,
                phase,
                exit_code,
                reason or "engine_identity_mismatch",
                budget,
            )
            return 1
        phase = "prepare"
        for command in protocol["prepare"]:
            exit_code, reason = await run_command(command, budget)
            if exit_code != 0 or reason is not None:
                write_result(protocol, phase, exit_code, reason, budget)
                return 1
        phase = "service"
        if protocol["service"] is not None:
            service, service_capture = await start_command(protocol["service"], budget)
        servers = await start_bridges(protocol)
        if managed_server := protocol["managed_server"]:
            if service is None:
                raise RuntimeError("managed server has no service process")
            reason = await wait_for_managed_service(
                protocol["startup_timeout_ms"],
                budget,
                service,
                managed_server["target_port"],
            )
            if reason is not None:
                write_result(
                    protocol,
                    phase,
                    service.returncode,
                    reason,
                    budget,
                )
                return 1
        READY.touch(mode=0o600, exist_ok=False)
        reason = await wait_for_start(protocol["startup_timeout_ms"], budget, service)
        if reason is not None:
            write_result(protocol, phase, service.returncode if service else None, reason, budget)
            return 1
        phase = "workload"
        exit_code, reason = await run_command(protocol["workload"], budget, service)
        write_result(protocol, phase, exit_code, reason, budget)
        return 0 if exit_code == 0 and reason is None else 1
    except Exception as error:  # exact failure is private runtime evidence
        write_result(protocol, phase, exit_code, type(error).__name__, budget)
        return 1
    finally:
        for server in servers:
            server.close()
        await asyncio.gather(*(server.wait_closed() for server in servers))
        if service is not None:
            await stop_process(service)
        if service_capture is not None:
            await service_capture


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: workload_harness.py <protocol.json>")
    protocol = load_protocol(Path(sys.argv[1]))
    return asyncio.run(execute(protocol))


if __name__ == "__main__":
    raise SystemExit(main())
