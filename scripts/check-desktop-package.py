#!/usr/bin/env python3
"""Check the installable package on a disposable native CI machine."""
import argparse
import os
import subprocess
import tempfile
import time
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
    # Sign the actual package with a throwaway key. This exercises the same
    # verifier and installer adapter without putting production signing material in CI.
    key = directory / "fixture.key"
    packager = root / "target/tools/bin" / ("cargo-packager.exe" if args.platform == "windows" else "cargo-packager")
    subprocess.run([str(packager), "signer", "generate", "--ci", "--password", "ci-fixture", "--path", str(key)], check=True, capture_output=True)
    sign_env = {**env, "CARGO_PACKAGER_SIGN_PRIVATE_KEY": str(key), "CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD": "ci-fixture"}
    subprocess.run([str(packager), "signer", "sign", str(package)], check=True, env=sign_env, capture_output=True)
    signature = Path(str(package) + ".sig")
    marker = directory / "updated-render.txt"
    update_env = {**env, "DIFFY_CI_UPDATE_PACKAGE": str(package), "DIFFY_CI_UPDATE_SIGNATURE": str(signature),
                  "DIFFY_CI_UPDATE_PUBLIC": str(key) + ".pub", "DIFFY_CI_UPDATE_DESTINATION": str(installation if args.platform == "windows" else executable),
                  "DIFFY_SMOKE_RESULT": str(marker)}
    if args.platform == "linux":
        executable.write_bytes(b"old appimage fixture")
    subprocess.run(["cargo", "test", "--locked", "--test", "update_desktop", "apply_signed_native_package", "--", "--ignored", "--exact"], cwd=root, env=update_env, check=True, timeout=120)
    if args.platform == "linux":
        subprocess.run([str(executable), "--smoke-test"], env=update_env, check=True, timeout=45)
    deadline = time.monotonic() + 60
    while not marker.exists() and time.monotonic() < deadline:
        time.sleep(0.1)
    if not marker.exists() or marker.read_text() != "rendered\n":
        raise RuntimeError("The updated application did not relaunch and render.")
    if state.read_bytes() != original:
        raise RuntimeError("Update/restart changed preferences.")
    if args.platform == "windows":
        # The marker is written just before quitting. Wait for this isolated
        # app to exit before TemporaryDirectory removes its installation.
        subprocess.run([
            "powershell", "-NoProfile", "-Command",
            "foreach ($app in @(Get-Process -Name diffy -ErrorAction SilentlyContinue)) { "
            "if ($app.Path -eq $env:DIFFY_CI_SMOKE_EXECUTABLE) { "
            "if (!$app.WaitForExit(45000)) { throw 'Updated smoke-test app did not exit.' } } }",
        ], env={**update_env, "DIFFY_CI_SMOKE_EXECUTABLE": str(executable)}, check=True, timeout=50)
print("Installed package, CLI Git access, and preference preservation verified.")
