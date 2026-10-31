# Changelog

Notable changes to the rugpui crates (`rugpui`, `rugpui-grid`, `rugpui-editor`,
`rugpui-shell`) and the vendored gpui patches. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
the workspace version in `Cargo.toml`. The crates are consumed by Git revision,
so each entry also says what a consumer has to change when it moves its pinned
revision past it.

## [Unreleased]

### Added

- A UI scale. `rugpui::set_ui_scale(factor, cx)` installs a factor as a
  global (clamped to 0.5–3.0, redrawing every window), `TextSize::{Xs, Sm,
  Body, Title}` names the four text sizes the widgets draw at (11, 12, 13 and
  14 px at the default factor) and `rugpui::scaled(px, cx)` converts any other
  size. An application maps its "interface size" onto the factor and converts
  its own fixed sizes the same way.
- `modal(..)` returns a `Modal` with opt-in options: `initial_focus(&handle)`
  and `autofocus(true)` take the focus as the dialog opens and give it back
  when it goes, `on_confirm` answers `Enter` on the panel or in a one-line
  field without a submit of its own, `escape_dismisses(true)` sends `Escape`
  to the dismiss handler, `backdrop_dismisses(false)` makes the backdrop
  inert, and `dismiss_guard` is asked — with a `DismissReason` — before either
  gesture dismisses. `FocusReturn` captures and restores the focus for an
  overlay that is not a modal.
- `form_row(..)` returns a `FormRow` with `label_width(..)` and `stacked(..)`,
  and `fit_label_width(labels, window, cx)` measures a form's labels into a
  column between 96 and 200 px.
- `Button::focus_handle(&handle)`, so a dialog can focus one of its buttons,
  and `Button::truncate(true)`, which lets a button shrink below its label,
  cut it off with an ellipsis and show the whole label as a tooltip while it
  is cut.
- Menus work from the keyboard. An open `MenuButton` or `ContextMenu` takes
  `Up`, `Down`, `Home`, `End`, `Enter`, `Space`, `Escape`, `Tab` and typed
  letters, skipping separators and disabled rows, while the focus stays on the
  surface it was opened over. `highlight_first(true)` opens a menu on its first
  row, `MenuButton::tab_index(..)` makes the trigger a tab stop that opens with
  `Enter`, `Space` or `Down`, and `rugpui::init` binds `Shift+F10` and the Menu
  key to the new `OpenContextMenu` action and `F10` to `OpenAppMenu`, with no
  key context, for the surfaces that own the menus to handle.
- A middle click on a closable tab asks `TabBar::on_close` to close it.
- `WindowControls::on_close(handler)` runs the handler in place of removing
  the window.
- `rugpui_shell::close_guard`: `set_close_guard` installs a per-window guard
  that the caption's close (`request_close`), the platform's close (through
  `on_window_should_close`), `request_quit` and `request_restart` all ask, and
  `proceed` performs the intent once the user has said to go on. `CloseGuard`
  is the ready-made guard, putting up `UnsavedWorkDialog` (Cancel, the gesture
  "anyway", optional Save all) when the application reports work it would
  lose. Its words are the new `close_guard.*` keys.
- `rugpui_shell::toast`: `show_toast(Toast::info | warning | error(..), cx)`
  from anywhere with an `App`, drawn by `toast_stack()` in the window's
  bottom-right corner. Info leaves after four seconds, a warning after eight,
  an error stays until closed.
- `rugpui_grid::GridRequest`, a second event a `GridView` raises once its host
  opts in with `key_requests(true)` (or `set_key_requests`): `Delete` (and
  `Backspace` on macOS) with a selection, `F2` or a printable character on an
  editable cell (`Edit`, carrying the typed text), and the paste chord with
  text on the clipboard (`Paste`). Off by default, and whenever there is
  nothing to act on, those keys pass through to whatever they reached before.
  `GridView::begin_edit_with` opens the field holding the typed text and still
  stages it against the cell's own value.
- `App::prompt_for_paths_in` in the vendored gpui: an open-file prompt that
  starts in a folder, the counterpart of the folder `prompt_for_new_path`
  takes. It is a new method rather than a `PathPromptOptions` field, so
  existing struct literals are untouched.
- In the editor on macOS, `Cmd+G` and `Cmd+Shift+G` are Find Next and Find
  Previous, beside F3.

### Changed

