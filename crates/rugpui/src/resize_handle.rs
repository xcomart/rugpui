//! A band the pointer can grab to resize something, and the bar it lights up.
//!
//! One element, drawn over whatever it is resizing rather than wedged beside
//! it: an invisible band that takes the press and the resize cursor, and a
//! rounded accent bar inside it that fades in while the pointer is on the band
//! or holding it. It knows nothing about what the gesture means — the drag
//! payload is the host's, and so is every pixel the drag goes on to move.
//! [`Splitter`](crate::splitter::Splitter) uses it for the divider between two
//! panes; a panel whose width is a setting uses it on its own trailing edge;
//! neither had to write the fade.
//!
//! What it does *not* do is turn a pointer position into a number. That is the
//! host's, because only the host knows what box the pointer should be measured
//! against — see [`split_share`](crate::splitter::split_share) for the
//! arithmetic and [`splitter`](crate::splitter) for why the container, not the
//! band, is the thing to measure against.
//!
//! ## Why the handle keeps a little state of its own
//!
//! A fade is not a fact about the layout, it is a fact about one pointer and one
//! band, and no host has any use for it. So it does not go in the host's struct:
//! the handle files it under gpui's element state — the same store an `on_click`
//! uses to remember it saw a press — keyed by the handle's own id. It lives
//! exactly as long as the band is on screen and is gone the moment it is not,
//! which is precisely the lifetime a fade wants.
//!
//! That is also why the id has to be unique in the window rather than merely
//! among its siblings: two handles sharing an id would share a fade, and light
//! up together.
//!
//! ## Why the bar is smaller than the band that answers a press
//!
//! The grab band has to be wide enough for a pointer to find; the mark drawn on
//! the seam has to be thin enough not to read as a gutter. Those are different
//! numbers, so they are two elements: an invisible band that takes the press,
//! and a rounded bar inside it that takes the accent. They differ in thickness
//! and in nothing else: the bar runs the whole length of the band, so what the
//! pointer can grab and what the eye is told to grab end at the same place.
//!
//! ## Why the bar is not always centred in its band
//!
//! Where the bar sits across the band depends on what the band is marking.
//! [`ResizeHandle::at`] puts the band astride a line — the seam between two
//! panes — so the grab area is symmetric about it and the bar is centred in the
//! band, which puts it back on the line. [`ResizeHandle::at_end`] and
//! [`ResizeHandle::at_start`] put the whole band *inside* the thing being
//! resized, flush with one of its edges, and the line the eye already sees
//! there is that edge — a panel's hairline border, usually. A bar centred in
//! such a band would float a few pixels in from the border and read as a second
//! rule beside it, so it is pushed flush with the same edge the band is and
//! lands on top of the border instead.
//!
//! ## Why the release is heard twice
//!
//! A drag that ends anywhere but on the band is the common case, not the odd
//! one: once whatever is being resized hits its limit the band stops and the
//! pointer runs on, often clean out of the window. gpui asks separately about a
//! release on an element and a release away from it — `on_mouse_up` and
//! `on_mouse_up_out` — and between them the two cover every release there is, so
//! the band hears its own gesture end without the host having to lend it a
//! listener. A release on the band leaves the bar up, since the pointer is
//! provably still there; one anywhere else fades it out.

use gpui::{
    Animation, AnimationExt, App, Axis, Context, ElementId, Entity, Length, MouseButton, Pixels,
    Window, div, ease_in_out, prelude::*, px,
};

use crate::scrollbar::{FADE_IN, FADE_OUT};
use crate::theme::theme;

/// Thickness of the band that answers a press, in pixels.
///
/// Wider than the line the eye sees, for the same reason a scrollbar's grab
/// area is wider than its thumb: a one pixel target is not one a pointer can be
/// expected to find.
pub(crate) const DEFAULT_THICKNESS: f32 = 6.;

/// Thickness of the accent bar drawn inside the grab band, in pixels.
///
/// Half the band, near enough: thick enough to be a shape with two rounded ends
/// rather than a rule that happens to be curved, thin enough that the pointer's
/// target stays visibly larger than the thing it is aimed at.
pub(crate) const DEFAULT_BAR: f32 = 3.;

