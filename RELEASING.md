# Releasing Diffy

The macOS beta supports macOS 13+ on Apple Silicon. Windows x86-64 and Linux x86-64 are early alphas with limited hardware testing. All platforms share the same numeric version and signing trust; Beta/Alpha are maturity labels, not GitHub prerelease flags. App identity: `com.hitslop.diffy`; repository: https://github.com/hitSlop/diffy. The workspace can remain named `differ`.

## Build and validate

Install Rust and Xcode command-line tools, then install the pinned packager:

```sh
cargo install cargo-packager --version 0.11.8 --locked --root target/tools
cargo fmt --check
cargo test --locked --features ui-tests
cargo clippy --locked --all-targets --features ui-tests -- -D warnings
python3 scripts/release-macos.py --unsigned
```

Unsigned packages are isolated in `dist/unsigned` and must not be uploaded as production releases. GitHub Actions performs these checks without access to signing secrets. For a quick local development bundle, use `sh scripts/bundle-macos.sh --debug`; it produces `dist/Diffy.app`. The script uses the project's existing Developer ID certificate from the login keychain when available, giving macOS folder permissions a stable code identity across rebuilds. Without that certificate it falls back to ad-hoc signing and reports that permissions may prompt again. `DIFFY_CODESIGN_IDENTITY` and `DIFFY_CODESIGN_KEYCHAIN` override those local choices; `DIFFY_CODESIGN_IDENTITY=-` explicitly selects ad-hoc signing. Local bundles are not notarized or published by this script.

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
- `Diffy-macos-aarch64.dmg`: byte-identical fixed-name copy for the website's permanent download link
- `Diffy_<version>_aarch64.app.tar.gz`
- The archive's `.sig` file
- `update-macos-aarch64.json`
- `SHA256SUMS`

Notarization reports stay local. Never upload `.release-secrets`, VISIONCAL credentials, signing certificates, or debug/unsigned packages. Validate the bundle with `codesign`, `stapler`, and `spctl`; launch it through Finder without a shell PATH and open a real repository. Install to a writable Applications directory before testing in-app updates.

## Draft release

Commit the validated source on a release branch, push it, and open a PR against `master`. Tag the exact validated commit and prepare a draft while the PR checks run. Review and merge the PR before publication:

```sh
git tag v0.2.3
git push origin HEAD v0.2.3
gh release create v0.2.3 --repo hitSlop/diffy --draft --verify-tag \
  --title 'Diffy v0.2.3' --notes-file packaging/release-notes.md \
  dist/release/Diffy_0.2.3_aarch64.dmg \
  dist/release/Diffy-macos-aarch64.dmg \
  dist/release/Diffy_0.2.3_aarch64.app.tar.gz \
  dist/release/Diffy_0.2.3_aarch64.app.tar.gz.sig \
  dist/release/update-macos-aarch64.json dist/release/SHA256SUMS
```

The manifest archive URL points to its immutable version tag. The app checks `releases/latest/download/update-macos-aarch64.json`, so draft releases are intentionally invisible to automatic checks. Publish only after release verification and explicit release authorization.

Always upload the fixed-name DMG with every release. The website redirects `https://diffy.hitslop.com/download` to GitHub's latest published release asset with that name. Include both DMGs in `SHA256SUMS`; retain the versioned app archive and manifest used by in-app updates.

## Update verification

The automated suite covers preference migration, update state transitions, Git write gating, transport validation, real signature verification, corrupted payloads, and existing application behavior. To exercise the real app archive on isolated bundle copies:

```sh
DIFFY_TEST_UPDATE_ARCHIVE="$PWD/dist/release/Diffy_0.2.3_aarch64.app.tar.gz" \
DIFFY_TEST_UPDATE_SIGNATURE="$PWD/dist/release/Diffy_0.2.3_aarch64.app.tar.gz.sig" \
cargo test --locked --test update_archive signed_app_archive_installs_and_relaunches_an_isolated_bundle -- --ignored --exact --nocapture
```

Production checks and downloads require HTTPS. This offline fixture operates exclusively on temporary app copies, verifies the signed archive before installation, and confirms the installed executable launches. It never replaces the user's app or edits their preferences.

