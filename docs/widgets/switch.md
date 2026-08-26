# Switch

A labelled on/off switch — a track with a knob that slides from one end to the
other. Its API mirrors [`Checkbox`](./checkbox.md) method for method; pick
between them on feel alone. A switch reads as "this is turned on right now", a
checkbox as "this option is selected", which is why a switch suits a setting
that takes effect immediately and a checkbox suits a form the user submits.

Source: [switch.rs](../../crates/rugpui/src/switch.rs).

## Minimal example

From the gallery:

```rust
use rugpui::Switch;

Switch::new("wifi", "Auto-reconnect")
    .checked(self.switch_on)
    .on_toggle({
        let this = this.clone();
        move |value, _window, cx| {
            this.update(cx, |gallery, cx| {
                gallery.switch_on = value;
                cx.notify();
            });
        }
    })
```

A switch with no handler renders fine and simply does nothing when clicked; the
gallery shows `Switch::new("telemetry", "Send telemetry")` that way.

![A switch off beside a switch on](../screenshots/switch/states.png)

*The two positions: `checked(false)`, the default, and `checked(true)`. The
knob is moved by flex alignment, not by an offset.*

Because `on_toggle` takes its `bool` by value, `cx.listener` — which produces an
`Fn(&E, ..)` — does not fit. `cx.processor` is the by-value counterpart:

```rust
Switch::new("notifications", "Enable notifications")
    .checked(self.notifications)
    .on_toggle(cx.processor(|this, checked, _window, cx| {
        this.notifications = checked;
        cx.notify();
    }))
```

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `Switch::new` | `id: impl Into<ElementId>`, `label: impl Into<SharedString>` | — | Creates a switch in the off position. `id` must be unique among its siblings. |
| `checked` | `bool` | `false` | Whether the switch is on. |
| `tab_index` | `isize` | none | Places the switch in the window's tab order. |
| `on_toggle` | `impl Fn(bool, &mut Window, &mut App) + 'static` | none | Fired with the value the switch is toggling *to*. |

There is no `disabled`, no size option and no variant: the switch draws one way
only — a 30x16 px track with a 12 px knob — and a control the user must not
touch is left out of the tree or drawn by the host.

## State the host keeps

The same bargain as every other widget in the kit: the switch owns nothing. The
host keeps the `bool`, hands it in through `checked(..)` on every render, and
writes the argument of `on_toggle` back into its own state followed by
`cx.notify()`. The gallery's `switch_on: bool` field is the whole of it.

The callback receives the *new* value, so there is no negation to write and no
chance of the widget and the host disagreeing about which way the flip went.

## Keyboard and mouse

- Clicking anywhere on the row — track or label — flips it; the whole row is one
  clickable element.
- With `tab_index`, a focused switch draws an accent outline and toggles on
  `Space` or `Enter`, which gpui delivers as an ordinary click.
- The knob is positioned by flex alignment rather than an offset: the track uses
  `justify_end()` while on and `justify_start()` while off.

## Theme slots

- `accent` — the track fill and border while on, and the focus outline.
- `background` — the knob while on, so it reads as a hole punched in the accent.
- `surface` — the track fill while off.
- `border` — the track outline while off.
- `text_muted` — the knob while off, one step quieter than the label beside it.
- `text` — the label.

## Pitfalls

- The focus ring is a transparent 1 px border that is merely recoloured on
  focus, so gaining focus costs no layout and the row does not shift.
- `on_toggle` is only wired when a handler is supplied; without one the row is
  still styled `cursor_pointer`, which can read as a live control that ignores
  clicks. Leave the handler off only for a genuinely static example.