/// What the bar over the band is doing right now.
///
/// Three states and no fourth: a fade that has run to its end is still the
/// phase that ran it, because an animation left alone sits at its last frame —
/// [`Fade::In`] finished is a bar at full strength and [`Fade::Out`] finished is
/// a bar at nothing, inside a band that paints nothing either way. Adding a
/// "shown" would only be a second name for a state already on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Fade {
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
/// reports every element as unhovered while a drag is in flight — and a handle
/// being dragged is the one time the bar most needs to stay up.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HandleState {
    /// What the bar is doing.
    pub(crate) fade: Fade,
    /// Whether a press that started on this band has yet to be released.
    pub(crate) held: bool,
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
/// A press outranks the pointer: while the band is held the bar stays up
/// wherever the pointer has got to, including outside the window. And a bar
/// that was never shown stays hidden rather than fading out of nothing, which
/// would draw a frame of full-strength accent nobody asked for.
pub(crate) fn next_fade(current: Fade, hovered: bool, held: bool) -> Fade {
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
/// A no-op unless a press of this band is outstanding, which is what keeps an
/// ordinary click elsewhere in the window — and a release the band has already
/// answered for itself — from touching the bar.
fn end_press(state: &Entity<HandleState>, cx: &mut App) {
    state.update(cx, |handle, cx| {
        if !handle.held {
            return;
        }
        handle.held = false;
        handle.shift(false, cx);
    });
}

/// Where a handle files its fade.
///
/// One function so that the key is written once: the state is looked up by this
/// id on every frame, and a handle whose key drifted between frames would get a
/// fresh, hidden bar each time it was drawn.
pub(crate) fn fade_key(id: &ElementId) -> ElementId {
    ElementId::from((id.clone(), "handle-fade"))
}

/// Where along the axis the band sits, and which edge of it the bar hugs.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placement {
    /// Centred on an offset from the container's leading edge.
    At(Length),
    /// Flush with the container's leading edge — its left, or its top.
    Start,
    /// Flush with the container's trailing edge — its right, or its bottom.
    End,
}

/// A grab band that starts a resize drag, with an accent bar that fades in
/// under the pointer.
///
/// Absolutely positioned, so the nearest positioned ancestor — a `relative()`
/// box — is what it places itself against, and it paints over that box's
/// contents rather than taking any of its room. It starts a gpui drag carrying
/// `payload`; where the pointer goes afterwards is the host's to read, from an
/// `on_drag_move` listener on the box the gesture should be measured against.
///
/// `id` has to be unique among the handles that can be on screen at the same
/// time — in practice, within the window — because it is the key the fade is
/// filed under.
///
/// ```ignore
/// // A sidebar whose width is a setting, resized from its own right edge.
/// div()
///     .relative()
///     .w(px(self.width))
///     .child(contents)
///     .child(
///         ResizeHandle::new("files-width", Axis::Horizontal, DraggedPanelEdge)
///             .at_end(),
///     )
///     .on_drag_move(cx.listener(Self::resize))
/// ```
#[derive(IntoElement)]
pub struct ResizeHandle<T: 'static> {
    id: ElementId,
    axis: Axis,
    payload: T,
    placement: Placement,
    thickness: Pixels,
    bar: Pixels,
}

