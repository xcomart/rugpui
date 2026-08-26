//! Two panes side by side, with a divider the pointer can move.
//!
//! One box holding two children and a number between `0.` and `1.` to say how
//! much of the box the first of them gets. Nothing here knows what is in either
//! half — a tree beside a grid, an editor above a preview, a sidebar beside
//! everything else are all the same splitter — which is the bargain every
//! widget in this crate makes, and the reason a split layout does not have to
//! be rewritten each time the thing being split changes.
//!
//! Like the rest of the kit the splitter is stateless: the owning view passes
//! the current ratio on every render and updates its own state from
//! [`Splitter::on_change`]. One `f32` per divider is the whole of it — no drag
//! flag, no phase, no handle to keep alive between frames.
//!
//! ## Why the container hears the drag, and not the handle
//!
//! A ratio is a fraction *of a box*, so the only thing worth measuring a
//! pointer against is the box. The handle is exactly the wrong thing to measure
//! against: it slides out from under the pointer as the drag goes on, while the
//! container stays where it is for as long as the frame lives. So the handle
//! only starts the gesture, and the container — which gpui hands `bounds` for
//! on every `DragMoveEvent` — is what turns a pointer position into a share.
//!
//! That also means a drag that has wandered far outside the window still lands
//! somewhere sensible: the share is recomputed from scratch each move rather
//! than integrated from deltas, so there is no accumulated drift to undo when
//! the pointer comes back.
//!
//! ## Why the payload carries an id
//!
//! gpui delivers a drag move to *every* element listening for that payload
//! type, ancestor or not. Splitters nest — a column split in two, one half split
//! again — so the handle of an inner split makes every enclosing split's
//! listener fire as well, each with its own, larger `bounds`. Without the id in
//! [`DraggedSplit`] the outer divider would jump every time the inner one was
//! touched. Each splitter answers only for drags carrying its own id, which is
//! also why that id has to be unique within the window rather than merely among
//! its siblings.
//!
//! ## The two guards
//!
//! A division by a width says `NaN` rather than complaining, and a `NaN` handed
//! back to a host is stored, drawn from, and poisons every length computed from
//! it for the rest of the session — including the ratio that would let the
//! divider be dragged back. [`split_share`] therefore answers `None` for a box
//! with no size instead of answering a number.
//!
//! The other guard is the minimum share. A pane squeezed to nothing takes the
//! handle with it and leaves no way to drag it back out, so both halves keep
//! [`Splitter::min_ratio`] of the box whatever the pointer asks for.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Axis, Bounds, DragMoveEvent, ElementId, Pixels, Point, Window, div,
    prelude::*, px, relative,
};

use crate::theme::theme;

/// Share of the box the first child gets when the host has not said otherwise.
const DEFAULT_RATIO: f32 = 0.5;

/// Smallest share either half keeps, when the host has not said otherwise.
///
/// A tenth of the box is small enough to read as "collapsed" and large enough
/// to still hold the handle, which is the only thing that matters: a half
/// dragged to zero would take the divider off the edge of the container and
/// there would be nothing left to grab.
const DEFAULT_MIN_RATIO: f32 = 0.1;

/// Thickness of the band that answers a press, in pixels.
///
/// Wider than the line the eye sees, for the same reason a scrollbar's grab
/// area is wider than its thumb: a one pixel target is not one a pointer can be
/// expected to find.
const DEFAULT_HANDLE: f32 = 6.;

/// Thickness of the line the eye sees, in pixels.
///
/// A hairline and nothing more. It is a seam between two panes, not a border
/// around either of them, and anything thicker starts to read as a gutter.
const SEAM: f32 = 1.;

/// Callback fired with the ratio the divider is moving to.
type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// The smallest share a splitter may be asked to keep.
///
/// Half the box is the most a *minimum* can mean — beyond it the range
/// `min..=1-min` turns inside out, and `f32::clamp` panics on a reversed range
/// rather than picking an end. A host that asks for six tenths gets a divider
/// pinned to the middle instead of a crash.
fn floor(min: f32) -> f32 {
    if !min.is_finite() {
        return 0.;
    }
    min.clamp(0., 0.5)
}

/// `share` pinned to the range a splitter with this minimum can show.
///
/// A share that is not a number at all falls back to the middle rather than to
/// an edge: the middle is the only answer that says nothing about which half
/// the host meant to favour.
fn within(share: f32, min: f32) -> f32 {
    let min = floor(min);
    if share.is_nan() {
        return DEFAULT_RATIO;
    }
    share.clamp(min, 1. - min)
}

