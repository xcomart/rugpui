# WindowControls

The minimise / maximise / close buttons of a window that draws its own caption.
Only Windows and Linux need them: macOS keeps its native traffic lights even
with a transparent title bar, so the strip is left out there entirely — a second
set would be two ways to close one window.

Source: [window_controls.rs](../../crates/rugpui/src/window_controls.rs).

Most applications never construct this directly. `rugpui-shell` wraps it in
`window_control_strips`, which asks the platform what layout it wants and hands
back the pair of strips ready to render; see [shell.md](../shell.md). This page
is about the widget underneath.

## The pieces

| item | what it is |
| --- | --- |
| `WindowControls` | The element: a strip of caption buttons, drawn in the order it is given them. |
| `WindowControlIcons` | Four `SharedString` asset paths — `minimize`, `maximize`, `restore`, `close`. |
| `window_controls::split` | Turns the desktop's reported button layout into the two strips a title bar draws. Not re-exported at the crate root; reach it as `rugpui::window_controls::split`. |

```rust
pub fn split(
    layout: Option<WindowButtonLayout>,
    supported: gpui::WindowControls,
) -> (Vec<WindowButton>, Vec<WindowButton>)
```

Note the name collision: `gpui::WindowControls` is the *window's* report of what
it can do, and is unrelated to `rugpui::WindowControls`, the element.

## Builder options

`WindowControls` has one constructor and no builder methods — everything it
draws was decided before it was built.

| method | argument | effect |
| --- | --- | --- |
| `WindowControls::new` | `id: impl Into<ElementId>`, `icons: WindowControlIcons`, `buttons: Vec<WindowButton>` | Creates the strip. `id` must be unique among its siblings and — because a title bar can carry a strip at each end — among the strips themselves. |

## Minimal example

Straight against the widget, the way the shell's `window_control_strips` does it:

```rust
use gpui::WindowButton;
use rugpui::{WindowControlIcons, WindowControls, window_controls};

let (leading, trailing) = window_controls::split(cx.button_layout(), window.window_controls());

let strip = |id: &'static str, buttons: Vec<WindowButton>| {
    (!buttons.is_empty()).then(|| WindowControls::new(id, icons.clone(), buttons))
};

let leading = strip("window-controls-leading", leading);
let trailing = strip("window-controls-trailing", trailing);
```

...then rendered at the two ends of the title bar row, `leading` before whatever
the row starts with and `trailing` after whatever it ends with.

The four icon paths have to name assets the host's asset source can answer for.
`rugpui-shell` ships them: `rugpui_shell::WINDOW_CONTROL_ICONS` is the table of
`(path, bytes)` pairs to concatenate into the application's own `IconSet`, and
`rugpui_shell::window_control_icons()` builds the matching
`rugpui::WindowControlIcons`. An application that does not use the shell supplies
its own four SVGs and fills the struct by hand:

```rust
WindowControlIcons {
    minimize: "icons/window-minimize.svg".into(),
    maximize: "icons/window-maximize.svg".into(),
    restore: "icons/window-restore.svg".into(),
    close: "icons/window-close.svg".into(),
}
```

## Why two strips

A Linux desktop publishes a button layout — GNOME's `button-layout` gsetting,
or the KDE equivalent — and putting the close button on the left is a setting
people actually use. So `split` returns a left list and a right list, either of
which may come out empty, and the title bar renders a strip at each end.

`split`'s second argument is the *window's* answer rather than the desktop's,
and the two disagree often enough to matter: a compositor may offer no minimise
while the layout still names one. A button the window cannot perform is dropped
wherever it appears. Close is never dropped — no platform reports it as
unsupported, and a caption with no way to close the window would be a trap.

`layout` is `None` off Linux, and also for a Linux desktop that publishes
nothing; that means the familiar minimise / maximise / close on the right.

## State the host keeps

None. The strip is stateless like the rest of the kit: it reads the window's own
`is_maximized()` to pick between the maximise and restore glyphs, and draws
exactly the buttons it is handed, in the order it is handed them. Both decisions
were already made by `split`.

## Mouse, and the two wirings

Each button is wired twice over, deliberately:

- It marks itself as a `WindowControlArea` (`Min`, `Max`, `Close`). That is what
  Windows needs: the hit test then reports the area as a caption button, the
  window procedure performs the action natively, and on Windows 11 the maximise
  button offers the snap layouts on hover. That path never delivers a click to
  the app.
- It also carries an `on_click` — `window.minimize_window()`,
  `window.zoom_window()`, `window.remove_window()` — which is what runs
  everywhere else.

Every button also `occlude()`s. The strip sits inside the toolbar's drag area,
and without occlusion the drag hitbox would answer the hit test first and the
buttons would read as "move the window".

## Sizing

One button is 46 px wide, matching the caption buttons Windows draws, and fills
the height of its parent. The glyph inside is 12 px — half a toolbar icon,
because a caption glyph is meant to read as a hairline mark rather than as a
control of its own. Neither is configurable.

## Theme slots

- `surface` — the strip's background.
- `border` — the hairline along its bottom edge.
- `surface_hover` — the hover fill of minimise and maximise.
- `icon` — the resting glyph colour. Not `text_muted`: a glyph is a solid run of
  pixels while an icon is a hairline, and an antialiased hairline arrives on
  screen weaker than the text beside it, so `icon` is derived to clear a
  contrast floor against both `background` and `surface`. See
  [theming.md](../theming.md).
- `text` — the glyph on a hovered minimise or maximise button.

The close button's hover is the one hardcoded colour in the widget layer, and
the only one that has to be: `#E81123` fill with a `#FFFFFF` glyph is exactly
what Windows paints under a close button, and a themed shade would read as a
different control.

## Pitfalls

- Because the caption glyphs are the smallest thing the app draws, the four
  assets should carry a heavier stroke than the rest of the icon set. The ones
  in `rugpui_shell::WINDOW_CONTROL_ICONS` already do.
- Without an asset source registered on the `Application`, gpui answers every
  icon path with `None` and the buttons paint as empty boxes.
- Each button gets its own style group name, because a `group_hover` resolves
  against the nearest ancestor carrying the name and the three buttons are
  siblings. Nothing to configure, but it explains why hovering one does not
  light up its neighbours.
- Do not render a strip on macOS. `window_control_strips` already returns
  `(None, None)` there; a hand-rolled caller has to check for itself.
