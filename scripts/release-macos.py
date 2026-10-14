#!/usr/bin/env python3
"""Prepare signed release assets; never publish a release or print credentials."""
import argparse
import datetime
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IDENTITY = "Developer ID Application: Mushroom DAO Holdings Corp. (78UAXU8QG8)"
REPO = "hitSlop/diffy"
KEYCHAIN = str(Path.home() / "Library/Keychains/login.keychain-db")


def run(args, env, capture=False):
    result = subprocess.run([str(a) for a in args], cwd=ROOT, env=env,
                            stdout=subprocess.PIPE if capture else None, text=True)
    if result.returncode:
        # CalledProcessError includes arguments, which may contain notarization credentials.
        raise RuntimeError(f"{Path(str(args[0])).name} failed (exit {result.returncode}).")
    return result.stdout if capture else None


def credentials(path, env):
    allowed = {"ELECTROBUN_DEVELOPER_ID", "ELECTROBUN_TEAMID", "ELECTROBUN_APPLEID", "ELECTROBUN_APPLEIDPASS"}
    if path.exists():
        for line in path.read_text().splitlines():
            match = re.match(r"\s*(?:export\s+)?([A-Z_]+)\s*=\s*(.*?)\s*$", line)
            if match and match[1] in allowed and match[1] not in env:
                value = match[2]
                if value.startswith(('"', "'")) and value[-1:] == value[:1]:
                    value = value[1:-1]
                env[match[1]] = value
    for name in ("ELECTROBUN_TEAMID", "ELECTROBUN_APPLEID", "ELECTROBUN_APPLEIDPASS"):
        if not env.get(name):
            raise RuntimeError(f"Missing {name}; configure the local signing environment.")


def notarize(path, env, output):
    args = ["xcrun", "notarytool", "submit", path, "--wait", "--output-format", "json",
            "--apple-id", env["ELECTROBUN_APPLEID"], "--team-id", env["ELECTROBUN_TEAMID"],
            "--password", env["ELECTROBUN_APPLEIDPASS"]]
    result = json.loads(run(args, env, capture=True))
    output.write_text(json.dumps(result, indent=2) + "\n")
    if result.get("status") != "Accepted":
        raise RuntimeError(f"Apple notarization was {result.get('status', 'unsuccessful')}; see {output}.")


