# tooltip_label

A one-line label that appears when the pointer rests on a control. Use it to
name an icon-only button or to spell out an abbreviation; anything longer than a
few words is documentation and belongs in a guide rather than under the cursor.

Source: [tooltip.rs](../../crates/rugpui/src/tooltip.rs).

## Why this is a function and not a widget

gpui asks for tooltips as a *builder*: `.tooltip(f)` stores `f` and calls it to
make a fresh view each time the pointer settles. The view has to be an `AnyView`,
so a tooltip cannot be a plain element the way the rest of this crate's widgets
are — it needs an entity behind it. `tooltip_label` hides that. It takes the text
once and hands back exactly the closure `.tooltip` wants:

```rust
pub fn tooltip_label(
    label: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static
```

## Minimal example

From the gallery, hung on an ordinary `div`:

```rust
use rugpui::tooltip_label;

div()
    .id("tooltip-target")
    .px(px(8.))
    .py(px(4.))
    .rounded_md()
    .border_1()
    .border_color(palette.border)
    .text_color(palette.text_muted)
    .tooltip(tooltip_label("Rests here to show a tooltip"))
    .child("Hover me")
```

Or on an icon:

```rust
div().id("save").tooltip(tooltip_label("Save")).child(icon)
```

`.tooltip(..)` is gpui's own method on a stateful element, so the element needs
an `.id(..)` — a tooltip on an id-less `div` will not compile.

## Options

There are none. `tooltip_label` takes a single argument and has no builder:

| item | argument | effect |
| --- | --- | --- |
| `tooltip_label` | `label: impl Into<SharedString>` | Returns the `.tooltip(..)` callback showing `label`. |

The text is captured once and cloned per hover, so the caller can hand over a
localised string without keeping it alive itself.

## State the host keeps

None. There is no open flag, no timer and no anchor to store — gpui owns the
hover timing and the tooltip's lifetime entirely. This is the one widget in the
kit where "the host keeps nothing" is literally true, since even the id belongs
to the element the tooltip is attached to.

## Positioning

Nothing here positions anything. gpui lays the view out at the pointer and, when
the box would cross a window edge, flips it to the other side of the cursor on
that axis. Adding an `anchored` or a `deferred` around it would fight machinery
that has already done the work.

The one adjustment the widget makes is a 16 px top **margin**. gpui puts the
tooltip one pixel from the mouse position, which is the *tip* of the arrow
cursor and therefore underneath the rest of the glyph; the margin clears it so
the first word is not read through the pointer. It is a margin rather than an
offset passed to gpui precisely because a margin is part of the measured size,
so the edge-flipping above still sees the box the user actually sees.

## Theme slots

The styling is the [menu](./menu.md) panel's, one step quieter — a tooltip is
read and dismissed rather than clicked, so it takes `surface` instead of the
menu's page background and a softer shadow, which keeps it from reading as
something that can be pressed.

- `surface` — the box's background.
- `border` — its outline.
- `text` — the label, at 11 px.

## Pitfalls

- The label never wraps (`whitespace_nowrap`). A long string produces a very wide
  box rather than two lines; keep tooltips to a few words.
- Several widgets in this crate take tooltip text directly rather than needing
  this helper — [`TabBar::tooltips`](./tab-bar.md) and
  [`MenuButton::tooltip`](./menu.md) among them. Use those where they exist;
  `tooltip_label` is for the host's own elements.
- The closure builds a new entity on every hover. That is gpui's design, not an
  inefficiency to work around by hoisting the view out.
