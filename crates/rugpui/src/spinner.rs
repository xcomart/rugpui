//! An indeterminate busy indicator.
//!
//! [`Spinner`] is what a view shows while it is waiting on something whose
//! progress it cannot measure — a query still running, a file still opening. A
//! wait whose end *is* known belongs in a progress bar instead; the spinner
//! says only that work is under way.
//!
//! The arc is painted rather than drawn from an icon. A widget kit that shipped
//! an svg would need the host to have registered an asset source and a path to
//! find it under, and this crate deliberately knows nothing about the host's
//! assets, so the geometry is computed here and handed to a [`canvas`].

use std::{f32::consts::TAU, time::Duration};

use gpui::{
    Animation, AnimationExt, App, ElementId, Hsla, PathBuilder, Pixels, Point, Window, canvas,
    point, prelude::*, px,
};

use crate::theme::theme;

/// How long the arc takes to come back around to where it started.
const PERIOD: Duration = Duration::from_millis(800);

/// Width of the arc's stroke, in pixels.
///
/// Kept constant rather than scaled with the spinner: at the sizes a spinner is
/// used at, a hairline reads as a smudge and anything heavier as a ring.
const STROKE: f32 = 2.;

/// How much of the circle the arc covers, in turns.
///
/// Three quarters leaves a gap wide enough that the rotation is legible — a
/// closed ring would look identical in every frame.
const SWEEP: f32 = 0.75;

/// How many elliptical arcs the sweep is built from.
///
/// Each piece stays under a half turn, which keeps the large-arc flag of every
/// [`PathBuilder::arc_to`] unambiguously `false` no matter where the sweep
/// starts.
const SEGMENTS: usize = 3;

/// A spinning arc that says work is under way without saying how much is left.
///
/// The widget owns no state: the animation runs off the element id, so the
/// parent view neither stores a phase nor asks for repaints. Rendering one is
/// enough to make it turn, and dropping it from the tree is enough to stop it.
///
/// ```ignore
/// if self.running {
///     Spinner::new("query-busy").size(px(14.)).into_any_element()
/// } else {
///     Button::new("run", "Run").into_any_element()
/// }
/// ```
#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: Pixels,
    color: Option<Hsla>,
}

impl Spinner {
    /// Creates a spinner sixteen pixels across, drawn in the theme's accent.
    ///
    /// `id` must be unique among the siblings of the spinner: it is what the
    /// animation keeps its phase under, so two spinners sharing one id would
    /// fight over a single rotation.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            size: px(16.),
            color: None,
        }
    }

    /// Sets the width and height of the box the arc is drawn inside.
    ///
    /// The stroke keeps its width as the box grows, so a large spinner is a
    /// thin ring and a small one is a thick comma. A box narrower than the
    /// stroke draws nothing at all.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Overrides the arc's color.
    ///
    /// Worth setting when the spinner sits on a colored surface the accent
    /// disappears into — inside a filled button, say, where the label's own
    /// color is the one that reads.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| theme(cx).accent);

        // The animation drives its own frames, so the view holding the spinner
        // is never asked to re-render. `delta` runs 0..1 over each period,
        // which is exactly the phase in turns.
        SpinnerArc {
            size: self.size,
            color,
            phase: 0.,
        }
        .with_animation(
            self.id,
            Animation::new(PERIOD).repeat(),
            |mut arc, delta| {
                arc.phase = delta;
                arc
            },
        )
    }
}

/// One frame of the spinner: the arc standing still at `phase`.
#[derive(IntoElement)]
struct SpinnerArc {
    size: Pixels,
    color: Hsla,
    /// Where the arc begins, in turns clockwise from twelve o'clock.
    phase: f32,
}

impl RenderOnce for SpinnerArc {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let SpinnerArc { size, color, phase } = self;

        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let radius = radius(size);
                if radius <= px(0.) {
                    return;
                }

                let center = bounds.center();
                let radii = point(radius, radius);
                let start = phase * TAU;

