//! A horizontal progress bar.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, ElementId, Window, div, ease_in_out, prelude::*, px, relative,
};

use crate::theme::theme;

/// Fraction of the track's width the sweeping segment covers while
/// [`ProgressBar`] is indeterminate.
const SWEEP_FRACTION: f32 = 0.3;

/// How long one pass of the indeterminate sweep takes.
const SWEEP_DURATION: Duration = Duration::from_millis(1200);

/// Clamps a fill fraction to the `0.0..=1.0` range a progress bar can draw.
fn clamp_fraction(fraction: f32) -> f32 {
    fraction.clamp(0., 1.)
}

/// Maps an animation delta in `0.0..=1.0` to the left edge of the sweeping
/// segment, as a fraction of the track's width.
///
/// The segment starts entirely to the left of the track and ends entirely to
/// its right, so the sweep is never seen appearing or disappearing mid-track.
fn sweep_left(delta: f32) -> f32 {
    -SWEEP_FRACTION + delta * (1. + SWEEP_FRACTION)
}

/// A stateless horizontal progress bar.
///
/// Like the rest of this crate's widgets it owns no state: the parent view
/// passes the current fill on every render. A bar started with
/// [`ProgressBar::indeterminate`] ignores [`ProgressBar::fraction`] and
/// instead animates a segment sweeping across the track, for work whose
/// extent isn't known yet.
///
/// ```ignore
/// ProgressBar::new("upload").fraction(self.uploaded / self.total)
/// ```
#[derive(IntoElement)]
pub struct ProgressBar {
    id: ElementId,
    fraction: f32,
    indeterminate: bool,
}

impl ProgressBar {
    /// Creates an empty, determinate progress bar.
    ///
    /// `id` must be unique among the siblings of the bar; it also seeds the
    /// element id of the indeterminate sweep animation.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            fraction: 0.,
            indeterminate: false,
        }
    }

    /// Sets how much of the track is filled, clamped to `0.0..=1.0`.
    ///
    /// Ignored once [`ProgressBar::indeterminate`] has been set.
    pub fn fraction(mut self, fraction: f32) -> Self {
        self.fraction = clamp_fraction(fraction);
        self
    }

    /// Switches the bar to indeterminate mode: a segment sweeps the track
    /// left to right on a loop instead of showing a fill amount.
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);

        let fill = if self.indeterminate {
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(SWEEP_FRACTION))
                .rounded_full()
                .bg(theme.accent)
                .with_animation(
                    ElementId::from((self.id.clone(), "sweep")),
                    Animation::new(SWEEP_DURATION)
                        .repeat()
                        .with_easing(ease_in_out),
                    |this, delta| this.left(relative(sweep_left(delta))),
                )
                .into_any_element()
        } else {
            div()
                .h_full()
                .w(relative(self.fraction))
                .rounded_full()
                .bg(theme.accent)
                .into_any_element()
        };

        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(px(6.))
            .rounded_full()
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(fill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_fraction_keeps_values_in_range() {
        assert_eq!(clamp_fraction(0.5), 0.5);
        assert_eq!(clamp_fraction(-1.), 0.);
        assert_eq!(clamp_fraction(2.), 1.);
    }

    #[test]
    fn sweep_left_starts_and_ends_fully_outside_the_track() {
        assert_eq!(sweep_left(0.), -SWEEP_FRACTION);
        assert!((sweep_left(1.) - 1.).abs() < 1e-5);
    }
}
