# Range slider

A horizontal slider over an *interval* from `0.0` to `1.0`: two knobs on one
track, `low` and `high`, and a filled band between them. Everything the
[`Slider`](./slider.md) is, with a second knob and the one rule that follows
from it — neither knob may pass the other. Like every other widget in the kit it
knows nothing of what the two numbers mean; a price band, a date window, a pair
of gain limits are all the same control.

Source: [range_slider.rs](../../crates/rugpui/src/range_slider.rs).

## Minimal example

From the gallery, under the `Slider` it shares a section with:

```rust
use rugpui::RangeSlider;

RangeSlider::new("band")
    .low(low)
    .high(high)
    .step(0.05)
    .on_change({
        let this = this.clone();
        move |low, high, _window, cx| {
            this.update(cx, |gallery, cx| {
                gallery.range = (low, high);
                cx.notify();
            });
        }
    })
```

`on_change` is handed *both* ends every time rather than the one that moved, so
a host that stores an interval stores an interval and never has to work out
which end it is being told about. It takes its two `f32`s by value, so
`cx.listener` — which produces an `Fn(&E, ..)` — does not fit; `cx.processor` is
the by-value counterpart.

![Three range sliders: the whole range, a band inside it, and both knobs
met](../screenshots/range-slider/values.png)

*The whole of a range slider's state is where its two ends are:
`low(0.0).high(1.0)`, `low(0.25).high(0.75)` and `low(0.5).high(0.5)`.*

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `RangeSlider::new` | `id: impl Into<ElementId>` | — | Creates a range slider selecting the whole range. `id` is unusual; see below. |
| `low` | `f32` | `0.0` | Where the interval starts. Clamped to `0.0..=1.0` for drawing; `NaN` draws as `0.0`; a `low` above its `high` is drawn *at* the high knob. |
| `high` | `f32` | `1.0` | Where the interval ends, clamped the same way. |
| `step` | `f32` | `0.05` | How far one arrow key moves the focused knob, and which grid keyboard values snap to. A step that is not positive and finite disables stepping rather than freezing the keys. |
| `tab_index` | `isize` | none | Places the low knob at `index` and the high knob at `index + 1` in the window's tab order, and enables the arrow / `Home` / `End` keys on whichever holds focus. |
| `on_change` | `impl Fn(f32, f32, &mut Window, &mut App) + 'static` | none | Fired with the interval the slider is moving to, by all three ways of moving a knob, and never with the interval already showing. |

Two more items in the module are part of the public API:

| item | signature | purpose |
| --- | --- | --- |
| `RangeSlider::dragged` | `fn dragged(&self, event: &DragMoveEvent<DraggedKnob>, cx: &App) -> Option<(Knob, f32)>` | Which knob `event` is dragging and where it has reached, or `None` when the drag belongs to another control. The value is the one `on_change` would be given for that knob, stopped at its neighbour and all. |
| `Knob` | `enum Knob { Low, High }` | Which end of the interval a drag is moving. Only interesting to a host reading the gesture itself — the callback reports both ends and never needs to name one. |

The drag payload is [`Slider`](./slider.md)'s own `DraggedKnob`: a knob carries
its own sub-id — `(id, "low")` or `(id, "high")` — so the one payload type tells
the two knobs of one range apart with the same comparison it uses to tell two
sliders apart.

## `id` has to be unique in the *window*

For exactly the reason [the slider's page gives](./slider.md#id-has-to-be-unique-in-the-window),
and across both widgets: a range slider's id must differ from every other range
slider's *and* every `Slider`'s that can be dragged at the same time, since they
share the payload type gpui hands to every listener of it.

## State the host keeps

A pair — the gallery keeps `range: (f32, f32)`, initialised to `(0.25, 0.75)`.
There is nothing else to initialise and nothing to keep beside it: the control
is rebuilt from `low(..)` and `high(..)` on every render and reports the new
interval through `on_change`; store it and call `cx.notify()`. Mapping a
fraction to whatever the host actually stores works exactly as it does for the
[slider](./slider.md#state-the-host-keeps), applied twice.

## Keyboard and mouse

| gesture | effect |
| --- | --- |
| drag a knob | that knob follows the pointer, keeping the grab offset the press landed at, and stops at the other knob; no snapping to the step |
| press the track | the *nearer* knob's centre jumps to where the pointer landed, stopping at the other knob; no snapping |
| `Left` / `Down` | the focused knob, one step towards the start |
| `Right` / `Up` | the focused knob, one step towards the end |
| `Home` / `End` | the focused knob, the whole way — which for the low knob means `0.0` and *the high knob*, and for the high knob means *the low knob* and `1.0` |

Three things differ from the single-knob slider:

- **Neither knob may pass the other.** A knob dragged into its neighbour stops
  there, and the two are allowed to meet: `low == high` is a legal, if empty,
  interval.
- **A press on the bare track moves whichever knob is nearer**, since there is
  no longer one obvious answer to "the knob comes here". A press exactly halfway
  between them moves the **high** knob — a tie is every press on that one
  column, and both answers are as good as each other, so it only has to be
  settled the same way every time.
- **The two knobs are two tab stops**, so a range slider takes two tab indices
  where every other widget in the kit takes one, and the focus ring is drawn
  around a knob rather than around the whole control.

## Geometry

The [slider's four boxes](./slider.md#geometry) with a second knob hung off the
same rail, and one difference: the filled part runs from the low knob's centre
to the high knob's, so both of its ends are values placed as a percentage of the
rail. It needs no reach back over the rail's leading edge, because it no longer
starts where the track does.

Each knob is wrapped in its own focus ring — a transparent circle 4 px outside
it, which is exactly the padding the control keeps clear, so a ring around a
knob at either end of the track lands on the control's own edge rather than
spilling past it. The ring does not occlude, so a press that lands in its margin
still reaches the track underneath.

When the two knobs sit on top of each other, only the one drawn last takes the
press, so the one with somewhere left to go is drawn last: at the very end of
the track that is the low knob, since the high one cannot move up any further,
and everywhere else it is the high knob.

## Theme slots

The same four the slider uses:

- `surface` — the groove's fill.
- `border` — the groove's outline and each knob's outline.
- `accent` — the filled band, a knob's outline on hover, and the focus ring.
- `background` — a knob's fill, so it reads as a hole in the filled band.

## Pitfalls

- **Ids collide across a window, and across both slider types.**
- **`tab_index` consumes two indices.** Number the rest of the form around that.
- `low(..)` and `high(..)` clamp for drawing only, and a crossed pair is *drawn*
  ordered rather than corrected — the host's state is left alone until something
  moves, at which point the pair reported back is in order. A host that must
  never hold a crossed pair should order it on its own side too.
- Dragging and track presses do **not** snap to `step`. Only the keyboard does.
- An empty interval is reachable and looks like one knob. If the host needs a
  minimum width to the band, enforce it inside `on_change`.
