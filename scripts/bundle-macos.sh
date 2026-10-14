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
# TCC remembers folder permissions by designated code-signing requirement.
# Ad-hoc signatures bind that identity to the binary's hash, so every rebuild
# can prompt again. Reuse this project's release signer when it is available.
signing_identity=${DIFFY_CODESIGN_IDENTITY:-"Developer ID Application: Mushroom DAO Holdings Corp. (78UAXU8QG8)"}
signing_keychain=${DIFFY_CODESIGN_KEYCHAIN:-"$HOME/Library/Keychains/login.keychain-db"}
if [ "$signing_identity" = "-" ]; then
  codesign --force --sign - "$bundle"
elif security find-identity -v -p codesigning "$signing_keychain" | /usr/bin/grep -F -- "\"$signing_identity\"" >/dev/null; then
  codesign --force --timestamp=none --keychain "$signing_keychain" --sign "$signing_identity" "$bundle"
elif [ -n "${DIFFY_CODESIGN_IDENTITY:-}" ]; then
  printf 'Requested signing identity is unavailable in %s.\n' "$signing_keychain" >&2
  exit 1
else
  printf 'Project signing certificate unavailable; using ad-hoc signing. Folder access may prompt again after rebuilding.\n' >&2
  codesign --force --sign - "$bundle"
fi
codesign --verify --deep --strict "$bundle"
printf 'Built %s\n' "$bundle"
