#!/usr/bin/env python3
"""Build Windows/Linux packages and signed updater assets without publishing."""
import argparse
import datetime
import hashlib
import json
import os
import platform
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "windows": ("x86_64-pc-windows-msvc", "nsis", "Diffy-windows-x86_64-setup.exe"),
    "linux": ("x86_64-unknown-linux-gnu", "appimage", "Diffy-linux-x86_64.AppImage"),
}


def run(args, env, capture=False):
    result = subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env,
                            check=True, text=True, stdout=subprocess.PIPE if capture else None)
    return result.stdout if capture else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=TARGETS, required=True)
    parser.add_argument("--unsigned", action="store_true", help="Build CI packages without updater signing material")
    args = parser.parse_args()
    if platform.system().lower() != args.platform:
        parser.error("Build on the target operating system with its native toolchain.")
    target, package_format, stable_name = TARGETS[args.platform]
    env = {key: value for key, value in os.environ.items() if not key.startswith("CARGO_PACKAGER_")}
    tools = ROOT / "target/tools/bin"
    env["PATH"] = str(tools) + os.pathsep + env.get("PATH", "")
    packager = shutil.which("cargo-packager", path=env["PATH"])
    if not packager or "0.11.8" not in run([packager, "--version"], env, True):
        raise RuntimeError("Install cargo-packager 0.11.8 with --locked into target/tools first.")
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], env, True))["packages"][0]
    version = metadata["version"]
    out = ROOT / "dist" / ("unsigned" if args.unsigned else "release") / args.platform
    # Reject stale outputs so the fixed-name link always contains this build.
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    env["DIFFY_PACKAGE_KIND"] = package_format
    run(["cargo", "build", "--locked", "--release", "--bins", "--target", target], env)
    run(["python3" if args.platform == "linux" else "python", "scripts/collect-licenses.py", target], env)
    config = metadata["metadata"]["packager"].copy()
    config.pop("macos", None)
    config.pop("before-packaging-command", None)
    config.update({
        "version": version, "formats": [package_format], "target-triple": target,
        "binaries": [{"path": "diffy", "main": True}, {"path": "diffy-cli", "main": False}],
        "binaries-dir": str(ROOT / "target" / target / "release"), "out-dir": str(out),
        "icons": ["assets/icon/icon.iconset/icon_32x32.png", "assets/icon/icon.iconset/icon_128x128.png", "assets/icon/icon.iconset/icon_256x256.png"],
    })
    if args.platform == "windows":
        config["nsis"] = {"install-mode": "currentUser"}
    configuration = ROOT / "target/packaging" / f"{args.platform}.json"
    configuration.parent.mkdir(parents=True, exist_ok=True)
    configuration.write_text(json.dumps(config, indent=2) + "\n")
    run([packager, "--release", "--config", configuration], env)
    suffix = ".exe" if args.platform == "windows" else ".AppImage"
    packages = list(out.glob(f"*{suffix}"))
    if len(packages) != 1:
        raise RuntimeError(f"Expected one {suffix} package, found {len(packages)}.")
    package = packages[0]
    stable = out / stable_name
    shutil.copyfile(package, stable)
    if args.platform == "linux":
        stable.chmod(package.stat().st_mode)
    assets = [package, stable]
    if not args.unsigned:
        key = os.environ.get("DIFFY_UPDATER_KEY", str(ROOT / ".release-secrets/updater.key"))
        password_file = ROOT / ".release-secrets/updater.password"
        password = os.environ.get("DIFFY_UPDATER_PASSWORD")
        if password is None:
            password = password_file.read_text().strip()
        env["CARGO_PACKAGER_SIGN_PRIVATE_KEY"] = key
        env["CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD"] = password
        run([packager, "signer", "sign", package], env)
        signature = Path(str(package) + ".sig")
        manifest = out / f"update-{args.platform}-x86_64.json"
        manifest.write_text(json.dumps({
            "version": version, "platforms": {f"{args.platform}-x86_64": {
                "url": f"https://github.com/hitSlop/diffy/releases/download/v{version}/{package.name}",
                "signature": signature.read_text().strip(), "format": package_format,
            }}, "notes": f"Diffy {version} for {args.platform} x86-64 (alpha).",
            "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"),
        }, indent=2) + "\n")
        assets.extend([signature, manifest])
    (out / "SHA256SUMS").write_text("".join(f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n" for asset in assets))
    print(f"Packages ready in {out}; no release was published.")


if __name__ == "__main__":
    main()
