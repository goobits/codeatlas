"""Run the HTTP fixture as a descendant to verify process-group cleanup."""

from __future__ import annotations

import subprocess
import sys


if len(sys.argv) < 2:
    raise SystemExit("usage: managed_server.py COMMAND [ARG ...]")

raise SystemExit(subprocess.Popen([sys.executable, *sys.argv[1:]]).wait())