/// The share of `bounds` a pointer at `position` is asking the first half to
/// take, already clamped to `min..=1-min`.
///
/// The pure half of the widget, and public because a host laying out its own
/// split — a sidebar whose width is a setting, say, rather than a [`Splitter`]
/// — needs exactly this arithmetic and should not have to rediscover the two
/// guards in the module docs. `None` when the box has no size, which is the one
/// case that would otherwise produce a `NaN` the host stores for good.
pub fn split_share(
    axis: Axis,
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    min: f32,
) -> Option<f32> {
    let share = match axis {
        Axis::Horizontal => (position.x - bounds.left()) / bounds.size.width,
        Axis::Vertical => (position.y - bounds.top()) / bounds.size.height,
    };
    if !share.is_finite() {
        return None;
    }

    Some(within(share, min))
}

/// The divider a drag is currently holding.
///
/// Carries only the id of the splitter the gesture started on, because that is
/// the one thing a listener cannot work out for itself: the box to measure
/// against arrives on the event, and the pointer arrives with it. See the module
/// docs for why nesting makes the id necessary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraggedSplit {
    /// The splitter whose ratio this drag is writing.
    id: ElementId,
}

impl DraggedSplit {
    /// The splitter this drag belongs to.
    ///
    /// For a host that listens for the payload itself — to drive something the
    /// callback does not cover, or to keep every drag it sees on one handler —
    /// and has to tell one divider's gesture from another's before doing so.
    pub fn id(&self) -> &ElementId {
        &self.id
    }
}

/// A stateless split of one box into two, along one axis.
///
/// The parent view passes the current ratio on every render and updates its own
/// state from [`Splitter::on_change`], which receives the ratio the divider is
/// moving to — already clamped, already a number. Ratios outside the range are
/// pinned rather than refused, so a host is never made to sanitise a fraction
/// before it can draw.
///
/// `id` has to be unique among every splitter that can be dragged at the same
/// time — in practice, within the window — because it is what tells one
/// divider's drag from another's.
///
/// ```ignore
/// let this = cx.entity();
/// Splitter::new("data-split", Axis::Horizontal)
///     .ratio(self.split_x)
///     .min_ratio(0.15)
///     .first(self.tree.clone())
///     .second(self.results.clone())
///     .on_change(move |ratio, _window, cx| {
///         this.update(cx, |view, cx| {
///             view.split_x = ratio;
///             cx.notify();
///         });
///     })
/// ```
#[derive(IntoElement)]
pub struct Splitter {
    id: ElementId,
    axis: Axis,
    ratio: f32,
    min_ratio: f32,
    thickness: Pixels,
    seam: bool,
    first: Option<AnyElement>,
    second: Option<AnyElement>,
    on_change: Option<ChangeHandler>,
}

impl Splitter {
    /// Creates an even split along `axis`, with nothing in either half.
    ///
    /// [`Axis::Horizontal`] puts the two halves side by side and moves the
    /// divider left and right; [`Axis::Vertical`] stacks them and moves it up
    /// and down.
    ///
    /// `id` must be unique among the splitters of the window; see the type docs
    /// for why the usual "unique among its siblings" is not enough here.
    pub fn new(id: impl Into<ElementId>, axis: Axis) -> Self {
        Self {
            id: id.into(),
            axis,
            ratio: DEFAULT_RATIO,
            min_ratio: DEFAULT_MIN_RATIO,
            thickness: px(DEFAULT_HANDLE),
            seam: true,
            first: None,
            second: None,
            on_change: None,
        }
    }