- Every widget's text, and the heights that hold it, follow the UI scale:
  buttons, checkboxes, switches, segmented controls, text fields, dropdowns,
  menus, tabs, trees, lists, tooltips, dialogs, the grid's rows, header and
  gutter, the editor's find bar and code tooltips, and the shell's dialogs. At
  the default scale nothing moves. A change of scale refits the grid's fitted
  columns.
- A one-line `TextInput` without `on_submit` passes `Enter` on instead of
  swallowing it, which is what lets a dialog around it take the key as its
  default action. A host that relied on `Enter` doing nothing in such a field
  under a binding of its own gives the field an `on_submit`.
- `modal` and `form_row` return the concrete `Modal` and `FormRow` instead of
  `impl IntoElement`; both are elements, so existing call sites compile as
  they were.
- The strips `rugpui_shell::window_control_strips` builds send their close
  button through `request_close`, which closes the window at once when no
  guard is installed — exactly as before.
- On macOS, `Cmd+G` and `Cmd+Shift+G` take the key in an editor even with
  nothing to find, so a host's own `Cmd+G` at a wider context no longer fires
  while an editor has the focus.
- A `GridView` subscription closure that ignores its event entirely now has
  to name its type (`event: &GridEvent`), since the grid emits two event
  types; closures that match on `GridEvent` variants infer it as before.

### Fixed

- A `Checkbox` or `Switch` label longer than the room it has wraps onto more
  lines instead of running past its container on one: the label, and the
  row itself, may now be narrower than the label's one-line width. The box or
  the track stays beside the first line of a wrapped label rather than moving
  to the middle of it; a label that fits draws exactly as before. A host that
  puts one in a row of its own still gives the item between them `min_w_0()`
  for the label to have a width to wrap at.
- A `modal` title too long for the panel wraps inside the title band, which
  grows to hold it, instead of running out past the panel's border. A title
  that fits keeps the band's 44 px.
- The update dialog's download heading gives way to the percentage beside
  it rather than pushing it out of the dialog.

## [0.1.13] - 2026-10-29

### Added

- A grid cell's one-line field can carry a button that hands the edit to a
  larger editor of the host's own. A source opts in per cell with
  `GridSource::cell_expandable`; pressing the button closes the field with
  nothing staged and raises `GridEvent::ExpandRequested` with what had been
  typed. `GridView::expand_edit` does the same from code,
  `GridView::is_expandable` says whether the open field offers it, and
  `GridView::expand_tooltip` / `set_expand_tooltip` label the button. Its mark
  is the new `rugpui::EXPAND`, in `rugpui::ICONS`.

### Fixed

- A single-line `TextInput` slides its text left under the field to keep the
  caret in view. Until now a line longer than the field ran on past its
  right-hand edge, caret and all, where it was cut off out of sight — and in
  a grid field with an expand button beside it the caret seemed to go under
  the button. The text is cut at the field's own edges, and a press, a
  selection and an input method's candidate window all follow the slide.

### Changed

- `TabBar` runs the parts of a strip its owner leaves to it, off its own
  per-element state. Given no `.scrollbar(...)` it draws the overlay bar along
  its bottom edge: scrolling the tabs or resting the pointer on that edge puts
  it up, and the thumb can be dragged. Given no `.on_menu_open_change(...)` it
  still shows the dropdown listing every tab, and opens and closes it itself.
  Given no `.scroll_handle(...)` it scrolls with a handle of its own. A tab that
  becomes active is scrolled into view. Until now a strip whose owner kept no
  `ScrollbarState`, hide timer and drag listener scrolled without a bar, and
  one without the handler had no dropdown at all. A consumer that passes its
  own bar, handle or open state keeps it; one that ran a bar only for a tab
  strip can drop that state and pass nothing. A strip that left the dropdown
  out by setting no handler now shows it.
- `locale::check_locale_dir` also fails on an unquoted value with a `#` after
  a space. YAML reads the rest of such a line as a comment, so the value loads
  cut short — `Every color has to be written as #RRGGBB.` showed as only the
  words before the `#` — and the failure names the file, the key and what was
  lost. A consumer whose locale files have such a value has to quote it before
  its locale test passes again.
- Identifiers, comments and docs use US spelling. `rugpui-shell`'s
  `menu_rows::greyed` is now `grayed`, and `ThemeEditorEvent::Cancelled` is
  `Canceled`; a consumer that calls the one or matches the other has to
  rename it. The vendored gpui keeps its upstream spelling.

## [0.1.12] - 2026-10-26

### Added

