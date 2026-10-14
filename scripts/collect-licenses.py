#!/usr/bin/env python3
"""Ship license texts and an exact dependency inventory alongside the executable."""
import json
import shutil
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"
], cwd=root))
output = root / "target/packaging/licenses"
if output.exists():
    shutil.rmtree(output)
output.mkdir(parents=True)
inventory = []
# Benchmark-only dependencies are not part of the packaged application.
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
included = set()
pending = [metadata["resolve"]["root"]]
while pending:
    package_id = pending.pop()
    if package_id in included:
        continue
    included.add(package_id)
    pending.extend(dep["pkg"] for dep in nodes[package_id]["deps"]
                   if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))
for package in sorted((p for p in metadata["packages"] if p["id"] in included), key=lambda p: (p["name"], p["version"])):
    directory = Path(package["manifest_path"]).parent
    destination = output / (package["name"] + "-" + package["version"])
    candidates = [p for p in directory.iterdir()
                  if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"))]
    if package.get("license_file"):
        candidates.append(directory / package["license_file"])
    for source in candidates:
        if source.is_file():
            destination.mkdir(exist_ok=True)
            shutil.copyfile(source, destination / source.name)
    inventory.append({k: package.get(k) for k in ("name", "version", "license", "repository", "source")})
(output / "dependencies.json").write_text(json.dumps(inventory, indent=2) + "\n")
print("Collected dependency licenses.")