    /// Sets the first child's share of the box, as a fraction from `0.` to `1.`.
    ///
    /// Clamped to `min_ratio..=1-min_ratio` for drawing; the host's own value is
    /// left alone. A fraction that is not a number at all draws as an even
    /// split rather than collapsing a half.
    pub fn ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio;
        self
    }

    /// Sets the smallest share either half may be squeezed to.
    ///
    /// Defaults to `0.1`. Values above `0.5` would invert the range and are
    /// read as `0.5`, which pins the divider to the middle; a value that is not
    /// a number at all disables the minimum rather than freezing the divider.
    pub fn min_ratio(mut self, min: f32) -> Self {
        self.min_ratio = min;
        self
    }

    /// Puts `child` in the leading half — left of the divider, or above it.
    pub fn first(mut self, child: impl IntoElement) -> Self {
        self.first = Some(child.into_any_element());
        self
    }

    /// Puts `child` in the trailing half — right of the divider, or below it.
    pub fn second(mut self, child: impl IntoElement) -> Self {
        self.second = Some(child.into_any_element());
        self
    }

    /// Sets how thick the band that answers a press is, in pixels.
    ///
    /// Defaults to 6 px. This is the grab area alone: the line drawn on the
    /// seam stays a hairline whatever the band is widened to, so a splitter can
    /// be made easier to hit without being made to look heavier.
    pub fn handle_thickness(mut self, thickness: Pixels) -> Self {
        self.thickness = thickness;
        self
    }

    /// Drops the line drawn on the seam, leaving the grab band invisible until
    /// the pointer finds it.
    ///
    /// For a split whose two halves already have an edge between them — framed
    /// panels with a gap, a pane that draws its own border — where a second
    /// line would only be a double rule.
    pub fn seamless(mut self) -> Self {
        self.seam = false;
        self
    }

    /// Sets the callback invoked with the ratio the divider is moving to.
    ///
    /// The value handed over is clamped and finite, so a host can store it
    /// unexamined; it is never the ratio already showing.
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Splitter {
    /// Two boxes in a flex line, and two more floating over the seam between
    /// them.
    ///
    /// The halves are sized by `flex_basis` alone — a percentage each, adding up
    /// to the whole — which is what lets the split be drawn without knowing how
    /// wide or tall the container ended up. Both are `min_w_0`/`min_h_0`, since
    /// a flex child refuses to shrink below its content's minimum otherwise and
    /// a grid or an editor inside one would quietly push the divider off the
    /// ratio it was told to sit at.
    ///
    /// The seam and the handle are taken out of the flow and placed at the same
    /// percentage, so both follow the ratio rather than being wedged between the
    /// halves: a divider that took part in the layout would have to be paid for
    /// out of one half's share, and the arithmetic that decides which one is
    /// exactly the arithmetic this widget exists to avoid. The handle is pulled
    /// back half its own thickness so the grab area is symmetric about the line
    /// the eye sees, and occludes: a plain hitbox would let the press reach the
    /// pane underneath as well.
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = theme(cx);
        let axis = self.axis;
        let id = self.id;
        let min = self.min_ratio;
        let ratio = within(self.ratio, min);
        let thickness = self.thickness;
        let on_change = self.on_change;

        let half = |share: f32, child: Option<AnyElement>| {
            div()
                .flex()
                .flex_basis(relative(share))
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(child.unwrap_or_else(|| div().into_any_element()))
        };

        // A hairline on the boundary itself, which is the first pixel of the
        // trailing half rather than a pixel taken from either — the same place a
        // border between the two would have landed.
        let seam = self.seam.then(|| {
            div().absolute().bg(palette.border).map(|line| match axis {
                Axis::Horizontal => line.top_0().bottom_0().left(relative(ratio)).w(px(SEAM)),
                Axis::Vertical => line.left_0().right_0().top(relative(ratio)).h(px(SEAM)),
            })
        });

        let offset = px(-f32::from(thickness) / 2.);
        let handle = div()
            .id(ElementId::from((id.clone(), "split-handle")))
            .absolute()
            .occlude()
            .map(|band| match axis {
                Axis::Horizontal => band
                    .top_0()
                    .bottom_0()
                    .left(relative(ratio))
                    .ml(offset)
                    .w(thickness)
                    .cursor_ew_resize(),
                Axis::Vertical => band
                    .left_0()
                    .right_0()
                    .top(relative(ratio))
                    .mt(offset)
                    .h(thickness)
                    .cursor_ns_resize(),
            })
            // Invisible until the pointer finds it, and accent while it is held:
            // gpui keeps the hover style on the element a drag started from, so
            // one rule covers both "you can grab this" and "you are holding it".
            .hover(|style| style.bg(palette.accent))
            // An empty preview: the divider follows the pointer directly, so a
            // ghost trailing it would only be a second thing to watch.
            .on_drag(DraggedSplit { id: id.clone() }, |_, _, _, cx| {
                cx.new(|_| gpui::Empty)
            });

        div()
            .relative()
            .flex()
            .map(|container| match axis {
                Axis::Horizontal => container.flex_row(),
                Axis::Vertical => container.flex_col(),
            })
            .size_full()
            .min_w_0()
            .min_h_0()
            // Listening here rather than on the handle because the handle moves
            // out from under the pointer as the drag goes on, while this box
            // stays put and is what the new ratio is measured against.
            .on_drag_move(move |event: &DragMoveEvent<DraggedSplit>, window, cx| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                // Enclosing splits see the same moves, so a listener has to
                // check that the divider being dragged is the one it drew.
                if event.drag(cx).id != id {
                    return;
                }
                if let Some(next) = split_share(axis, event.bounds, event.event.position, min)
                    && next != ratio
                {
                    handler(next, window, cx);
                }
            })
            .child(half(ratio, self.first))
            .child(half(1. - ratio, self.second))
            .children(seam)
            .child(handle)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ops::Deref;

    use gpui::{
        Context, Modifiers, MouseButton, Render, TestAppContext, VisualTestContext, point, size,
    };

    use super::*;

    /// Width of the window the drag test runs in.
    ///
    /// A round number, so that a column is a ratio and a ratio is a column with
    /// nothing to work out on the way.
    const HARNESS_WIDTH: f32 = 400.;

    /// Height of the same window.
    const HARNESS_HEIGHT: f32 = 200.;

    /// A box 400 across and 200 down, 40 and 20 in from the window's corner —
    /// offset on both axes so that reading the pointer against the window
    /// rather than against the box would show up as a wrong answer.
    fn frame() -> Bounds<Pixels> {
        Bounds::new(point(px(40.), px(20.)), size(px(400.), px(200.)))
    }

    /// Floats that came through a division are compared with a tolerance, since
    /// none of these values are exactly representable.
    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-5
    }

    /// A splitter with two children in a window, which is the only way to find
    /// out whether the two halves and the divider floating over them can be laid
    /// out at all — the arithmetic below says nothing about that.
    struct Harness {
        ratio: Rc<Cell<f32>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let ratio = self.ratio.clone();
            div().size_full().child(
                Splitter::new("split", Axis::Horizontal)
                    .ratio(ratio.get())
                    .first(div().size_full().child("left"))
                    .second(div().size_full().child("right"))
                    .on_change(move |value, _window, cx| {
                        this.update(cx, |harness: &mut Harness, cx| {
                            harness.ratio.set(value);
                            cx.notify();
                        });
                    }),
            )
        }
    }

    /// The pointer is read against the box's own corner, and the middle of the
    /// box is an even split on either axis.
    #[test]
    fn the_middle_of_the_box_is_an_even_split() {
        let bounds = frame();

        assert_eq!(
            split_share(
                Axis::Horizontal,
                bounds,
                point(px(40. + 200.), px(0.)),
                DEFAULT_MIN_RATIO
            ),
            Some(0.5)
        );
        assert_eq!(
            split_share(
                Axis::Vertical,
                bounds,
                point(px(0.), px(20. + 100.)),
                DEFAULT_MIN_RATIO
            ),
            Some(0.5)
        );

        // And each axis ignores the other's coordinate entirely, so a pointer
        // that has wandered sideways off a vertical split still answers.
        let quarter = split_share(
            Axis::Horizontal,
            bounds,
            point(px(40. + 100.), px(9_000.)),
            DEFAULT_MIN_RATIO,
        )
        .expect("a box with room");
        assert!(close(quarter, 0.25), "{quarter}");
    }

    /// Dragged clean out of the box — including outside the window, where a
    /// gesture ends up more often than not — the divider stops at the minimum
    /// rather than running on or collapsing a half.
    #[test]
    fn a_pointer_outside_the_box_stops_at_the_minimum() {
        let bounds = frame();

        assert_eq!(
            split_share(
                Axis::Horizontal,
                bounds,
                point(px(-9_000.), px(0.)),
                DEFAULT_MIN_RATIO
            ),
            Some(DEFAULT_MIN_RATIO)
        );
        assert_eq!(
            split_share(
                Axis::Vertical,
                bounds,
                point(px(0.), px(9_000.)),
                DEFAULT_MIN_RATIO
            ),
            Some(1. - DEFAULT_MIN_RATIO)
        );
    }

    /// The minimum is the host's to choose, and a nonsensical one is read as
    /// something drawable rather than taken at its word: `f32::clamp` panics on
    /// a range that has turned inside out.
    #[test]
    fn the_minimum_is_respected_and_never_inverted() {
        let bounds = frame();
        let far_left = point(px(-9_000.), px(0.));

        assert_eq!(
            split_share(Axis::Horizontal, bounds, far_left, 0.25),
            Some(0.25)
        );
        assert_eq!(
            split_share(Axis::Horizontal, bounds, far_left, 0.),
            Some(0.)
        );

        // Past a half there is no range left, so the divider pins to the middle.
        assert_eq!(
            split_share(Axis::Horizontal, bounds, far_left, 0.9),
            Some(0.5)
        );
        assert_eq!(
            split_share(Axis::Horizontal, bounds, far_left, f32::NAN),
            Some(0.)
        );
    }

    /// A box with no size has no share to report, and says so rather than
    /// dividing by zero — which is what the first frame of a splitter looks
    /// like, before anything has been laid out.
    #[test]
    fn a_box_with_no_size_reports_nothing() {
        let flat = Bounds::new(point(px(40.), px(20.)), size(px(0.), px(0.)));

        assert_eq!(
            split_share(Axis::Horizontal, flat, point(px(40.), px(20.)), 0.1),
            None
        );
        assert_eq!(
            split_share(Axis::Vertical, flat, point(px(40.), px(20.)), 0.1),
            None
        );
        assert_eq!(
            split_share(
                Axis::Horizontal,
                Bounds::default(),
                point(px(5.), px(5.)),
                0.1
            ),
            None,
            "an unlaid-out splitter answered a drag"
        );
    }

    /// Whatever the host has stored is drawable: out of range is pinned, and a
    /// ratio that is not a number at all draws as an even split rather than
    /// poisoning every length computed from it.
    #[test]
    fn a_ratio_outside_the_range_is_pinned_to_it() {
        assert_eq!(within(-3., 0.1), 0.1);
        assert_eq!(within(0.25, 0.1), 0.25);
        assert_eq!(within(9., 0.1), 0.9);
        assert_eq!(within(f32::NAN, 0.1), DEFAULT_RATIO);
        assert_eq!(within(0.5, 0.6), 0.5);
    }

    /// A drag only answers to the splitter it started on. Nested splits see
    /// each other's moves, and an outer divider that took an inner one's drag
    /// would jump every time the inner one was touched.
    #[test]
    fn a_drag_names_the_splitter_it_belongs_to() {
        let dragged = DraggedSplit { id: "inner".into() };

        assert_eq!(dragged.id(), &ElementId::from("inner"));
        assert_ne!(dragged.id(), &ElementId::from("outer"));
    }

    /// A splitter with two children lays out in a real window and follows a
    /// drag of its divider — the half of the widget the arithmetic above cannot
    /// speak for. The halves, the seam and the occluding handle all have to
    /// survive a layout pass, the handle has to be where the ratio says it is
    /// for a press to find it at all, and the container has to hear a gesture
    /// that started on a child of its own.
    #[gpui::test]
    fn dragging_the_divider_follows_the_pointer(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let ratio = Rc::new(Cell::new(0.3));
        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), {
            let ratio = ratio.clone();
            move |_, _| Harness { ratio }
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        assert_eq!(ratio.get(), 0.3, "a splitter nobody touched moved anyway");

        // Down on the divider, then three pixels along — just past the two gpui
        // tells a drag from a click by, which is where the payload is minted.
        let middle = px(HARNESS_HEIGHT / 2.);
        let press = point(px(0.3 * HARNESS_WIDTH), middle);
        cx.simulate_mouse_move(press, None, Modifiers::none());
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(press.x + px(3.), press.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();

        // The share is measured from the container each move rather than
        // integrated from deltas, so the divider lands where the pointer is
        // whatever route it took to get there.
        let along = point(px(0.65 * HARNESS_WIDTH), middle);
        cx.simulate_mouse_move(along, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        assert!(close(ratio.get(), 0.65), "{}", ratio.get());

        // Dragged clean out of the window it stops at the minimum, and it is
        // still listening out there, well outside anything it drew.
        let past = point(px(-9_000.), middle);
        cx.simulate_mouse_move(past, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(past, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        assert!(close(ratio.get(), DEFAULT_MIN_RATIO), "{}", ratio.get());
    }
}
