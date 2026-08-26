# Segmented

A horizontal one-of-many strip, used where a group of radio buttons would
otherwise go. Best for a handful of short labels that the user benefits from
seeing all at once — an output format, a units choice, a view mode. For a long
list, or one whose entries are long, use [`Select`](./select.md) instead.

Source: [segmented.rs](../../crates/rugpui/src/segmented.rs).

## Minimal example

From the gallery:

```rust
use rugpui::Segmented;

Segmented::new("format")
    .options(vec![("csv", "CSV"), ("json", "JSON"), ("insert", "INSERT")])
    .selected(self.segment)
    .on_select({
        let this = this.clone();
        move |index, _window, cx| {
            this.update(cx, |gallery, cx| {
                gallery.segment = index;
                cx.notify();
            });
        }
    })
```

`on_select` takes its `usize` by value, so `cx.listener` — which produces an
`Fn(&E, ..)` — does not fit; `cx.processor` is the by-value counterpart.

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `Segmented::new` | `id: impl Into<ElementId>` | — | Creates an empty control with segment `0` selected. `id` must be unique among its siblings. |
| `options` | `impl IntoIterator<Item = (V, L)>` where both convert into `SharedString` | empty | The segments in display order, as `(value, label)` pairs. |
| `selected` | `usize` | `0` | Index of the highlighted segment. Out of range simply highlights nothing. |
| `tab_index` | `isize` | none | Puts the whole group at one tab stop. |
| `on_select` | `impl Fn(usize, &mut Window, &mut App) + 'static` | none | Fired with the index the user picked. Never fired for the segment already selected. |

## Why every option carries a value

`options` wants pairs, not bare labels, and the first half of the pair is never
shown. It exists to build stable element ids: each segment's id is
`ElementId::from((control_id, value))`. Ids derived from the position in the list
would move when the option list changes, and gpui's hover state — which is keyed
on the id — would follow the wrong segment. A short machine-readable slug such
as `"csv"` is the right thing to pass; it can be the same string the host stores.

## State the host keeps

The selected index, as a plain `usize` — the gallery keeps `segment: usize`. The
control is rebuilt from that index on every render and hands the new index back
through `on_select`; write it into the host's state and call `cx.notify()`.

Since the callback carries an index rather than a value, the host is responsible
for mapping it back to whatever it means. That is why the pair's value half is
worth keeping meaningful: `options[index].0` is usually what wants storing.

## Keyboard and mouse

- Clicking an unselected segment fires `on_select`. The selected segment is not
  clickable at all — its handler is filtered out — so re-selecting is a no-op by
  construction rather than by a check in the callback.
- With `tab_index`, the group takes a **single** tab stop rather than one per
  segment, so `Tab` steps past the control instead of through it. That is the
  behaviour WAI-ARIA prescribes for a radio group.
- While focused, `Left`/`Up` move the selection one segment towards the start
  and `Right`/`Down` one towards the end, **wrapping** at either end
  (`rem_euclid` over the option count). A keystroke carrying any modifier is
  ignored, and propagation is stopped only when the selection actually moved.
- The tab stop is only installed when there is at least one option, so an empty
  control cannot trap focus.

## Theme slots

- `surface` — the container's background.
- `border` — the container's outline.
- `accent` — the focus outline of the container.
- `surface_active` — the selected segment's fill.
- `surface_hover` — the hover fill of an unselected segment.
- `text` — the selected segment's label.
- `text_muted` — unselected labels.

## Pitfalls

- The control is `w_full`: it stretches to its parent, and the segments share
  that width via `flex_grow_1`. Constrain the parent if you want it narrower.
- An out-of-range `selected` is not an error and not clamped — it just leaves
  every segment looking unselected. Arrow keys still work from it, computing
  from the out-of-range index.
- The arrow keys need `on_select` to be set as well as `tab_index`; without a
  handler there is nowhere to report the move and the keys do nothing.
