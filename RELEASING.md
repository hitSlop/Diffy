# Releasing Diffy

The initial release is macOS 13+, Apple Silicon only. App identity: `com.hitslop.diffy`; repository: https://github.com/hitSlop/diffy. The workspace can remain named `differ`.

## Build and validate

Install Rust and Xcode command-line tools, then install the pinned packager:

```sh
cargo install cargo-packager --version 0.11.8 --locked --root target/tools
cargo fmt --check
cargo test --locked --features ui-tests
cargo clippy --locked --all-targets --features ui-tests -- -D warnings
python3 scripts/release-macos.py --unsigned
```

Unsigned packages are isolated in `dist/unsigned` and must not be uploaded as production releases. GitHub Actions performs these checks without access to signing secrets. For a quick local development bundle, use `sh scripts/bundle-macos.sh --debug`; it produces an ad hoc signed `dist/Diffy.app`.

## Signing material

Local releases use the Developer ID Application certificate for Mushroom DAO Holdings Corp. (team `78UAXU8QG8`) in the login keychain. The script explicitly selects that keychain. It reads only the required `ELECTROBUN_*` identity and Apple notarization variables from the sibling `VISIONCAL/.env` file or environment. Use `--credentials /path/to/.env` to select another local file. Credentials are not sourced as shell code or copied into Git.

Updater signing material is separate from the Apple certificate:

- `.release-secrets/updater.key`: encrypted private key; permissions `0600`.
- `.release-secrets/updater.password`: encryption password; permissions `0600`.
- `packaging/updater.pub`: public key embedded in the app and committed to Git.

Back up the private key and password securely outside the workspace before distributing Diffy. Keep the directory private (`0700`). Do not regenerate a key for an existing release: installed apps trust the embedded public key. Losing the private key prevents future updates for those installations. Deliberate key rotation requires a separately designed transition release.

## Prepare release artifacts

Update the version in Cargo.toml and regenerate Cargo.lock when needed. The app, bundle, manifest, and artifact names derive their version from Cargo.

```sh
python3 scripts/release-macos.py
```

The script builds the ARM64 executable and supplied icon, collects dependency licenses, packages the app, signs with hardened runtime, submits it to Apple, and staples its ticket before packaging the DMG. It notarizes and staples the DMG, then signs the exact app archive used by the updater.

Final assets under `dist/release`:

- `Diffy_<version>_aarch64.dmg`
- `Diffy_<version>_aarch64.app.tar.gz`
- The archive's `.sig` file
- `update-macos-aarch64.json`
- `SHA256SUMS`

Notarization reports stay local. Never upload `.release-secrets`, VISIONCAL credentials, signing certificates, or debug/unsigned packages. Validate the bundle with `codesign`, `stapler`, and `spctl`; launch it through Finder without a shell PATH and open a real repository. Install to a writable Applications directory before testing in-app updates.

## Draft release

Commit the validated source, push `main`, and tag the exact release commit. Create a draft rather than publishing:

```sh
git tag v0.2.0
git push origin main v0.2.0
gh release create v0.2.0 --repo hitSlop/diffy --draft --verify-tag \
  --title 'Diffy v0.2.0' --notes-file packaging/release-notes.md \
  dist/release/Diffy_0.2.0_aarch64.dmg \
  dist/release/Diffy_0.2.0_aarch64.app.tar.gz \
  dist/release/Diffy_0.2.0_aarch64.app.tar.gz.sig \
  dist/release/update-macos-aarch64.json dist/release/SHA256SUMS
```

The manifest archive URL points to its immutable version tag. The app checks `releases/latest/download/update-macos-aarch64.json`, so draft releases are intentionally invisible to automatic checks. Publish only after release verification and explicit release authorization.

## Update verification

The automated suite covers preference migration, update state transitions, Git write gating, transport validation, real signature verification, corrupted payloads, and existing application behavior. To exercise the real app archive on isolated bundle copies:

```sh
DIFFY_TEST_UPDATE_ARCHIVE="$PWD/dist/release/Diffy_0.2.0_aarch64.app.tar.gz" \
DIFFY_TEST_UPDATE_SIGNATURE="$PWD/dist/release/Diffy_0.2.0_aarch64.app.tar.gz.sig" \
cargo test --locked --test update_archive -- --nocapture
```

Production checks and downloads require HTTPS. This offline fixture operates exclusively on temporary app copies, verifies the signed archive before installation, and confirms the installed executable launches. It never replaces the user's app or edits their preferences.
