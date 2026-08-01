from __future__ import annotations

import subprocess
import sys
from pathlib import Path


if len(sys.argv) != 4:
    raise SystemExit("usage: server_wrapper.py PORT OPENAPI_PATH CHILD_PID_PATH")

server = Path(__file__).with_name("server.py")
child = subprocess.Popen([sys.executable, str(server), sys.argv[1], sys.argv[2]])
Path(sys.argv[3]).write_text(str(child.pid), encoding="utf8")
raise SystemExit(child.wait())