- `rugpui-editor`: `EditorView::set_tab_width(width, cx)` and
  `EditorView::tab_width()`, with `DEFAULT_TAB_WIDTH` (4) and `MAX_TAB_WIDTH`
  (16). A consumer that wants a width other than four for a document calls
  the setter; nothing else changes.

### Fixed

- `rugpui-editor` draws a tab to the next tab stop instead of as whatever the
  font has at U+0009, which in most faces was next to nothing. Tab stops are
  counted in the cells a monospaced font draws a character across, so a tab
  after a Hangul word lines up with one after a Latin word. The caret, the
  selection, a click, the IME candidate box and word wrap all follow the
  drawn width, and `Up`/`Down` aim for the column a tab reaches. The buffer
  keeps its tabs. `CodeSnippet` draws tabs the same way, at the default
  width. A consumer changes nothing.

- `Select` and `SchemeSelect` draw the overlay scrollbar down a long open list
  by themselves when the caller passes none through `.scrollbar(...)`, run off
  the dropdown's own state: scrolling the list or resting the pointer on its
  right edge puts the bar up, and the thumb can be dragged. Until now only the
  lists whose owner kept a `ScrollbarState`, a hide timer and a drag listener
  had one. A consumer changes nothing; one that passes its own bar keeps it.

- A tooltip no longer opens under the pointer. gpui put the box one pixel
  from the cursor's hot spot, under the arrow, and the 16 px margin
  `tooltip_frame` added never took effect, since a root's margin is not laid
  out. The vendored gpui now puts the box 24 px below the hot spot, flips it
  above the pointer at the foot of the window, slides it sideways to stay in
  the window, and stands a box too tall for either side beside the cursor,
  all inside `Window::client_content_bounds`, so the shadow band of a
  client-decorated window no longer cuts one off. `tooltip_frame` drops the
  margin. A consumer changes nothing.

## [0.1.11] - 2026-10-23

### Added

- `rugpui-shell`: `apply_client_inset(window, tiling)` sets the client inset
  and the input region of a client-decorated window together, and clears both
  under server-side decorations. A consumer replaces its own
  `window.set_client_inset(...)` branch on `client_tiling` with this one call.
- The vendored gpui implements `Window::set_input_region` on X11, through the
  input shape of the X Shape extension, and neither Linux backend sends a
  region the window already has again.
- The vendored gpui has `WindowOptions::client_inset`, and `rugpui-shell`
  `opening_client_inset(style)` to fill it. On X11 a window opened with it is
  created with the band around its bounds and `_GTK_FRAME_EXTENTS` already
  set when it is mapped. A consumer adds
  `client_inset: opening_client_inset(titlebar)` to its `WindowOptions`.
- `rugpui-shell`: `window_bounds_at_least(saved, width, height, maximized,
  min_size, client_inset, cx)`, `window_bounds` with the opening size held to
  a visible minimum (the shadow band added on Wayland, which opens at the outer
  size). A consumer that sets `WindowOptions::window_min_size` opens with this
  instead, so a smaller saved size comes back at the minimum.

### Changed

- `rugpui-shell`: the resize grip of a client-decorated window is the
  `RESIZE_GRIP` (10 px) strip along the inner edge of the shadow band instead
  of the whole 36 px band, and `RESIZE_CORNER` is `RESIZE_GRIP + 12`. The band
  keeps its width, so the shadow is unchanged.

### Fixed

- `rugpui-shell`: a click in a client-decorated window's shadow, outside the
  resize grip, reaches the window underneath instead of starting a resize.
- A client-decorated window no longer shrinks by the shadow band on every
  launch under xfwm4 (Xfce on X11). The frame extents arrived after the window
  was mapped, which xfwm4 takes out of the visible window where Mutter grows
  the window around them; opened with `client_inset`, they are there before.
- The vendored gpui holds `WindowOptions::window_min_size` to the visible
  window on X11, as Wayland does: the size hint is scaled to device pixels and
  carries the frame extents, where it used to be the logical size of the whole
  X window, band included.

## [0.1.10] - 2026-10-22

### Added

- The vendored gpui has `Window::client_content_bounds`, the viewport less
  `client_inset` on every edge that is not tiled: what a client-decorated
  window shows, in window coordinates. `anchored` snaps against it, and rugpui's menus,
  dropdown lists and dialogs take their height caps from it, so all of them
  agree on where the window ends.

### Changed

- `rugpui-shell`: `window_geometry` takes the `App` as well as the `Window`,
  `window_geometry(window, cx)`, to tell the Linux backends apart. A consumer
  that records its window geometry passes `cx` at the call site.