                let mut builder = PathBuilder::stroke(px(STROKE));
                builder.move_to(on_circle(center, radius, start));
                for segment in 1..=SEGMENTS {
                    let angle = start + SWEEP * TAU * segment as f32 / SEGMENTS as f32;
                    // Sweeping clockwise, which with y pointing down the screen
                    // is the positive direction, hence `sweep`.
                    builder.arc_to(radii, px(0.), false, true, on_circle(center, radius, angle));
                }

                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size(size)
    }
}

/// The radius the arc is drawn at inside a box `size` across.
///
/// A stroke straddles the path it follows, so half of it falls outside the
/// radius. Pulling the radius in by that half keeps the whole ring within the
/// element's bounds, where a naive `size / 2` would let it bleed over every
/// edge. A box too small to hold even the stroke gets a radius of zero, which
/// the caller takes as nothing to draw.
fn radius(size: Pixels) -> Pixels {
    px((size.as_f32() - STROKE).max(0.) / 2.)
}

/// The point `radius` away from `center` at `angle` radians.
///
/// Angles are measured clockwise from twelve o'clock, so that a quarter turn is
/// three o'clock on screen. Screen coordinates put y at the bottom, which is
/// why the vertical term is subtracted rather than added.
fn on_circle(center: Point<Pixels>, radius: Pixels, angle: f32) -> Point<Pixels> {
    point(
        center.x + radius * angle.sin(),
        center.y - radius * angle.cos(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixels a thousandth apart are the same pixel as far as painting goes.
    fn close(actual: Point<Pixels>, expected: (f32, f32)) -> bool {
        (actual.x.as_f32() - expected.0).abs() < 1e-3
            && (actual.y.as_f32() - expected.1).abs() < 1e-3
    }

    /// The middle of a ten-pixel box.
    fn center() -> Point<Pixels> {
        point(px(5.), px(5.))
    }

    /// Twelve o'clock is straight up, and the quarter turns walk clockwise
    /// around the face from there.
    #[test]
    fn quarter_turns_land_on_the_compass_points() {
        let radius = px(4.);
        let at = |turns: f32| on_circle(center(), radius, turns * TAU);

        assert!(close(at(0.), (5., 1.)), "twelve o'clock is above center");
        assert!(close(at(0.25), (9., 5.)), "a quarter turn is to the right");
        assert!(close(at(0.5), (5., 9.)), "a half turn is below center");
        assert!(close(at(0.75), (1., 5.)), "three quarters is to the left");
    }

    /// A full turn is no turn at all, which is what lets the animation's phase
    /// wrap from one period to the next without the arc jumping.
    #[test]
    fn a_full_turn_comes_back_to_where_it_started() {
        let radius = px(4.);
        let start = on_circle(center(), radius, 0.3 * TAU);
        let wrapped = on_circle(center(), radius, 1.3 * TAU);

        assert!(close(wrapped, (start.x.as_f32(), start.y.as_f32())));
    }

    /// Every point of the arc sits on the circle it was asked for, whichever
    /// way the phase has carried it.
    #[test]
    fn every_angle_stays_on_the_circle() {
        let radius = px(4.);
        for step in 0..32 {
            let at = on_circle(center(), radius, step as f32 / 32. * TAU);
            let distance = ((at.x.as_f32() - 5.).powi(2) + (at.y.as_f32() - 5.).powi(2)).sqrt();
            assert!((distance - 4.).abs() < 1e-3, "step {step} left the circle");
        }
    }

    /// Half the stroke lies outside the radius, so the radius gives that half
    /// back rather than letting the ring overhang the box.
    #[test]
    fn the_radius_leaves_room_for_the_stroke() {
        assert_eq!(radius(px(16.)), px(7.));
        assert_eq!(radius(px(10.)), px(4.));

        // The whole box is stroke, or less than that: nothing to draw.
        assert_eq!(radius(px(STROKE)), px(0.));
        assert_eq!(radius(px(1.)), px(0.));
        assert_eq!(radius(px(0.)), px(0.));
    }
}