impl<T: 'static> ResizeHandle<T> {
    /// Creates a handle that drags along `axis`, carrying `payload`.
    ///
    /// [`Axis::Horizontal`] is a band that moves left and right — so a tall,
    /// thin one, with the east-west resize cursor; [`Axis::Vertical`] is the
    /// transpose. The axis names the direction of travel, not the shape of the
    /// band, which is the same thing a [`Splitter`](crate::splitter::Splitter)'s
    /// axis names.
    ///
    /// `payload` is the value gpui hands to every `on_drag_move` listener for
    /// its type while the gesture is in flight. Nothing here reads it: give it a
    /// type of your own, and — if handles of that type can nest, as splitters do
    /// — put something in it that says which handle the drag started on.
    ///
    /// The band is placed at the container's trailing edge until told otherwise;
    /// see [`at`](Self::at) and [`at_start`](Self::at_start) for the other two.
    pub fn new(id: impl Into<ElementId>, axis: Axis, payload: T) -> Self {
        Self {
            id: id.into(),
            axis,
            payload,
            placement: Placement::End,
            thickness: px(DEFAULT_THICKNESS),
            bar: px(DEFAULT_BAR),
        }
    }

    /// Centres the band on `offset` from the container's leading edge.
    ///
    /// The placement for a band astride a line rather than inside an edge: a
    /// splitter's divider sits at `relative(ratio)` of its container and the
    /// band is pulled back half its own thickness, so the grab area is symmetric
    /// about the seam. The bar is centred in the band to match, which puts it
    /// back on the line the band was centred on.
    pub fn at(mut self, offset: impl Into<Length>) -> Self {
        self.placement = Placement::At(offset.into());
        self
    }

    /// Puts the band inside the container's trailing edge — its right on
    /// [`Axis::Horizontal`], its bottom on [`Axis::Vertical`].
    ///
    /// The placement for a panel that is resized by dragging its own edge. The
    /// whole band is inside the panel, so none of it hangs over the neighbour,
    /// and the bar is pushed flush with the same edge rather than centred: what
    /// is already drawn there is the panel's own border, and the bar lands on
    /// top of it instead of floating a few pixels in from it.
    pub fn at_end(mut self) -> Self {
        self.placement = Placement::End;
        self
    }

    /// Puts the band inside the container's leading edge — its left on
    /// [`Axis::Horizontal`], its top on [`Axis::Vertical`].
    ///
    /// The mirror of [`at_end`](Self::at_end), for a panel on the other side.
    pub fn at_start(mut self) -> Self {
        self.placement = Placement::Start;
        self
    }

    /// Sets how thick the band that answers a press is, in pixels.
    ///
    /// Defaults to 6 px. This is the grab area alone: widening it makes the
    /// handle easier to hit without making the bar the eye sees any heavier.
    pub fn thickness(mut self, thickness: Pixels) -> Self {
        self.thickness = thickness;
        self
    }

    /// Sets how thick the bar drawn inside the grab band is, in pixels.
    ///
    /// Defaults to 3 px, and is clamped to the band's own thickness: a bar wider
    /// than the thing it lives in would be a rectangle with its rounded ends
    /// clipped off, which is worse than either shape on its own. Set it to zero
    /// for a handle that answers a press but never marks itself.
    pub fn bar_thickness(mut self, thickness: Pixels) -> Self {
        self.bar = thickness;
        self
    }
}

