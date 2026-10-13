#!/bin/sh
set -eu
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH}"
export MACOSX_DEPLOYMENT_TARGET=13.0
cd "$(dirname "$0")/.."
mkdir -p target/packaging
python3 scripts/build-icon.py
cargo build --locked --release --target aarch64-apple-darwin
python3 scripts/collect-licenses.py