def prepare_dmg(env):
    if env.get("CARGO_PACKAGER_FORMAT") != "dmg":
        return
    output = Path(env["DIFFY_RELEASE_DIR"])
    bundle = output / "Diffy.app"
    binary = bundle / "Contents/MacOS/diffy"
    identity = env.get("ELECTROBUN_DEVELOPER_ID", IDENTITY)
    run(["codesign", "--force", "--options", "runtime", "--timestamp", "--keychain", KEYCHAIN, "--sign", identity, binary], env)
    run(["codesign", "--force", "--options", "runtime", "--timestamp", "--keychain", KEYCHAIN, "--sign", identity, bundle], env)
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", bundle], env)
    zip_path = output / "notarization.zip"
    run(["ditto", "-c", "-k", "--keepParent", bundle, zip_path], env)
    notarize(zip_path, env, output / "app-notarization.json")
    run(["xcrun", "stapler", "staple", bundle], env)
    run(["xcrun", "stapler", "validate", bundle], env)
    run(["spctl", "--assess", "--type", "execute", "--verbose=2", bundle], env)
    zip_path.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-dmg", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--unsigned", action="store_true", help="Build packaging checks without signing or an update manifest")
    parser.add_argument("--credentials", type=Path, default=ROOT.parent / "VISIONCAL/.env")
    args = parser.parse_args()
    env = os.environ.copy()
    env["PATH"] = f"{ROOT}/target/tools/bin:{Path.home()}/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:" + env.get("PATH", "")
    env["MACOSX_DEPLOYMENT_TARGET"] = "13.0"
    if args.prepare_dmg:
        prepare_dmg(env)
        return
    # Isolate packaging from signing environment inherited from other projects.
    env = {k: v for k, v in env.items() if not k.startswith("CARGO_PACKAGER_")}
    version = json.loads(run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], env, True))["packages"][0]["version"]
    packager = shutil.which("cargo-packager", path=env["PATH"])
    if not packager or "0.11.8" not in run([packager, "--version"], env, True):
        raise RuntimeError("Install cargo-packager 0.11.8 with --locked first.")
    identity = env.get("ELECTROBUN_DEVELOPER_ID", IDENTITY)
    if not args.unsigned:
        credentials(args.credentials, env)
        identity = env.get("ELECTROBUN_DEVELOPER_ID", IDENTITY)
        for name in ("updater.key", "updater.password"):
            if not (ROOT / ".release-secrets" / name).is_file():
                raise RuntimeError(f"Missing private updater signing material: {name}.")
        if (ROOT / ".release-secrets/updater.key.pub").read_text().strip() != (ROOT / "packaging/updater.pub").read_text().strip():
            raise RuntimeError("Updater private key doesn't match the application's public key.")
    output = ROOT / "dist" / ("unsigned" if args.unsigned else "release")
    output.mkdir(parents=True, exist_ok=True)
    if args.unsigned:
        run([packager, "--release", "--target", "aarch64-apple-darwin", "--out-dir", output], env)
    else:
        metadata = json.loads(run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], env, True))["packages"][0]
        config = metadata["metadata"]["packager"].copy()
        config.update({
            "version": version, "binaries": [{"path": "diffy", "main": True}],
            "binaries-dir": str(ROOT / "target/aarch64-apple-darwin/release"),
            "target-triple": "aarch64-apple-darwin", "out-dir": str(output),
            "before-each-package-command": "python3 scripts/release-macos.py --prepare-dmg",
        })
        config_path = ROOT / "target/packaging/release.json"
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(json.dumps(config, indent=2) + "\n")
        env["DIFFY_RELEASE_DIR"] = str(output)
        # One packager invocation: sign and staple the app between app and DMG stages.
        run([packager, "--config", json.dumps(config)], env)
    bundle = output / "Diffy.app"
    binary = bundle / "Contents/MacOS/diffy"
    metadata = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    assert metadata["CFBundleIdentifier"] == "com.hitslop.diffy"
    assert metadata["CFBundleShortVersionString"] == version
    assert run(["lipo", "-archs", binary], env, True).strip() == "arm64"
    if args.unsigned:
        print(f"Unsigned packaging checks ready in {output}")
        return
    archive = output / f"Diffy_{version}_aarch64.app.tar.gz"
    # BSD tar's AppleDouble metadata is incompatible with the updater's single app root.
    archive_env = {**env, "COPYFILE_DISABLE": "1"}
    run(["tar", "-czf", archive, "-C", output, "Diffy.app"], archive_env)
    dmg_candidates = list(output.glob(f"Diffy_{version}_*.dmg"))
    if len(dmg_candidates) != 1:
        raise RuntimeError("Expected exactly one packaged DMG.")
    dmg = output / f"Diffy_{version}_aarch64.dmg"
    if dmg_candidates[0] != dmg:
        dmg_candidates[0].rename(dmg)
    run(["codesign", "--force", "--timestamp", "--keychain", KEYCHAIN, "--sign", identity, dmg], env)
    notarize(dmg, env, output / "dmg-notarization.json")
    run(["xcrun", "stapler", "staple", dmg], env)
    run(["xcrun", "stapler", "validate", dmg], env)
    # Keep customer download links stable while the updater retains versioned URLs.
    stable_dmg = output / "Diffy-macos-aarch64.dmg"
    shutil.copyfile(dmg, stable_dmg)
    env["CARGO_PACKAGER_SIGN_PRIVATE_KEY"] = str(ROOT / ".release-secrets/updater.key")
    env["CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD"] = (ROOT / ".release-secrets/updater.password").read_text().strip()
    run([packager, "signer", "sign", archive], env)
    signature = Path(str(archive) + ".sig")
    manifest = output / "update-macos-aarch64.json"
    manifest.write_text(json.dumps({
        "version": version, "url": f"https://github.com/{REPO}/releases/download/v{version}/{archive.name}",
        "signature": signature.read_text().strip(), "format": "app",
        "notes": f"Diffy {version} for Apple Silicon Macs. Requires macOS 13 or later.",
        "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"),
    }, indent=2) + "\n")
    # Exercise the signed archive before considering the release assets ready.
    verify_env = {**env, "DIFFY_TEST_UPDATE_ARCHIVE": str(archive), "DIFFY_TEST_UPDATE_SIGNATURE": str(signature), "DIFFY_TEST_UPDATE_VERSION": version}
    run(["cargo", "test", "--locked", "--test", "update_archive", "signed_app_archive_installs_and_relaunches_an_isolated_bundle", "--", "--ignored", "--exact"], verify_env)
    assets = [dmg, stable_dmg, archive, signature, manifest]
    (output / "SHA256SUMS").write_text("".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in assets))
    print(f"Signed, notarized release assets ready in {output}. Publication is not automatic.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, AssertionError, OSError, ValueError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        sys.exit(1)