impl<T: 'static> RenderOnce for ResizeHandle<T> {
    /// An occluding band with a bar inside it.
    ///
    /// The band occludes because a plain hitbox would let the press reach
    /// whatever is underneath as well, and paints nothing itself: what the eye
    /// follows is the bar, which is a separate element for the reason in the
    /// module docs — the target and the mark want different sizes.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = theme(cx);
        let axis = self.axis;
        let id = self.id;
        let placement = self.placement;
        let thickness = self.thickness;

        // Looked up rather than passed in, and looked up again on every frame:
        // the entity gpui hands back is the same one for as long as the band
        // keeps being drawn, and it already notifies this view when it changes,
        // so nothing here has to arrange a repaint of its own.
        let state = window.use_keyed_state(fade_key(&id), cx, |_, _| HandleState::default());
        let fade = state.read(cx).fade;

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
            .map(|bar| match (axis, placement) {
                (Axis::Horizontal, Placement::At(_)) => {
                    bar.top_0().bottom_0().left(gutter).w(bar_width)
                }
                (Axis::Horizontal, Placement::Start) => {
                    bar.top_0().bottom_0().left_0().w(bar_width)
                }
                (Axis::Horizontal, Placement::End) => bar.top_0().bottom_0().right_0().w(bar_width),
                (Axis::Vertical, Placement::At(_)) => {
                    bar.left_0().right_0().top(gutter).h(bar_width)
                }
                (Axis::Vertical, Placement::Start) => bar.left_0().right_0().top_0().h(bar_width),
                (Axis::Vertical, Placement::End) => bar.left_0().right_0().bottom_0().h(bar_width),
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

        // Pulled back half its own thickness, so that a band centred on a line
        // is symmetric about it. The two flush placements need no such offset:
        // the whole band is meant to be inside the edge it names.
        let offset = px(-f32::from(thickness) / 2.);
        div()
            .id(id)
            .absolute()
            .occlude()
            .map(|band| match axis {
                Axis::Horizontal => {
                    let band = band.top_0().bottom_0().w(thickness).cursor_ew_resize();
                    match placement {
                        Placement::At(offset_from_start) => band.left(offset_from_start).ml(offset),
                        Placement::Start => band.left_0(),
                        Placement::End => band.right_0(),
                    }
                }
                Axis::Vertical => {
                    let band = band.left_0().right_0().h(thickness).cursor_ns_resize();
                    match placement {
                        Placement::At(offset_from_start) => band.top(offset_from_start).mt(offset),
                        Placement::Start => band.top_0(),
                        Placement::End => band.bottom_0(),
                    }
                }
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
            // it — so the bar stays up and there is nothing to fade.
            .on_mouse_up(MouseButton::Left, {
                let state = state.clone();
                move |_, _window, cx| {
                    state.update(cx, |handle, cx| {
                        handle.held = false;
                        handle.shift(true, cx);
                    });
                }
            })
            // And the other half of the same event: a release anywhere but on
            // the band, which is where most of them land once whatever is being
            // resized has hit its limit and the pointer has run on. A no-op
            // unless a press of this band is outstanding, so an ordinary click
            // elsewhere in the window leaves the bar alone.
            .on_mouse_up_out(MouseButton::Left, move |_, _window, cx| {
                end_press(&state, cx)
            })
            // An empty preview: whatever is being resized follows the pointer
            // directly, so a ghost trailing it would only be a second thing to
            // watch.
            .on_drag(self.payload, |_, _, _, cx| cx.new(|_| gpui::Empty))
            .children(bar)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::ops::Deref;
    use std::rc::Rc;

    use gpui::{
        Modifiers, Point, Render, TestAppContext, VisualTestContext, point, relative, size,
    };

    use super::*;

    /// The payload the harness below drags with, which is also the simplest
    /// shape a payload can have: a handle that cannot nest has nothing to say.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct DraggedEdge;

    /// Where the harness leaves the handle's state for the test to find.
    ///
    /// An `Option` because it is only filled once a frame has been drawn, and a
    /// cell because the harness writes it from its own render.
    type Watched = Rc<RefCell<Option<Entity<HandleState>>>>;

    /// Width of the window the placement test runs in.
    const HARNESS_WIDTH: f32 = 300.;

    /// Height of the same window.
    const HARNESS_HEIGHT: f32 = 120.;

    /// Width of the panel whose trailing edge carries the handle.
    const PANEL_WIDTH: f32 = 200.;

    /// A panel with a handle on its own right edge — the placement a sidebar
    /// whose width is a setting uses, and the one the arithmetic above says
    /// nothing about: whether the band is where `at_end` claims it is can only
    /// be found out by pointing at it.
    struct Harness {
        /// The handle's own fade, once a frame has been drawn.
        ///
        /// Reached by asking gpui for the very key the handle files it under,
        /// from the same place in the element tree the handle renders from —
        /// which is what makes it the same entity rather than a second one.
        fade: Watched,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            // gpui isolates a stateless view's subtree under its type name — see
            // `ViewElement::request_layout` — so the handle's own element ids
            // sit one level below the harness's. Standing in the same place is
            // what makes this the handle's state rather than a second copy of
            // it, and the assertions below fail loudly if that ever stops being
            // true.
            let state = window.with_id(
                ElementId::Name(std::any::type_name::<ResizeHandle<DraggedEdge>>().into()),
                |window| {
                    window.use_keyed_state(fade_key(&"edge".into()), cx, |_, _| {
                        HandleState::default()
                    })
                },
            );
            *self.fade.borrow_mut() = Some(state);
            div().size_full().child(
                div()
                    .relative()
                    .w(px(PANEL_WIDTH))
                    .h_full()
                    .child(ResizeHandle::new("edge", Axis::Horizontal, DraggedEdge).at_end()),
            )
        }
    }

    /// The state a handle keeps, reached from the test.
    fn fade_state(state: &Watched, cx: &mut VisualTestContext) -> HandleState {
        let state = state.borrow().clone().expect("a frame was drawn");
        cx.read(|cx| *state.read(cx))
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

        // Leaving takes it away — unless the band is being dragged, which is
        // exactly when gpui reports it as unhovered.
        assert_eq!(next_fade(Fade::In, false, false), Fade::Out);
        assert_eq!(next_fade(Fade::In, false, true), Fade::In);

        // A bar that was never shown has nothing to fade out of.
        assert_eq!(next_fade(Fade::Hidden, false, false), Fade::Hidden);
    }

    /// A bar is clamped to its band, in either order, and a band with no
    /// thickness has no bar to draw rather than a negative one.
    #[test]
    fn the_bar_never_outgrows_the_band() {
        let clamp = |band: f32, bar: f32| f32::from(px(bar)).clamp(0., f32::from(px(band)).max(0.));

        assert_eq!(clamp(DEFAULT_THICKNESS, DEFAULT_BAR), DEFAULT_BAR);
        assert_eq!(clamp(2., 9.), 2.);
        assert_eq!(clamp(0., 3.), 0.);
        assert_eq!(clamp(-4., 3.), 0.);
    }

    /// Opens the harness and returns the handle's state and the window.
    fn open(cx: &mut TestAppContext) -> (Watched, VisualTestContext) {
        cx.update(crate::init);

        let fade: Watched = Rc::default();
        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), {
            let fade = fade.clone();
            move |_, _| Harness { fade }
        });
        let visual = VisualTestContext::from_window(*window.deref(), cx);
        visual.run_until_parked();
        (fade, visual)
    }

    /// Two pixels in from the panel's right edge, halfway down it — inside the
    /// band only if the band really is flush with that edge rather than centred
    /// on it or left at the container's start.
    fn on_the_band() -> Point<Pixels> {
        point(px(PANEL_WIDTH - 2.), px(HARNESS_HEIGHT / 2.))
    }

    /// A press that is released somewhere else *inside the window* still takes
    /// the bar down with it.
    ///
    /// The case the band's own `on_mouse_up_out` exists for, and the one a host
    /// used to have to lend the handle a container listener for: gpui only runs
    /// `on_mouse_up` when the band is under the pointer, so a gesture that
    /// wandered a few pixels off it would otherwise leave the bar up for the
    /// rest of the session with nothing under the pointer to justify it.
    #[gpui::test]
    fn a_release_off_the_band_takes_the_bar_down(cx: &mut TestAppContext) {
        let (fade, mut cx) = open(cx);

        let press = on_the_band();
        cx.simulate_mouse_move(press, None, Modifiers::none());
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let held = fade_state(&fade, &mut cx);
        assert!(held.held, "a press on the band was not noticed");
        assert_eq!(held.fade, Fade::In);

        // Past the two pixels gpui tells a drag from a click by, then well
        // clear of the band but still inside the window — which is where most
        // of these gestures end, since the panel stops and the pointer does not.
        cx.simulate_mouse_move(
            point(press.x + px(3.), press.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        let off = point(px(20.), press.y);
        cx.simulate_mouse_move(off, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            fade_state(&fade, &mut cx).fade,
            Fade::In,
            "the bar went out mid-drag"
        );

        cx.simulate_mouse_up(off, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let done = fade_state(&fade, &mut cx);
        assert!(!done.held, "the press outlived the release");
        assert_eq!(done.fade, Fade::Out);
    }

    /// A release *on* the band leaves the bar up: the pointer is provably still
    /// there, and fading out and straight back in would blink once on every
    /// short drag — which is most of them.
    #[gpui::test]
    fn a_release_on_the_band_leaves_the_bar_up(cx: &mut TestAppContext) {
        let (fade, mut cx) = open(cx);

        let press = on_the_band();
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
        cx.simulate_mouse_move(point(px(20.), press.y), None, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::Out);
    }

    /// The band is where `at_end` says it is, and the bar comes up when the
    /// pointer finds it — the half of the widget no arithmetic can speak for,
    /// since it takes a layout pass to put the band anywhere at all.
    #[gpui::test]
    fn a_handle_on_a_trailing_edge_lights_up_under_the_pointer(cx: &mut TestAppContext) {
        let (fade, mut cx) = open(cx);
        assert_eq!(
            fade_state(&fade, &mut cx).fade,
            Fade::Hidden,
            "a handle nobody has pointed at drew its bar anyway"
        );

        cx.simulate_mouse_move(on_the_band(), None, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::In);

        // Well inside the panel and nowhere near its edge: the bar leaves.
        cx.simulate_mouse_move(
            point(px(20.), px(HARNESS_HEIGHT / 2.)),
            None,
            Modifiers::none(),
        );
        cx.run_until_parked();
        assert_eq!(fade_state(&fade, &mut cx).fade, Fade::Out);

        // And the band that was never asked for a different placement is in the
        // same place, since `at_end` is the default rather than a thing a host
        // has to remember to say.
        assert_eq!(
            ResizeHandle::new("edge", Axis::Horizontal, DraggedEdge).placement,
            ResizeHandle::new("edge", Axis::Horizontal, DraggedEdge)
                .at_end()
                .placement
        );
        assert_ne!(
            Placement::End,
            ResizeHandle::new("edge", Axis::Horizontal, DraggedEdge)
                .at(relative(0.5))
                .placement
        );
    }
}
