#!/usr/bin/env python3
"""Exercise a native demo window and its renderer without touching preferences."""
import os
import subprocess
import sys
import tempfile
from pathlib import Path

with tempfile.TemporaryDirectory(prefix="diffy-smoke-") as directory:
    result = Path(directory) / "render.txt"
    env = {**os.environ, "RUST_BACKTRACE": "1", "DIFFY_SMOKE_RESULT": str(result), "DIFFY_STATE_PATH": str(Path(directory) / "state.json")}
    process = subprocess.run([sys.argv[1], "--smoke-test"], env=env, timeout=45,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, encoding="utf-8", errors="replace")
    if process.stdout:
        print(process.stdout, end="", flush=True)
    process.check_returncode()
    if not result.exists() or result.read_text() != "rendered\n":
        raise RuntimeError("The native window did not render a frame.")
    if (Path(directory) / "state.json").exists():
        raise RuntimeError("The isolated demo unexpectedly saved preferences.")
print("Native startup and rendering verified.")
