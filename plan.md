# Diffy: pure black, Commit Mono, and quick project navigation

## Current implementation

- Native macOS Rust app using GPUI Kit 0.7.3, its Diff component, and Git CLI.
- Pure `#000000` window, toolbar, rail, sidebar, and footer, with invisible resize handles and no divider strokes.
- Bundled Commit Mono 1.143 Regular/Medium throughout the app. Source and labels use 13 px, headings 14 px, and footer hints 12 px; programming ligatures are disabled.
- Colored squircle project icons grow from 42 to 46 px on hover within fixed hit areas, respecting reduced motion. Names appear only on hover.
- Independent Staged / Changes views, compact alphabetical file rows on the right, and one selected diff at a time. Unified and split modes share per-project settings.
- Soft-wrapped code stays inside the viewport. Split mode uses 40% original / 60% modified; added files use a single full-width pane. Pixel scroll offsets survive live snapshot updates.
- Hover + / − controls on file rows and preview headers. Whole-file actions preserve working-tree bytes and unrelated index entries, keep scope, and select a remaining neighbor when necessary.
- Sticky Stage all / Unstage all below the file list and matching palette commands apply the current scope in one Git invocation.
- Right-click project settings support names, the GPUI Kit color picker, emoji or local image icons, and removal; appearance is persisted with backward-compatible optional preference fields. Image icons have rounded square crops and fixed hover sizing; missing images fall back to the project initial.
- File-navigation taps are immediate. A hold timer starts after 320 ms and advances every 200 ms while physical Command/arrow keys remain down on macOS, checking release every 16 ms and stopping on scope/project/focus changes. Native autorepeat duplicates are suppressed. Preference saves debounce for 250 ms and skip unchanged state.
- File rows prioritize brighter filenames over muted directory prefixes, compress the directory in the middle to fit, and expose full paths on hover.
- Draggable unused toolbar regions, native title-bar double-click behavior, and a footer with short repository path/branch on the left and spaced shortcut groups on the right.
- GPUI Kit Command palette on Cmd+P with searchable project names/paths and Action-backed command execution. Escape clears a query, then closes; confirmation executes once.
- Normal launches restore saved repositories, their colors/order, active project, scopes, modes, and selected paths. Empty startup offers Open repository; --demo is isolated from preferences.
- Remove from Diffy is available from project context menus and the palette, and only removes the saved entry. Stable project IDs keep background loads safe when projects are removed or deduplicated.

## State and repository behavior

Git compares HEAD → index for Staged and index → disk plus untracked additions for Changes. A partially staged path can appear in both. File and Git metadata changes trigger debounced live reloads. Overflow scrollbars remain visible; a narrow GPUI Component patch preserves offscreen height estimates and the visible source anchor during updates. Reads and whole-file mutations run in the background, reject stale responses, block duplicate mutations, and expose recoverable errors beside the last good preview.

Preferences are versioned JSON under `~/Library/Application Support/Diffy/state.json`, saved atomically. They contain paths and viewer settings, never source contents. Storage is injectable for isolated tests and development. Missing repositories stay visible and removable; invalid preferences fall back to the empty state with an error.

Cmd+Shift+Up/Down switches projects; Cmd+Up/Down switches files; Cmd+Shift+S toggles scope; Cmd+S stages/unstages; Cmd+O opens repositories; Cmd+R refreshes. The palette exposes these operations plus diff-mode selection, focus, removal, and quit.

## Validation

Disposable Git fixtures cover independent scopes, unusual paths, renames, copies, deletions, unborn branches, index locks, and preservation of source/index state. Preference tests cover serialization, atomic replacement, invalid data, and removal. Native offscreen Metal checks exercise keyboard and pointer interactions, palette filtering/dispatch, hover actions, reduced motion, restart restoration, and empty states. Inspect the live macOS window as well and deliver a locally signed `dist/Diffy.app`.

## Later work

Add selected-file patch fetching for large repositories, exact scroll restoration across launches, Herdr agent discovery, and SSH connections. Keep transport separate from the viewer so local and remote projects can share the review model.

Evaluate Difftastic or OmniDiff behind a structured diff adapter when needed; this POC does not require another engine. OmniDiff's licensing needs a distribution decision before adoption. Livediff is watcher inspiration, and imara-diff is a candidate for non-Git comparisons.

## Diffy v0.1.0 release

Use `com.hitslop.diffy`, Cargo package/executable `diffy`, MIT licensing, and https://github.com/hitSlop/diffy. Keep the existing workspace folder. Target Apple Silicon macOS 13+ first; Windows and Intel builds are deferred.

Package with cargo-packager 0.11.8 and the supplied iconset. Sign locally using the Mushroom DAO Developer ID in the login keychain and allowlisted VISIONCAL notarization credentials. Sign and staple the app before constructing the DMG; sign the final updater archive after stapling. Keep the encrypted updater key and its password ignored in `.release-secrets`; commit only its public key.

Use the HTTPS-patched cargo-packager-updater revision `0a3ea3a089363b7e77dfca238be5bbcf3f4d75f6`. Quiet startup checks, manual menu/palette checks, explicit Update and Restart, progress, signature verification, and Git write gating are required. Preserve and migrate valid Differ preferences without replacing existing Diffy state.

Validate native UI, Git operations, migration, signatures, isolated bundle installation/relaunch, signing, notarization, stapling, and Finder launch. Upload signed assets and checksums to a draft v0.1.0 release; do not publish automatically. See RELEASING.md for concrete commands and backup instructions.
