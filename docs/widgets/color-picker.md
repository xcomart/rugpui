# Color picker

A colour picker: a saturation/value field over a hue strip, with an optional
row of preset swatches under it. It works in [`Hsv`](#hsv) rather than in
gpui's `Hsla`, because a grey has no hue of its own: a picker that kept only the
colour would forget the hue the moment the knob touched the field's left edge or
bottom row, and the field would jump to red. Hue, saturation and value are what
the pointer moves, so they are what the host keeps.

Source: [color_picker.rs](../../crates/rugpui/src/color_picker.rs).

## Minimal example

From the gallery, at the bottom of the controls column:

```rust
use rugpui::{ColorPicker, Hsv};

ColorPicker::new("accent")
    .value(self.color)
    .field_height(px(96.))
    .presets([0xdc2626, 0x2563eb, 0x16a34a].map(|value| gpui::rgb(value).into()))
    .tab_index(0)
    .on_change({
        let this = this.clone();
        move |color, _window, cx| {
            this.update(cx, |gallery, cx| {
                gallery.color = color;
                cx.notify();
            });
        }
    })
```

![A picker on a blue, with a row of eight presets, the blue one ringed](../screenshots/color-picker/presets.png)

*The field fills the width it is given; the knob sits on the colour, the strip's
knob on its hue, and the preset matching the colour is ringed.*

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `ColorPicker::new` | `id: impl Into<ElementId>` | — | Creates a picker showing white. `id` has to be unique in the window, for the reason a [`Slider`](./slider.md)'s does. |
| `value` | `Hsv` | white | The colour shown. |
| `presets` | `impl IntoIterator<Item = Hsla>` | none | A row of swatches under the strip, each picking its colour on a click. The one matching the current colour (by its `#rrggbb`) is ringed. A grey preset keeps the current hue. |
| `field_height` | `Pixels` | `150px` | Height of the saturation/value field. The strip and the presets keep their own heights. |
| `tab_index` | `isize` | none | Places the picker in the tab order and enables its keys. |
| `on_change` | `impl Fn(Hsv, &mut Window, &mut App) + 'static` | none | Fired with the colour the picker is moving to, by every way of moving it, and never with the colour already showing. |

## `Hsv`

| item | purpose |
| --- | --- |
| `Hsv { h, s, v }` | Hue, saturation and value, each `0.0..=1.0`; hue `0.0` and `1.0` are both red. |
| `Hsv::new(h, s, v)` | Clamps each part into range; a `NaN` part counts as `0.0`. |
| `Hsv::from_hex` / `to_hex` | `#rrggbb` in, the `#` optional and either case; `#rrggbb` in lower case out. |
| `Hsv::from_hsla` / `from_rgba`, `to_hsla` / `to_rgba` | To and from gpui's colours. Alpha is ignored on the way in and `1.0` on the way out. |
| `Hsv::keeping_hue(previous)` | The same colour under `previous`'s hue when it is a grey — what a host does with a colour typed into a hex field beside the picker. |

A hex field is not part of the widget: a host that wants one puts its own
[`TextInput`](./text-input.md) beside the picker, sets it from
`Hsv::to_hex` in `on_change`, and reads it back with
`Hsv::from_hex(..).map(|color| color.keeping_hue(current))`.

## State the host keeps

Just the `Hsv`. There is nothing to initialise and no drag state; the picker is
rebuilt from `value(..)` on every render.

## Keyboard and mouse

| gesture | effect |
| --- | --- |
| press or drag in the field | Saturation across, value down — brightest at the top. The drag keeps answering after the pointer leaves the picker, stopping at the edges. |
| press or drag in the strip | The hue. |
| click a preset | That colour. |
| `←` / `→` (focused) | Saturation down / up by 0.01, or 0.1 with `Shift`. |
| `↑` / `↓` (focused) | Value up / down by 0.01, or 0.1 with `Shift`. |
| `PageUp` / `PageDown` (focused) | Turns the hue by ten degrees, wrapping past red. |

`DraggedColor` is the drag payload. It is public so a host listening for drags
itself can name it; `DraggedColor::color(id, position, current)` answers the
colour a drag has reached, or `None` for another picker's drag.