- `rugpui-shell`: `SHADOW_BAND` is 36 px instead of 24 px, and the focused
  window's shadow blurs over 12 px instead of 14 px. Code that lays anything out
  against the band reads the constant rather than repeating the number.

### Fixed

- `rugpui-shell`: an update check the releases API turns away for its
  anonymous rate limit (`403` or `429`) asks the releases page instead. The
  `latest` redirect names the tag, and the release's `SHA256SUMS` (or the
  registered `SignedChecksums` list) supplies the asset's digest, so the
  update still installs. The size comes from a `HEAD` on the download, and
  the release notes are unknown that way, so the dialog shows none.
- `rugpui-shell`: the focused window's shadow is no longer cut off at the edge
  of its band; a test checks that the band holds the tail past 2.75 sigma.
- `rugpui-shell`: `window_geometry` records the size the platform opens the
  window back at. X11 adds the shadow band around the size asked for, so the
  inner bounds are recorded there and the window no longer grows by the band
  on every launch; Wayland opens a window at its outer size, so the outer
  bounds are recorded there and it no longer shrinks.
- `rugpui`: under client-side decorations, snapped popups and menus stay clear
  of the transparent shadow band. The vendored `gpui/src/elements/anchored.rs`
  checks the snapping fit modes against `Window::client_content_bounds`
  (`SwitchAnchor` keeps the whole surface), and the menu height cap leaves the
  band out on both edges.
- Windows builds open under Wine. The vendored `gpui_windows` opens a window
  without Direct Manipulation when the system cannot create it, falling back
  to plain wheel messages for touchpad scrolling and pinch, and
  `gpui_windows/src/direct_write.rs` resolves fonts through the collection's
  family lookup when it has no font set.
- rustls-webpki was updated to 0.103.16 and webpki-roots to 1.0.10 for
  RUSTSEC-2026-0382 and -0381. A consumer running `cargo deny` updates both in
  its own lockfile with `cargo update -p rustls-webpki -p webpki-roots`.
- `rugpui`: a `modal` taller than a client-decorated window keeps its top and
  bottom edges. Its height cap came off the whole viewport, which since the
  shadow band widened to 36 px is taller than the content it is centered in,
  so a settings dialog in a short window lost its top and bottom edges.
- `rugpui`: an open `Select` or `SchemeSelect` list is no taller than the
  window shows, less its margins, and scrolls inside that instead of running
  into the shadow band and being cut off there.

## [0.1.9] - 2026-10-19

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
  square of the given color. A menu with any swatched row lays the square's
  column out on every row; a `Select` reserves nothing on a row without one and
  shows the selected option's square on the trigger. `SelectOption` has a new
  public field, `swatch: Option<Hsla>`, and no longer derives `Eq`; code that
  builds a `SelectOption` literal has to supply the field.
- `rugpui`: `ColorPicker`, a stateless color picker — a saturation/value
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
- gpui moves to Zed revision `56934ec0560bce8929025497411de7f5322fb6ca`
  (2026-10-17) from `fd82517a115d97a07835b52f0512b22b38e38ccf`. A consumer
  moves the `rev` of its `gpui` and `gpui_platform` entries to the new
  revision together with its rugpui revision. Upstream API changes reach
  application code too: `Window::blur` now takes `&mut App`, for one.
- The vendored X11 `WM_DELETE_WINDOW` patch is gone: upstream now runs
  `should_close` and `close` outside the client borrow itself (zed#63089).
- `zlog`, `ztracing` and `ztracing_macro` are Apache-2.0 upstream now
  (zed#63573), so the GPL-3.0 license exceptions are removed from `deny.toml`.
  wgpu comes from a pinned git revision of `gfx-rs/wgpu`, which a consumer
  running `cargo deny` has to allow as a source.

### Fixed

- `rugpui`: a `modal`'s backdrop occludes the window behind it. A press on the
  backdrop still dismisses the dialog, but no longer also reaches what it was
  drawn over, and the window behind no longer takes the wheel or hover while
  the dialog is up.
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

[Unreleased]: https://github.com/xcomart/rugpui/compare/v0.1.13...dev
[0.1.13]: https://github.com/xcomart/rugpui/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/xcomart/rugpui/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/xcomart/rugpui/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/xcomart/rugpui/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/xcomart/rugpui/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/xcomart/rugpui/compare/v0.1.7...v0.1.8
