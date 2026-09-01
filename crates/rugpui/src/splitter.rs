//! Two panes side by side, with a divider the pointer can move.
//!
//! One box holding two children and a number between `0.` and `1.` to say how
//! much of the box the first of them gets. Nothing here knows what is in either
//! half — a tree beside a grid, an editor above a preview, a sidebar beside
//! everything else are all the same splitter — which is the bargain every
//! widget in this crate makes, and the reason a split layout does not have to
//! be rewritten each time the thing being split changes.
//!
//! Like the rest of the kit the splitter is stateless *to its host*: the owning
//! view passes the current ratio on every render and updates its own state from
//! [`Splitter::on_change`]. One `f32` per divider is the whole of what a host
//! stores — no drag flag, no phase, no handle to keep alive between frames.
//!
//! ## Why the handle keeps a little state of its own
//!
//! A fade is not a fact about the layout, it is a fact about one pointer and one
//! divider, and no host has any use for it. So it does not go in the host's
//! struct: the handle files it under gpui's element state — the same store an
//! `on_click` uses to remember it saw a press — keyed by the splitter's own id,
//! which is unique in the window for the reason given below. It lives exactly as
//! long as the divider is on screen and is gone the moment it is not, which is
//! precisely the lifetime a fade wants.
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
//!
//! ## Why the bar is smaller than the band that answers a press
//!
//! The grab band has to be wide enough for a pointer to find; the mark drawn on
//! the seam has to be thin enough not to read as a gutter. Those are different
//! numbers, so they are two elements: an invisible band that takes the press,
//! and a rounded bar inside it that takes the accent. They differ in thickness
//! and in nothing else: the bar runs the whole length of the seam, so what the
//! pointer can grab and what the eye is told to grab end at the same place.

use std::rc::Rc;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Axis, Bounds, Context, DragMoveEvent, ElementId,
    Entity, MouseButton, Pixels, Point, Window, div, ease_in_out, prelude::*, px, relative,
};

use crate::scrollbar::{FADE_IN, FADE_OUT};
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

/// Thickness of the accent bar drawn inside the grab band, in pixels.
///
/// Half the band, near enough: thick enough to be a shape with two rounded ends
/// rather than a rule that happens to be curved, thin enough that the pointer's
/// target stays visibly larger than the thing it is aimed at.
const BAR_THICKNESS: f32 = 3.;

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

/// What the bar over the seam is doing right now.
///
/// Three states and no fourth: a fade that has run to its end is still the
/// phase that ran it, because an animation left alone sits at its last frame —
/// [`Fade::In`] finished is a bar at full strength and [`Fade::Out`] finished is
/// a bar at nothing, inside a band that paints nothing either way. Adding a
/// "shown" would only be a second name for a state already on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Fade {
    /// Never yet shown, so nothing is drawn at all — not even a transparent bar.
    #[default]
    Hidden,
    /// Coming up, or up.
    In,
    /// Going away, or gone.
    Out,
}

/// Everything the handle remembers between frames.
///
/// Kept in gpui's element state rather than in the host's struct; see the module
/// docs for why a fade is nobody else's business. `held` is here because the
/// pointer stops counting as hovering the band the moment a drag starts — gpui
/// reports every element as unhovered while a drag is in flight — and a divider
/// being dragged is the one time the bar most needs to stay up.
#[derive(Debug, Clone, Copy, Default)]
struct HandleState {
    /// What the bar is doing.
    fade: Fade,
    /// Whether a press that started on this band has yet to be released.
    held: bool,
}

impl HandleState {
    /// Re-reads the fade from `hovered` and the press, and asks for a repaint.
    ///
    /// Always notifies, even when the phase is unchanged. The repaint is the
    /// point: gpui re-checks each hover listener against the pointer as it
    /// paints, so the frame drawn after a release is what tells the band it is
    /// being hovered again — a fact it could not learn during the drag, and
    /// without which the bar would have no way to hear the pointer leave.
    fn shift(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.fade = next_fade(self.fade, hovered, self.held);
        cx.notify();
    }
}

/// The phase a bar in `current` moves to, given a pointer and a press.
///
/// The whole of the handle's behaviour, and pure, so that the awkward pairs —
/// the pointer leaving mid-drag, a release with the pointer still on the band —
/// can be stated as answers rather than traced through gpui's event order.
///
/// A press outranks the pointer: while the divider is held the bar stays up
/// wherever the pointer has got to, including outside the window. And a bar
/// that was never shown stays hidden rather than fading out of nothing, which
/// would draw a frame of full-strength accent nobody asked for.
fn next_fade(current: Fade, hovered: bool, held: bool) -> Fade {
    if hovered || held {
        Fade::In
    } else if current == Fade::Hidden {
        Fade::Hidden
    } else {
        Fade::Out
    }
}