The signed-archive exercise is ignored in routine checks and requires its archive and signature when invoked explicitly. The release script invokes it with the exact release version. To exercise a historical signed archive, set `DIFFY_TEST_UPDATE_VERSION` to its expected version; otherwise the current Cargo version is required.

## Windows and Linux native builds

Use Rust 1.92+ and the native toolchain. Windows requires Visual Studio 2022 C++ Build Tools, a Windows SDK, CMake, and the MSVC Rust target. The build script embeds the supplied icon; GPUI supplies the Windows manifest. Windows installers are per-user NSIS packages and are not publisher-signed for the alpha. Updater payload signatures remain required.

On Ubuntu 24.04, install the GPUI toolchain and runtime dependencies:

```sh
sudo apt-get install -y pkg-config gcc g++ clang cmake libfontconfig-dev libwayland-dev libwebkit2gtk-4.1-dev libxkbcommon-x11-dev libx11-xcb-dev libssl-dev libzstd-dev libvulkan1 mesa-vulkan-drivers xvfb xdg-utils libfuse2t64
cargo install cargo-packager --version 0.11.8 --locked --root target/tools
```

Run the corresponding command on each native operating system (Windows uses `python`):

```sh
python scripts/release-desktop.py --platform windows --unsigned
python3 scripts/release-desktop.py --platform linux --unsigned
```

The CI workflow runs native functional tests and Clippy, Linux offscreen renderer/interaction tests, native startup smoke tests, package installation/reinstallation checks, and CLI Git access. The pinned GPUI snapshot has no Windows headless screenshot renderer; the Windows native startup smoke opens a real demo window and draws a frame. This does not replace hardware QA. CI installer checks run only on disposable runners. Windows-safe Unicode/spaced filenames replace Unix-only filename fixtures on Windows.

Unsigned-check artifacts are under `dist/unsigned/windows` and `dist/unsigned/linux`. Windows outputs the versioned NSIS installer plus `Diffy-windows-x86_64-setup.exe`; Linux outputs the versioned AppImage plus `Diffy-linux-x86_64.AppImage`. Each directory includes source-tree/version/checksum metadata. Terminal launchers and dependency notices are packaged alongside the app.

## Assemble a complete release

Build all three platforms from the same clean source tree and version. Download the successful Windows/Linux CI package artifacts, retaining their `build-*.json` files. Build the signed/notarized macOS release locally with the existing script, then assemble:

```sh
python3 scripts/release-macos.py
python3 scripts/assemble-release.py --macos dist/release --windows /path/to/windows-artifact --linux /path/to/linux-artifact
```

Assembly rejects version, target, source-tree, or package-digest mismatches. It preserves the notarized macOS assets, signs the exact Windows installer and raw Linux AppImage with the existing updater key, writes per-platform manifests and one combined `SHA256SUMS`. It does not publish anything. Standalone native releases can omit `--unsigned` when the same signing key/public-key file is available; `DIFFY_UPDATER_KEY` and `DIFFY_UPDATER_PASSWORD` select it without committing secrets.

The current Linux updater installs raw AppImage bytes: do not point its manifest at a `.tar.gz`. AppImage restart and terminal installation use the outer `APPIMAGE` path, never the executable inside its temporary mount. Windows waits for Git writes and flushes preferences before the updater starts its installer and exits; `/R` makes the verified NSIS installer relaunch Diffy. Windows installer failures are handled by the installer, rather than promising the same bundle rollback as macOS.

Create a normal **draft** GitHub release for the next version after the complete source and package checks pass. Include every asset named by the combined checksums file and that file itself. Keep the existing `update-macos-aarch64.json` endpoint and fixed macOS DMG alias. New assets include `update-windows-x86_64.json`, `update-linux-x86_64.json`, and their signed packages. Ordinary published releases preserve `/releases/latest`; draft releases remain invisible to update checks.

Only after publication and download verification, set the site's `DIFFY_RELEASED_PLATFORMS` repository variable to `macos,windows,linux` and rebuild/deploy. Default `macos` keeps the new links unavailable. See `site/README.md` for the $14.99 Dodo product and shared post-purchase delivery setup.

Before promoting Windows/Linux from alpha, collect real hardware checks for rendering, clipboard, dialogs, held-key navigation, editor/terminal launch, window controls, and installation/update/restart. Intel Mac, ARM Windows/Linux, `.deb`, and broader Linux compatibility remain outside this release.
