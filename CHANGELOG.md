# Changelog

Notable changes to the rugpui crates (`rugpui`, `rugpui-grid`, `rugpui-editor`,
`rugpui-shell`) and the vendored gpui patches. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
the workspace version in `Cargo.toml`. The crates are consumed by Git revision,
so each entry also says what a consumer has to change when it moves its pinned
revision past it.

## [Unreleased]

### Added

- `rugpui-shell`: the update dialog shows the release notes from the GitHub
  release body, without the part GitHub generates. `Release` has a new public
  field, `notes: Vec<release_notes::Block>`; code that builds a `Release`
  literal has to supply it.
- `rugpui-editor`: `HistoryLimits` and `EditorView::set_history_limits` (also
  `History::with_limits`, `History::limits` and `History::set_limits`) cap the
  undo history by steps and by bytes of stored text. The default is 10,000 steps
  or 64 MiB, and `HistoryLimits::UNBOUNDED` keeps the earlier behavior.
- `rugpui`: an open `ContextMenu`, and a `MenuButton` dropdown, close on
  `Escape` through their existing dismiss callback (`on_dismiss`,
  `on_open_change(false)`), wherever the focus is. The key is taken ahead of
  the focused surface's bindings and does not reach them; a consumer that
  closed its menus on `Escape` itself can drop that code.
- `rugpui-shell`: `PaneTree::swap` exchanges the places of two panes. Each
  pane moves with its id, and the splits and their ratios stay where they are.
- `rugpui`: `MenuEntry::swatch` and `SelectOption::swatch` lead a row with a
  square of the given colour. A menu with any swatched row lays the square's
  column out on every row; a `Select` reserves nothing on a row without one and
  shows the selected option's square on the trigger. `SelectOption` has a new
  public field, `swatch: Option<Hsla>`, and no longer derives `Eq`; code that
  builds a `SelectOption` literal has to supply the field.
- `rugpui`: `ColorPicker`, a stateless colour picker — a saturation/value
  field over a hue strip, a `#rrggbb` field kept in step with both, optional
  preset swatches, a tab stop and keys —
  working in the new `Hsv` type, which converts to and from `Hsla`, `Rgba` and
  `#rrggbb`. `DraggedColor` is its drag payload. The gallery shows one under
  the controls column.
- `scripts/vendor_check.py` diffs `vendor/` against upstream Zed at the pinned
  revision and fails on an unmarked hunk or a patched file missing from the
  patch list in `Cargo.toml`.

### Changed

- `rugpui-editor`: `EditorView::is_dirty` reports whether the history stands on
  the saved state, so undoing back to the text last passed to `set_text` or
  `mark_clean` reads clean again, and undoing or redoing away from it reads
  dirty. Typing after `mark_clean` starts a new undo step. `is_dirty` is no
  longer a `const fn`.
- `rugpui-editor`: the undo history drops its oldest steps beyond the default
  limits instead of growing without bound. Undo cannot return past a dropped
  step, so a save older than the oldest kept step cannot become clean again.
- The library crates are marked `publish = false`, because they are used only
  by Git revision.

### Fixed

- The `RULOGMAN PATCH` markers and patch list in `Cargo.toml` now cover every
  vendored change, including `gpui_windows/src/events.rs` and
  `gpui_macos/src/gpui_macos.rs`. This changes comments only.

## [0.1.8] - 2026-10-17

### Added

- `rugpui`: tabs can be dragged to reorder within a strip or moved to another
  strip. `TabBar::drag_source` tags a strip, `DraggedTab { source, index }`
  describes the tab in flight, and `TabBar::on_reorder`,
  `TabBar::on_drag_over` and `TabBar::drop_indicator` report and draw the
  drop position. `rugpui-shell`: `Pane::insert` and `Pane::relocate` apply the
  move, and `PaneId::from_u64` restores a pane from its drag tag.
- `rugpui`: `TabDropState` replaces the drag-over, resolve and clear logic each
  application kept beside its tab strips.
- `rugpui`: keyboard navigation for `TabBar`. With `TabBar::focus_handle`, the
  bar binds Left/Right/Home/End to select a tab, Delete to close the active
  closable tab and Ctrl+Shift+PageUp/PageDown (Cmd on macOS) to move it through
  `on_reorder`. The focused strip draws a ring around the active tab.
  `rugpui::init` registers the bindings.
- `rugpui-shell`: `SignedChecksums` and `require_signed_checksums` let an
  application require a signed `SHA256SUMS` list beside each release archive,
  verified with the application's own verifier.

### Changed

- `rugpui`: **behavior change.** `TabBar::on_reorder` now receives the insertion
  gap reported by the last drag hover, which is the same index passed to
  `on_drag_over` and drawn by `drop_indicator`, rather than the tab under the
  drop. The strip reports `None` outside its scrolling row, including over the
  dropdown and new-tab button, and reports once per move. The insertion bar is
  drawn over the tabs instead of shifting them.

### Fixed

- `rugpui-shell`: **behavior change.** The updater refuses an asset when the
  releases API gives no SHA-256 digest for it. It no longer falls back to
  checking only the size.
- `rugpui-editor`: find and replace search again after each buffer change, so
  replace and replace-all no longer use stale ranges. `unmark_text` commits the
  marked text, and any edit, undo or redo first ends a running IME composition,
  so the history stays in step with the buffer.
- `rugpui-shell`: a NaN pane split ratio is refused.
- rustls was updated to 0.23.46 for RUSTSEC-2026-0285, -0346, -0348 and -0349.

[Unreleased]: https://github.com/xcomart/rugpui/compare/v0.1.8...dev
[0.1.8]: https://github.com/xcomart/rugpui/compare/v0.1.7...v0.1.8
