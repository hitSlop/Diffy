#!/bin/sh
set -eu

export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH}"
project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_root"

case "${1:---debug}" in
  --debug) cargo build --locked; profile=debug ;;
  --release) cargo build --locked --release; profile=release ;;
  *) printf 'Usage: %s [--debug | --release]\n' "$0" >&2; exit 2 ;;
esac

bundle="$project_root/dist/Diffy.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/diffy" "$bundle/Contents/MacOS/diffy"
cp packaging/Info.plist "$bundle/Contents/Info.plist"
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $version" "$bundle/Contents/Info.plist"
python3 scripts/build-icon.py "$bundle/Contents/Resources/Diffy.icns"
cp LICENSE "$bundle/Contents/Resources/LICENSE.txt"
cp assets/fonts/LICENSE "$bundle/Contents/Resources/CommitMono-LICENSE.txt"
cp THIRD_PARTY_NOTICES.md "$bundle/Contents/Resources/Third-party-notices.md"
cp vendor/gpui-component/LICENSE-APACHE "$bundle/Contents/Resources/GPUI-Component-LICENSE.txt"
codesign --force --deep --sign - "$bundle"
printf 'Built %s\n' "$bundle"
