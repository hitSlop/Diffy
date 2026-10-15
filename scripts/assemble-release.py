#!/usr/bin/env python3
"""Combine validated native packages and sign Windows/Linux updater payloads locally."""
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLATFORMS = {
    "macos-aarch64": ("aarch64-apple-darwin", "app", "Diffy-macos-aarch64.dmg"),
    "windows-x86_64": ("x86_64-pc-windows-msvc", "nsis", "Diffy-windows-x86_64-setup.exe"),
    "linux-x86_64": ("x86_64-unknown-linux-gnu", "appimage", "Diffy-linux-x86_64.AppImage"),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("macos", "windows", "linux"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    version = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT))["packages"][0]["version"]
    tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT, text=True).strip()
    for check in (["git", "diff", "--quiet"], ["git", "diff", "--cached", "--quiet"]):
        subprocess.run(check, cwd=ROOT, check=True)
    out = ROOT / "dist/release"
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    key = ROOT / ".release-secrets/updater.key"
    if Path(str(key) + ".pub").read_text().strip() != (ROOT / "packaging/updater.pub").read_text().strip():
        raise RuntimeError("The signing key doesn't match Diffy's public key.")
    env["CARGO_PACKAGER_SIGN_PRIVATE_KEY"] = str(key)
    env["CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD"] = (ROOT / ".release-secrets/updater.password").read_text().strip()
    packager = ROOT / "target/tools/bin/cargo-packager"
    assets = []
    for platform, (target, format_name, stable_name) in PLATFORMS.items():
        directory = getattr(args, platform.split('-')[0]).resolve()
        info_path = directory / f"build-{platform}.json"
        info = json.loads(info_path.read_text())
        if info["version"] != version or info["target"] != target or info["source_tree"] != tree:
            raise RuntimeError(f"{platform} wasn't built from this version and source tree.")
        package_name = info["package"]
        if Path(package_name).name != package_name:
            raise RuntimeError("Invalid package filename in build metadata.")
        package = directory / package_name
        if hashlib.sha256(package.read_bytes()).hexdigest() != info["sha256"]:
            raise RuntimeError(f"{platform} package checksum mismatch.")
        if platform != "macos-aarch64" and package.read_bytes() != (directory / stable_name).read_bytes():
            raise RuntimeError("Stable download must be byte-identical to its versioned package.")
        for source in [package, directory / stable_name, info_path]:
            destination = out / source.name
            if source.resolve() != destination.resolve(): shutil.copyfile(source, destination)
            assets.append(destination)
        installed = out / package_name
        if platform == "macos-aarch64":
            for source in [Path(str(package) + ".sig"), directory / f"update-{platform}.json"]:
                destination = out / source.name
                if source.resolve() != destination.resolve(): shutil.copyfile(source, destination)
                assets.append(destination)
            dmg_name = f"Diffy_{version}_aarch64.dmg"
            dmg = directory / dmg_name
            if dmg.read_bytes() != (directory / stable_name).read_bytes(): raise RuntimeError("macOS DMG alias mismatch")
            if dmg.resolve() != (out / dmg_name).resolve(): shutil.copyfile(dmg, out / dmg_name)
            assets.append(out / dmg_name)
        else:
            subprocess.run([str(packager), "signer", "sign", str(installed)], env=env, cwd=ROOT, check=True)
            signature = Path(str(installed) + ".sig")
            manifest = out / f"update-{platform}.json"
            manifest.write_text(json.dumps({"version": version, "platforms": {platform: {
                "url": f"https://github.com/hitSlop/diffy/releases/download/v{version}/{package_name}",
                "signature": signature.read_text().strip(), "format": format_name}},
                "notes": f"Diffy {version}, {platform} alpha.",
                "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")}, indent=2) + "\n")
            assets.extend([signature, manifest])
    unique = sorted(set(assets))
    (out / "SHA256SUMS").write_text("".join(f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n" for asset in unique))
    print(f"Complete signed release assets ready in {out}. Publication is a separate step.")


if __name__ == "__main__": main()
