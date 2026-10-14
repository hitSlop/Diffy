#!/usr/bin/env python3
"""Check the installable package on a disposable native CI machine."""
import argparse
import os
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--platform", choices=["windows", "linux"], required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
package = root / "dist/unsigned" / args.platform / ("Diffy-windows-x86_64-setup.exe" if args.platform == "windows" else "Diffy-linux-x86_64.AppImage")
if not os.environ.get("CI"):
    raise RuntimeError("Run installer checks only on disposable CI machines.")
with tempfile.TemporaryDirectory(prefix="diffy-package-") as directory:
    directory = Path(directory)
    repo = directory / "项目 with spaces"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    (repo / "new.txt").write_text("new\n")
    state = directory / "preferences/state.json"
    state.parent.mkdir()
    state.write_text('{"version":1,"projects":[],"active":null}')
    original = state.read_bytes()
    env = {**os.environ, "DIFFY_STATE_PATH": str(state), "APPIMAGE_EXTRACT_AND_RUN": "1"}
    if args.platform == "windows":
        installation = directory / "installed"
        subprocess.run([str(package), "/S", f"/D={installation}"], check=True, timeout=120)
        executable = installation / "diffy.exe"
        command = installation / "diffy-cli.exe"
        if not executable.exists() or not command.exists():
            raise RuntimeError("Installer omitted the app or terminal launcher.")
        subprocess.run([str(command), "--version"], check=True, env=env)
        result = subprocess.run([str(command), "--check", str(repo)], check=True, env=env, capture_output=True, text=True)
        # Exercise a second installation over the first, preserving user data.
        subprocess.run([str(package), "/S", f"/D={installation}"], check=True, timeout=120)
    else:
        executable = directory / "Diffy.AppImage"
        executable.write_bytes(package.read_bytes())
        executable.chmod(0o755)
        subprocess.run([str(executable), "--version"], check=True, env=env)
        result = subprocess.run([str(executable), "--check", str(repo)], check=True, env=env, capture_output=True, text=True)
    if "0 staged, 1 changes" not in result.stdout:
        raise RuntimeError(f"Packaged repository check failed: {result.stdout}")
    if state.read_bytes() != original:
        raise RuntimeError("Package installation or --check modified preferences.")
print("Installed package, CLI Git access, and preference preservation verified.")
