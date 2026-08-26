# ProgressBar

A thin horizontal bar showing how far along a piece of work is. Use it when the
extent of the work is known — bytes uploaded out of bytes total, rows exported
out of rows selected. When it is not, either switch the same bar to
indeterminate mode or reach for [`Spinner`](./spinner.md), which says only that
work is under way and takes far less room.

Source: [progress.rs](../../crates/rugpui/src/progress.rs).

## Minimal example

The gallery draws both modes, the determinate one sharing its value with the
[`Slider`](./slider.md) above it:

```rust
use rugpui::ProgressBar;

div()
    .child(ProgressBar::new("amount-progress").fraction(self.amount))
    .child(ProgressBar::new("loading").indeterminate())
```

Typically the fraction is computed from whatever the host is counting:

```rust
ProgressBar::new("upload").fraction(self.uploaded / self.total)
```

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `ProgressBar::new` | `id: impl Into<ElementId>` | — | Creates an empty, determinate bar. `id` must be unique among its siblings; it also seeds the element id of the indeterminate sweep animation. |
| `fraction` | `f32` | `0.0` | How much of the track is filled, clamped to `0.0..=1.0` on the way in. Ignored once `indeterminate` has been set. |
| `indeterminate` | — (no argument) | off | Switches to a segment sweeping the track on a loop instead of a fill amount. |

Note that `indeterminate()` takes no argument — it is a switch, not a setter.
There is no way back to determinate on the same builder, so a host that toggles
between the two picks the mode as it builds:

```rust
let bar = ProgressBar::new("export");
let bar = match self.total {
    Some(total) => bar.fraction(self.done as f32 / total as f32),
    None => bar.indeterminate(),
};
```

## State the host keeps

Only the number. The bar is stateless like the rest of the kit: the host passes
the current fill on every render, and there is no handle, no phase and no
callback. The gallery reuses its `amount: f32` field for both the slider and the
bar.

The indeterminate sweep is the one thing the host does *not* have to drive. The
animation runs off the element id, so rendering the bar is enough to make it
move and dropping it from the tree is enough to stop it — the owning view is
never asked to re-render for the animation's sake.

## The two modes

**Determinate.** A single filled `div` sized `relative(fraction)` inside the
track. `fraction` is clamped as it is stored, so `-1.0` draws empty and `2.0`
draws full; a `NaN` is *not* handled here, unlike the slider's `value`, so guard
a division that can produce one.

**Indeterminate.** A segment covering 30% of the track's width sweeps left to
right over 1200 ms, repeating, with an ease-in-out curve. The segment starts
entirely to the left of the track and ends entirely to its right, so the sweep is
never seen appearing or disappearing mid-track; `overflow_hidden` on the track
clips it at both ends. The animation's element id is `(bar_id, "sweep")`, which
is why the bar's own id has to be unique.

## Theme slots

- `surface` — the track's background.
- `border` — the track's outline.
- `accent` — the fill, in both modes.

## Pitfalls

- The bar is `w_full` and 6 px tall. It stretches to its parent, so constrain the
  parent rather than the bar.
- `fraction` after `indeterminate` is stored but never drawn. Calling both is not
  an error, merely dead code.
- `self.done / self.total` with `total == 0` yields `NaN`, which the clamp leaves
  as `NaN` — `f32::clamp` propagates it — and gpui will draw nothing sensible.
  Check for the zero case and use `indeterminate()` there.
