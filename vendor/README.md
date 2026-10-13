# GPUI Component patch

`gpui-component` is the crates.io 0.7.3 package with small changes in the diff
state, renderer, and document modules:

1. Reapply diff row-height estimates after a list width change has invalidated
   them in GPUI's prepaint. This preserves the complete scroll range for large
   files from the first frame, without rendering all rows eagerly.
2. Add `DiffState::set_files_preserving_position`, which retains the top visible
   source-line anchor, fractional row offset, and horizontal offset when replacing a live snapshot.
3. Render added files as one full-width column in either display mode.
4. Expose `Diff::split_ratio` (default 0.5). Differ uses 0.4 with soft wrapping
   to keep both sides visible and give the modified version more space.

Differ selects this package with a Cargo patch. The native integration test in
`tests/ui.rs` verifies wheel/keyboard scrolling to the last line of a 3,000-line
file with a very long wrapped line, both diff modes, pixel-precise wheel movement, full-width additions, and reading-position retention
while live edits arrive. Keep these tests when removing the patch after an
upstream fix. Upstream licensing is in `gpui-component/LICENSE-APACHE`.
