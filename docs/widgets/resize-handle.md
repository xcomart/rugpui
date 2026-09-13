# ResizeHandle

A band the pointer can grab to resize something, and the bar it lights up. One
absolutely positioned element drawn *over* whatever is being resized rather than
wedged beside it: an invisible band that takes the press and the resize cursor,
and a rounded accent bar inside it that fades in while the pointer is on the
band or holding it.

It knows nothing about what the gesture means. It starts a gpui drag carrying a
payload the host invents; where the pointer goes afterwards is the host's to
read. [`Splitter`](./splitter.md) uses it for the divider between two panes; a
panel whose width is a setting uses it on its own trailing edge; neither had to
write the fade.

Source: [resize_handle.rs](../../crates/rugpui/src/resize_handle.rs).

> No pictures on this page. The shots in these docs are taken by
> `scripts/docshots.sh` from the gallery, and there is no way to hold a pointer
> still while a screenshot is taken — which is the *only* state in which this
> widget paints anything at all. So it is described in words, as
> [README](../README.md) says a hover-only widget has to be.

## Minimal example

A sidebar whose width is a setting of the host's, resized by dragging its own
right edge:

```rust
use gpui::{Axis, DragMoveEvent, div, prelude::*, px, relative};
use rugpui::{ResizeHandle, split_share};

/// The host's own payload type. Nothing in the widget reads it.
#[derive(Clone, Debug)]
struct DraggedPanelEdge;

// in render:
div()
    .relative()                       // the handle positions itself against this
    .w(px(self.files_width))
    .h_full()
    .border_r_1()
    .border_color(palette.border)
    .child(file_list)
    .child(ResizeHandle::new("files-width", Axis::Horizontal, DraggedPanelEdge).at_end())

// and on the box the drag should be measured against — usually an ancestor
// that spans the whole area the panel may grow into:
.on_drag_move(cx.listener(
    |this, event: &DragMoveEvent<DraggedPanelEdge>, _window, cx| {
        let Some(share) =
            split_share(Axis::Horizontal, event.bounds, event.event.position, 0.1)
        else {
            return;
        };
        this.files_width = share * f32::from(event.bounds.size.width);
        cx.notify();
    },
))
```

`Splitter` does the same thing with the other placement — its band straddles the
seam rather than sitting inside an edge:

```rust
ResizeHandle::new(
    ElementId::from((id.clone(), "split-handle")),
    axis,
    DraggedSplit { id: id.clone() },
)
.at(relative(ratio))
.thickness(self.thickness)
.bar_thickness(self.bar)
```

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `ResizeHandle::new` | `id: impl Into<ElementId>`, `axis: gpui::Axis`, `payload: T` | — | Creates a handle that drags along `axis` carrying `payload`. `Horizontal` is a tall thin band that moves left and right, with the east-west resize cursor; `Vertical` is the transpose. See below for why `id` is unusual. |
| `at` | `impl Into<Length>` | — | Centres the band on an offset from the container's leading edge — `relative(ratio)`, or a `px`. The band is pulled back half its own thickness, and the bar is centred in it. |
| `at_end` | — | **the default** | Puts the whole band inside the container's trailing edge — `right_0()` on `Horizontal`, `bottom_0()` on `Vertical` — with the bar flush against that same edge. |
| `at_start` | — | | The mirror of `at_end`: `left_0()` / `top_0()`, bar flush with the leading edge. |
| `thickness` | `Pixels` | `6 px` | How thick the band that answers a press is. The grab area alone: widening it never makes the bar heavier. |
| `bar_thickness` | `Pixels` | `3 px` | How thick the accent bar is. Clamped to the band's own thickness, so the two can be set in either order; `0 px` gives a handle that answers a press but never marks itself. |

`T` is only required to be `'static` — no `Clone`, no `Debug`, nothing. That is
what gpui's `on_drag` asks for and nothing here asks for more.

## State the host keeps

None. Not one field.

The fade is *not* the host's: it is a fact about one pointer and one band, and
no view has any use for it, so the handle keeps it under gpui's element state —
the same store an `on_click` uses to remember it saw a press — keyed by the
handle's own id. It comes into being the first time the band is drawn and is
gone the moment it stops being drawn, which is exactly as long as a fade should
live. Nothing to declare, nothing to initialise, nothing to reset.

What the host does keep is whatever the drag is *for* — a width, a ratio — and
that would have existed anyway.

## `id` has to be unique in the *window*