/// Ends a press that did not finish on the band, and fades the bar out with it.
///
/// A no-op unless a press of this splitter's own band is outstanding, which is
/// what keeps an ordinary click in either pane — and a release the band has
/// already answered for itself — from touching the bar.
fn end_press(state: &Entity<HandleState>, cx: &mut App) {
    state.update(cx, |handle, cx| {
        if !handle.held {
            return;
        }
        handle.held = false;
        handle.shift(false, cx);
    });
}

/// Where a splitter's handle files its fade.
///
/// One function so that the key is written once: the state is looked up by this
/// id on every frame, and a splitter whose key drifted between frames would get
/// a fresh, hidden bar each time it was drawn.
fn fade_key(id: &ElementId) -> ElementId {
    ElementId::from((id.clone(), "handle-fade"))
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
    bar: Pixels,
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
            bar: px(BAR_THICKNESS),
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

    /// Sets how thick the bar drawn inside the grab band is, in pixels.
    ///
    /// Defaults to 3 px, and is clamped to the band's own thickness: a bar wider
    /// than the thing it lives in would be a rectangle with its rounded ends
    /// clipped off, which is worse than either shape on its own. Set it to zero
    /// for a splitter that answers a press but never marks itself.
    pub fn bar_thickness(mut self, thickness: Pixels) -> Self {
        self.bar = thickness;
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
    ///
    /// The band itself paints nothing. What the eye follows is the bar inside
    /// it, and it is a separate element for the reason in the module docs: the
    /// target and the mark want different sizes.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = theme(cx);
        let axis = self.axis;
        let id = self.id;
        let min = self.min_ratio;
        let ratio = within(self.ratio, min);
        let thickness = self.thickness;
        let on_change = self.on_change;

        // Looked up rather than passed in, and looked up again on every frame:
        // the entity gpui hands back is the same one for as long as the divider
        // keeps being drawn, and it already notifies this view when it changes,
        // so nothing here has to arrange a repaint of its own.
        let state = window.use_keyed_state(fade_key(&id), cx, |_, _| HandleState::default());
        let fade = state.read(cx).fade;

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

        // Clamped here rather than in the builder so that a host may set the two
        // thicknesses in either order and still get a bar that fits its band.
        let band_width = f32::from(thickness).max(0.);
        let bar_width = px(f32::from(self.bar).clamp(0., band_width));
        // Centred across the band by the room left over, which is the one
        // measurement that stays right whatever either thickness is set to.
        let gutter = px((band_width - f32::from(bar_width)) / 2.);

        let shape = div()
            .absolute()
            .rounded_full()
            .bg(palette.accent)
            .map(|bar| match axis {
                Axis::Horizontal => bar.top_0().bottom_0().left(gutter).w(bar_width),
                Axis::Vertical => bar.left_0().right_0().top(gutter).h(bar_width),
            });

        // The same two durations the scrollbar fades on, on purpose: two
        // overlays that appear under the pointer and leave when it goes should
        // breathe at one rate, or the window looks assembled from parts.
        //
        // Each phase animates under an id of its own. gpui keeps an animation's
        // start time in element state keyed by that id and drops it once the id
        // stops being drawn, so switching phase restarts the new one from zero
        // while staying on one phase — every frame of a drag, say — leaves the
        // clock running and the bar does not blink.
        let bar = match fade {
            Fade::Hidden => None,
            Fade::In => Some(
                shape
                    .with_animation(
                        ElementId::from((id.clone(), "bar-fade-in")),
                        Animation::new(FADE_IN).with_easing(ease_in_out),
                        |bar, delta| bar.opacity(delta),
                    )
                    .into_any_element(),
            ),
            Fade::Out => Some(
                shape
                    .with_animation(
                        ElementId::from((id.clone(), "bar-fade-out")),
                        Animation::new(FADE_OUT).with_easing(ease_in_out),
                        |bar, delta| bar.opacity(1. - delta),
                    )
                    .into_any_element(),
            ),
        };

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
            // The pointer arriving and the pointer leaving, and nothing else:
            // what either one means is decided by `next_fade`, which also knows
            // about the press this listener cannot see.
            .on_hover({
                let state = state.clone();
                move |hovered: &bool, _window, cx| {
                    let hovered = *hovered;
                    state.update(cx, |handle, cx| handle.shift(hovered, cx));
                }
            })
            // Taken on the press rather than when the drag is minted, since gpui
            // only mints one after the pointer has moved far enough to prove it
            // was a drag, and the bar should be up for the whole gesture.
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |_, _window, cx| {
                    state.update(cx, |handle, cx| {
                        handle.held = true;
                        handle.shift(true, cx);
                    });
                }
            })
            // A release that lands on the band itself: the pointer is provably
            // still here — gpui only runs this listener when the band is under
            // it — so the bar stays up and there is nothing to fade. Clearing
            // `held` here is also what tells the container's listener below,
            // which runs next and cannot tell where the release landed, that
            // this release has already been accounted for.
            .on_mouse_up(MouseButton::Left, {
                let state = state.clone();
                move |_, _window, cx| {
                    state.update(cx, |handle, cx| {
                        handle.held = false;
                        handle.shift(true, cx);
                    });
                }
            })
            // An empty preview: the divider follows the pointer directly, so a
            // ghost trailing it would only be a second thing to watch.
            .on_drag(DraggedSplit { id: id.clone() }, |_, _, _, cx| {
                cx.new(|_| gpui::Empty)
            })
            .children(bar);

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
            // A drag that ends anywhere but on the band — which, once the ratio
            // has hit its minimum, is most of them, because the divider stops
            // and the pointer runs on. Two listeners for the one event: gpui
            // asks separately about a release inside this box and a release
            // outside it, and a gesture that left the window is still a gesture
            // this divider has to hear the end of.
            //
            // Both are no-ops unless a press of this splitter's own band is
            // outstanding, so an ordinary click in either pane leaves the bar
            // alone, and so does a release the band has already answered for.
            .on_mouse_up(MouseButton::Left, {
                let state = state.clone();
                move |_, _window, cx| end_press(&state, cx)
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _window, cx| {
                end_press(&state, cx)
            })
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
    use std::cell::{Cell, RefCell};
    use std::ops::Deref;

    use gpui::{Entity, Modifiers, Render, TestAppContext, VisualTestContext, point, size};

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
        /// The handle's own fade, once a frame has been drawn.
        ///
        /// Reached by asking gpui for the very key the splitter files it under,
        /// from the same place in the element tree the splitter renders from —
        /// which is what makes it the same entity rather than a second one. If
        /// that ever stopped being true the tests below would read a state
        /// nobody writes to, and would fail rather than quietly pass.
        fade: Watched,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let ratio = self.ratio.clone();
            // gpui isolates a stateless view's subtree under its type name — see
            // `ViewElement::request_layout` — so the splitter's own element ids
            // sit one level below the harness's. Standing in the same place is
            // what makes this the splitter's state rather than a second copy of
            // it, and the assertions below fail loudly if that ever stops being
            // true.
            let handle = window.with_id(
                ElementId::Name(std::any::type_name::<Splitter>().into()),
                |window| {
                    window.use_keyed_state(fade_key(&"split".into()), cx, |_, _| {
                        HandleState::default()
                    })
                },
            );
            *self.fade.borrow_mut() = Some(handle);
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

    /// The pure half of the handle's behaviour, stated as answers.
    ///
    /// The two that matter are the ones event order alone would get wrong: the
    /// pointer "leaving" the moment a drag starts, which must not take the bar
    /// with it, and a release with the pointer still on the band, which must
    /// not fade anything out.
    #[test]
    fn a_press_outranks_the_pointer() {
        // Arriving shows it, from either of the two states it can arrive in.
        assert_eq!(next_fade(Fade::Hidden, true, false), Fade::In);
        assert_eq!(next_fade(Fade::Out, true, false), Fade::In);
        assert_eq!(next_fade(Fade::In, true, false), Fade::In);

        // Leaving takes it away — unless the divider is being dragged, which is
        // exactly when gpui reports the band as unhovered.
        assert_eq!(next_fade(Fade::In, false, false), Fade::Out);
        assert_eq!(next_fade(Fade::In, false, true), Fade::In);

        // A release with the pointer still on the band changes nothing; one
        // with the pointer elsewhere fades out.
        assert_eq!(next_fade(Fade::In, true, false), Fade::In);
        assert_eq!(next_fade(Fade::In, false, false), Fade::Out);

        // A bar that was never shown has nothing to fade out of.
        assert_eq!(next_fade(Fade::Hidden, false, false), Fade::Hidden);
    }

    /// Where the harness leaves the handle's state for the test to find.
    ///
    /// An `Option` because it is only filled once a frame has been drawn, and a
    /// cell because the harness writes it from its own render.
    type Watched = Rc<RefCell<Option<Entity<HandleState>>>>;

    /// The state a splitter's handle keeps, reached from the test.
    fn fade_state(state: &Watched, cx: &mut VisualTestContext) -> HandleState {
        let state = state.borrow().clone().expect("a frame was drawn");
        cx.read(|cx| *state.read(cx))
    }

    /// Opens the harness and returns the two things the tests watch.
    fn open(cx: &mut TestAppContext, start: f32) -> (Rc<Cell<f32>>, Watched, VisualTestContext) {
        cx.update(crate::init);

        let ratio = Rc::new(Cell::new(start));
        let fade: Watched = Rc::default();
        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), {
            let ratio = ratio.clone();
            let fade = fade.clone();
            move |_, _| Harness { ratio, fade }
        });
        let visual = VisualTestContext::from_window(*window.deref(), cx);
        visual.run_until_parked();
        (ratio, fade, visual)
    }

    /// Where the band sits when the divider is at `share`, halfway down it.
    fn on_the_band(share: f32) -> Point<Pixels> {
        point(px(share * HARNESS_WIDTH), px(HARNESS_HEIGHT / 2.))
    }

    /// The bar comes up when the pointer arrives and goes again when it leaves,
    /// and neither is something the host was asked to remember.
    #[gpui::test]
    fn the_bar_follows_the_pointer_onto_the_band_and_off_it(cx: &mut TestAppContext) {
        let (_ratio, fade, mut cx) = open(cx, 0.3);
        assert_eq!(
            fade_state(&fade, &mut cx).fade,
            Fade::Hidden,
            "a splitter nobody has pointed at drew its bar anyway"
        );

        cx.simulate_mouse_move(on_the_band(0.3), None, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::In);

        // Well away from the band, and still inside the window: the bar leaves
        // rather than staying up for the rest of the session.
        cx.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::Out);
    }

    /// The bar stays up for the whole of a drag, however far the pointer runs
    /// ahead of the divider — which it always does once the ratio has hit its
    /// minimum and the band has stopped moving. gpui reports every element as
    /// unhovered while a drag is in flight, so this is the case the press flag
    /// exists for.
    #[gpui::test]
    fn a_dragged_divider_keeps_its_bar_wherever_the_pointer_goes(cx: &mut TestAppContext) {
        let (ratio, fade, mut cx) = open(cx, 0.3);

        let press = on_the_band(0.3);
        cx.simulate_mouse_move(press, None, Modifiers::none());
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let held = fade_state(&fade, &mut cx);
        assert!(held.held, "a press on the band was not noticed");
        assert_eq!(held.fade, Fade::In);

        // Past the two pixels gpui tells a drag from a click by, then far
        // enough that the divider is pinned at its minimum and the pointer is
        // nowhere near the band any more.
        cx.simulate_mouse_move(
            point(press.x + px(3.), press.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();
        let mid = fade_state(&fade, &mut cx);
        assert!(mid.held);
        assert_eq!(mid.fade, Fade::In, "the bar went out mid-drag");

        let far = point(px(-9_000.), press.y);
        cx.simulate_mouse_move(far, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        let outside = fade_state(&fade, &mut cx);
        assert!(outside.held);
        assert_eq!(
            outside.fade,
            Fade::In,
            "the bar went out when the pointer left the window"
        );
        assert!(close(ratio.get(), DEFAULT_MIN_RATIO), "{}", ratio.get());

        // Released out there, the press is over and the bar has no reason to
        // stay: nothing is under the pointer to keep it up.
        cx.simulate_mouse_up(far, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let done = fade_state(&fade, &mut cx);
        assert!(!done.held, "the press outlived the release");
        assert_eq!(done.fade, Fade::Out);
    }

    /// A release with the pointer still on the band leaves the bar up. It is
    /// the common ending — a short drag that never reaches the minimum keeps
    /// the divider under the pointer — and fading out and straight back in
    /// would blink once on every one of them.
    #[gpui::test]
    fn a_release_on_the_band_leaves_the_bar_up(cx: &mut TestAppContext) {
        let (_ratio, fade, mut cx) = open(cx, 0.3);

        let press = on_the_band(0.3);
        cx.simulate_mouse_move(press, None, Modifiers::none());
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let released = fade_state(&fade, &mut cx);
        assert!(!released.held);
        assert_eq!(
            released.fade,
            Fade::In,
            "a release under the pointer blinked"
        );

        // And the bar can still hear the pointer leave afterwards, which is the
        // thing a release that skipped the repaint would have broken.
        cx.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::Out);
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
            move |_, _| Harness {
                ratio,
                fade: Rc::default(),
            }
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
