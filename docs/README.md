# rugpui documentation

These pages are a guide, not the reference: the reference is the rustdoc
(`cargo doc --workspace --no-deps`), generated from the same doc comments the
code is built from. What follows is the tour — the "why this shape" and the
worked examples that rustdoc doesn't have room for. Every snippet on these
pages was taken from `crates/rugpui-gallery`, the runnable example that puts
every widget below in one window; when a page and the gallery disagree, the
gallery is right.

## Start here

- [Getting started](./getting-started.md) — from an empty crate to a window
  with a rugpui widget in it.
- [Theming](./theming.md) — the two palettes, `Theme` and `EditorTheme`, and
  how a host picks and stores them.

## The widget kit (`rugpui`)

| page | widget | one line |
|---|---|---|
| [button](./widgets/button.md) | `Button` | A stateless push button with four visual weights. |
| [checkbox](./widgets/checkbox.md) | `Checkbox` | A labelled on/off box for a setting the user reads as a list of independent choices. |
| [switch](./widgets/switch.md) | `Switch` | A labelled on/off switch — a track with a knob that slides from one end to the other. |
| [segmented](./widgets/segmented.md) | `Segmented` | A horizontal one-of-many strip, used where a group of radio buttons would otherwise go. |
| [slider](./widgets/slider.md) | `Slider` | A horizontal slider over a fraction from `0.0` to `1.0`. |
| [progress](./widgets/progress.md) | `ProgressBar` | A thin horizontal bar showing how far along a piece of work is. |
| [spinner](./widgets/spinner.md) | `Spinner` | A rotating arc that says work is under way without saying how much is left. |
| [text-input](./widgets/text-input.md) | `TextInput` | A focusable text field: one line by default, several rows on request. |
| [select](./widgets/select.md) | `Select` | A dropdown that picks one string out of a list. |
| [menu](./widgets/menu.md) | `MenuButton` / `ContextMenu` | Two ways to show the same list of commands: a compact toolbar trigger and a panel with no trigger at all. |
| [tab-bar](./widgets/tab-bar.md) | `TabBar` | A horizontal strip of tabs with a status dot, an optional mark, close buttons and a dropdown listing every tab. |
| [tooltip](./widgets/tooltip.md) | `tooltip_label` | A one-line label that appears when the pointer rests on a control. |
| [modal](./widgets/modal.md) | `modal` / `form_row` | A centred dialog panel over a translucent backdrop, plus the labelled row its body is usually built out of. |
| [scrollbar](./widgets/scrollbar.md) | `Scrollbar` | An overlay scroll indicator: a thumb with no track behind it, drawn over the content rather than beside it. |
| [tree](./widgets/tree.md) | `TreeView` | A virtualised tree whose branches arrive one round trip at a time. |
| [scheme-select](./widgets/scheme-select.md) | `SchemeSelect` | A dropdown that picks one colour scheme out of a list. |
| [editor-theme-picker](./widgets/editor-theme-picker.md) | `EditorThemePicker` | A grid of selectable cards, each previewing one editor theme. |
| [window-controls](./widgets/window-controls.md) | `WindowControls` | The minimise / maximise / close buttons of a window that draws its own caption. |

## The larger crates

- [Grid](./grid.md) — `rugpui-grid` is one widget, `GridView`, over a table of
  rows it never fetches itself.
- [Editor](./editor.md) — `rugpui-editor` is a multi-line code editor: a rope,
  a pluggable line highlighter, an incremental syntax cache, and a gpui
  element that shapes only the rows that fit on screen.
- [Shell](./shell.md) — `rugpui-shell` is the layer above the widget kit: the
  parts of an application that turned out not to be about the application at
  all.

## Reading order

- **Embedding a form** — [Getting started](./getting-started.md), then
  [Theming](./theming.md), then whichever widgets the form needs:
  [text-input](./widgets/text-input.md), [select](./widgets/select.md),
  [checkbox](./widgets/checkbox.md)/[switch](./widgets/switch.md), and
  [modal](./widgets/modal.md) if the form is a dialog.
- **Embedding the grid or the editor** — [Getting started](./getting-started.md)
  and [Theming](./theming.md) first, since both crates read the same `Theme`;
  then [Grid](./grid.md) or [Editor](./editor.md) directly — each is a single
  widget with its own source trait and doesn't need the rest of the kit.
- **Writing a theme** — [Theming](./theming.md) end to end, then
  [scheme-select](./widgets/scheme-select.md) and
  [editor-theme-picker](./widgets/editor-theme-picker.md) for the pickers a
  settings dialog offers the user to switch between themes.