Two handles sharing an id would share a fade and light up together, because the
id is the key the fade is filed under. Every other widget in the kit asks only
for an id unique among its siblings; this one asks for more, for the same
reason [`Splitter`](./splitter.md#id-has-to-be-unique-in-the-window) does.

Note that the *payload type* has the same problem one level up: gpui delivers a
`DragMoveEvent` to **every** element listening for that type, ancestor or not.
If handles of one payload type can nest — as splitters do — put something in
the payload that says which handle the gesture started on, and have each
listener check it. A payload that can never nest can be a unit struct.

## The band and the bar

Two different numbers wanted at once. The band that answers a press has to be
wide enough for a pointer to find — 6 px, the same bargain a scrollbar's grab
area makes with its thumb. The mark the eye follows has to be thin enough not to
read as a gutter. So they are two elements: an invisible, `occlude()`ing band
that takes the press and the cursor, and a rounded 3 px bar inside it that takes
the accent. Thickness is the only thing they differ in — the bar runs the whole
length of the band, so what the pointer can grab and what the eye is told to
grab end at the same place.

The bar is not drawn at all until the pointer first arrives, and after that it
fades rather than snapping:

| | duration | easing |
| --- | --- | --- |
| in | `FADE_IN`, 120 ms | `ease_in_out` |
| out | `FADE_OUT`, 250 ms | `ease_in_out` |

Those are [`rugpui::scrollbar`](./scrollbar.md)'s own two constants, used here on
purpose. A scrollbar and a resize handle are the same kind of thing — an overlay
that appears under the pointer and leaves when it goes — and two overlays
breathing at different rates make a window look assembled from parts. Out is
twice in for the reason it is there: nothing is waiting on a bar that is
leaving, so it can afford to go gently, while one arriving is a reaction to
something the user has just done and anything slower reads as lag.

Each phase animates under an element id of its own (`…/bar-fade-in`,
`…/bar-fade-out`). gpui keeps an animation's start time in element state keyed by
that id and drops it once the id stops being drawn, so switching phase restarts
the new one from zero, while staying on one phase leaves the clock running.

## Where the bar sits across the band

This is the one thing the three placements really differ in, and it is not
arbitrary.

- **`at(offset)`** puts the band *astride a line* — the seam between two panes,
  which belongs to neither of them. The band is pulled back half its thickness
  so the grab area is symmetric about that line, and the bar is centred in the
  band by whatever room is left over, which puts it back exactly on the line it
  was centred about.
- **`at_end()` / `at_start()`** put the whole band *inside* the thing being
  resized, flush with one of its edges, so none of it hangs over the neighbour
  and nothing else has to leave room for it. The line the eye already sees there
  is that edge — a panel's hairline border, usually. A bar centred in such a
  band would float two or three pixels in from the border and read as a second
  rule beside it, so it is pushed flush with the same edge the band is and lands
  *on top of* the border instead.

## The release is heard twice, and neither one is the host's

A drag that ends anywhere but on the band is the common case, not the odd one:
once whatever is being resized hits its limit the band stops and the pointer
runs on, often clean out of the window. The bar has to come down then.

gpui asks separately about a release on an element and a release away from it,
and between them the two cover every release there is:

- **on the band** — `on_mouse_up`, which gpui only runs when the band is under
  the pointer, so the bar demonstrably still has a pointer on it and stays up.
  No fade out and straight back in, which is what a blink is, and this is the
  common short-drag ending.
- **anywhere else, inside the window or outside it** — `on_mouse_up_out`, which
  fades the bar out. A no-op unless a press of *this* band is outstanding, so an
  ordinary click elsewhere in the window leaves the bar alone.

Both listeners are on the band itself, so a host adds nothing to its container
but the `on_drag_move` that does the actual resizing.

That matters for a second reason. The bar stays fully up for the whole of a
drag, however far the pointer runs ahead — and gpui reports *every* element as
unhovered while a drag is in flight, so hover alone would take the bar down the
instant the gesture began. The handle therefore remembers the press, and a press
outranks the pointer: while the band is held the phase stays `In` whatever hover
says, and because the phase is unchanged the animation keeps its id and its clock
across every re-render the moving layout causes. The bar does not blink as the
panel travels.

Every one of those handlers asks for a repaint even when the phase is unchanged.
The repaint is the point: gpui re-checks each hover listener against the pointer
as it paints, and the frame drawn after a release is what tells the band it is
being hovered again — a fact it had no way to learn during the drag, and without
which the bar could not hear the pointer eventually leave.

## Geometry

On `Axis::Horizontal` (the transpose on `Vertical`):

- the **band** is `absolute`, `occlude`, `top_0`, `bottom_0`, `w(thickness)`,
  `cursor_ew_resize`, and paints nothing. `at(offset)` adds
  `left(offset).ml(-thickness/2)`; `at_end()` adds `right_0()`; `at_start()`
  adds `left_0()`;
- the **bar** is a child of the band: `absolute`, `rounded_full`, `top_0`,
  `bottom_0`, `w(bar_thickness)`, and `left(gutter)` under `at`, `right_0()`
  under `at_end`, `left_0()` under `at_start`. `gutter` is
  `(thickness - bar_thickness) / 2`, which is the one measurement that stays
  right whatever either thickness is set to.

## Theme slots

- `accent` — the rounded bar, while the pointer is on the band or holding it.

Nothing else is painted. The band itself is transparent in every state, and the
edge or seam the handle sits on is drawn by whoever owns it — the host's border,
or a `Splitter`'s own hairline.

## Pitfalls

- **The container must be `relative()`.** The handle is absolutely positioned,
  so it places itself against the nearest positioned ancestor. Dropped into a
  box that is not positioned it will find some ancestor further up and land
  somewhere surprising.
- **The container needs a definite size on the handle's axis**, since the band
  stretches to the container's full cross-axis length and pins to one of its
  edges.
- **Ids collide across a window, not just across siblings** — see above.
- **Payload types collide across ancestors.** Every listener for a payload type
  sees every drag of it, with its own `bounds`. Put an id in the payload as soon
  as two of these can be on screen at once.
- **The handle does not move anything.** It starts the drag and lights up; the
  host reads `on_drag_move` and changes its own state. Measure the pointer
  against a box that stays put — not against the panel being resized, which
  slides out from under it.
- **`at_end()` is the default**, so a handle that was never given a placement
  ends up on the container's trailing edge. Say `at(..)` explicitly for a band
  astride a line.
