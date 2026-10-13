#!/usr/bin/env python3
"""Compile the supplied PNGs from a clean iconset, excluding Finder resource files."""
import shutil
import struct
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
source = root / "assets/icon/icon.iconset"
clean = root / "target/packaging/Diffy.iconset"
if clean.exists():
    shutil.rmtree(clean)
clean.mkdir(parents=True)
for size in (16, 32, 128, 256, 512):
    for scale in (1, 2):
        name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
        png = source / name
        data = png.read_bytes()
        if data[:8] != b"\x89PNG\r\n\x1a\n" or struct.unpack(">II", data[16:24]) != (size * scale, size * scale):
            raise ValueError(f"Invalid supplied icon dimensions: {name}")
        shutil.copyfile(png, clean / name)
output = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "target/packaging/Diffy.icns"
output.parent.mkdir(parents=True, exist_ok=True)
subprocess.run(["/usr/bin/iconutil", "-c", "icns", str(clean), "-o", str(output)], check=True)
