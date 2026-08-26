# Button

A stateless push button with four visual weights. Reach for it wherever a click
should run an action; anything that stores a value belongs in
[`Checkbox`](./checkbox.md), [`Switch`](./switch.md) or
[`Segmented`](./segmented.md) instead.

Source: [button.rs](../../crates/rugpui/src/button.rs).

## Minimal example

`Button` owns nothing, so it is rebuilt on every render of its parent. The
gallery draws one of each variant:

```rust
use rugpui::{Button, ButtonVariant};

row()
    .child(Button::new("primary", "Connect"))
    .child(Button::new("secondary", "Cancel").variant(ButtonVariant::Secondary))
    .child(Button::new("ghost", "Reset").variant(ButtonVariant::Ghost))
    .child(Button::new("danger", "Drop").variant(ButtonVariant::Danger))
    .child(Button::new("disabled", "Connect").disabled(true))
```

(`row()` there is the gallery's own two-line helper for
`div().flex().flex_row().flex_wrap().items_center().gap(px(8.))`, not part of
this crate.)

With a handler, from inside a view:

```rust
Button::new("connect", "Connect")
    .variant(ButtonVariant::Primary)
    .on_click(cx.listener(|this, _event, _window, cx| this.connect(cx)))
```

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `Button::new` | `id: impl Into<ElementId>`, `label: impl Into<SharedString>` | — | Creates a `Primary` button. `id` must be unique among the button's siblings. |
| `variant` | `ButtonVariant` | `Primary` | Visual weight; see the table below. |
| `disabled` | `bool` | `false` | Halves the opacity of background, border and label, drops the hover and press styles, and stops `on_click` from being wired at all. |
| `full_width` | `bool` | `false` | Stretches the button across its parent's width (`w_full`). |
| `compact` | — (no argument) | off | Shrinks the button from 30 px tall / 12 px padding / 13 px text to 20 / 8 / 11, for a dense toolbar or status bar. |
| `tab_index` | `isize` | none | Places the button in the window's tab order. |
| `on_click` | `impl Fn(&ClickEvent, &mut Window, &mut App) + 'static` | none | Click callback. Ignored while the button is disabled. |

Note that `compact()` takes no argument — it is a switch, not a setter, unlike
`disabled(bool)` and `full_width(bool)`.

## Variants

| variant | when to use | drawn with |
| --- | --- | --- |
| `ButtonVariant::Primary` | the single main action of a view | `accent` fill, label in `background` |
| `ButtonVariant::Secondary` | everything alongside the main action | `surface_hover` fill, `border` outline, label in `text` |
| `ButtonVariant::Ghost` | dense toolbars, where a fill would be noise | transparent until hovered, then `surface_hover`; label in `text` |
| `ButtonVariant::Danger` | destructive actions | `danger` fill, label in `background` |

## State the host keeps

None. There is no pressed or hovered flag to store — gpui drives both from the
element's own styles. The only state a button implies is whatever its handler
mutates and whatever decides `disabled(..)`, and both live in the host view.

## Keyboard and mouse

- Hover and press are styled through gpui's `hover`/`active` styles. The hover
  and press fills are the variant's base colour shifted by ±6% lightness
  (`surface_active` darkened by 4% for `Secondary`), computed by
  `rugpui::theme::shift_lightness`.
- A button given `tab_index` draws an accent outline while focused and is
  activated by `Enter` or `Space`, which gpui delivers as an ordinary click.
- A disabled button is skipped by the tab order entirely, mirroring how the
  platform treats a disabled control: the `tab_index` call is filtered out
  rather than merely ignored.

## Theme slots

Read at render time from the global [`Theme`](../theming.md):

- `accent` — `Primary` fill, and the focus outline of every variant.
- `danger` — `Danger` fill.
- `background` — the label colour on the two filled variants.
- `surface_hover` / `surface_active` — `Secondary` fill and its press state, and
  the hover/press fills of `Ghost`.
- `border` — the `Secondary` outline.
- `text` — the label on `Secondary` and `Ghost`.

## Pitfalls

- The outline is always drawn, merely transparent for variants that have none.
  That is deliberate: gaining focus recolours the border instead of adding one,
  so a focused button does not change size and shove its neighbours along.
- `id` has to be unique among siblings. Two buttons sharing an id will confuse
  gpui's hover and press tracking.
- `on_click` is *not* stored when `disabled(true)` is set, so a handler cannot
  fire on a disabled button by any route.
