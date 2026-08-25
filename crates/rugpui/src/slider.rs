//! A horizontal slider over a fraction.
//!
//! One knob riding one track, and a number between `0.` and `1.` to say where
//! it sits. Nothing here knows what that number means: a volume, a font size, a
//! timeout in seconds are all the same slider, and turning the fraction into
//! whatever the host actually stores — and back again — is the host's business.
//! That is the same bargain every widget in this crate makes, and it is what
//! keeps a slider from having to hear about units, formatting or locales.
//!
//! Like the rest of the kit the slider is stateless: the owning view passes the
//! current value on every render and updates its own state from
//! [`Slider::on_change`]. There is nothing to keep beside it and nothing to
//! initialise.
//!
//! Three ways to move it, and all three end at the same callback:
//!
//! * Dragging the knob, which is the interesting one and is described below.
//! * Pressing the track, which puts the knob's centre where the pointer landed.
//! * The arrow keys, while the slider holds focus. Those move in steps of
//!   [`Slider::step`] and snap to that step's grid, so a value a drag left at
//!   `0.42` becomes `0.45` rather than `0.47` — the behaviour a stepped control
//!   is expected to have, and the only reason a step exists at all.
//!
//! ## How a drag finds its way home
//!
//! The same route [`crate::scrollbar`] takes, for the same reason: gpui hands a
//! `DragMoveEvent` to *every* element listening for that drag type, and the
//! `bounds` on the event belong to the listener rather than to the knob. So the
//! knob carries its own track — in window coordinates — inside [`DraggedKnob`],
//! along with where in the knob the press landed, and a pointer position is
//! enough to answer "what value is this now?" without having seen the press.
//!
//! Those track bounds cannot be known while the element is being built, because
//! nothing has been laid out yet. A [`canvas`] the width of the track writes
//! them into a cell the payload shares, during the prepaint of the very frame
//! the element was built in — which is over long before any press can arrive.
//! Every render makes a fresh cell, so a payload stops being updated the moment
//! its frame is replaced, and a drag therefore measures against the track as it
//! stood when the drag began. That is exactly the scrollbar's rule, arrived at
//! from the other end.
//!
//! The slider listens for its own drags, so a plain [`Slider::on_change`] is all
//! a host needs. [`Slider::dragged`] is there for the host that wants to read
//! the gesture itself — to drive something the callback does not cover, or to
//! keep the drag and the scrollbar's on one handler — and both may be used at
//! once, since they agree on the value.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, DragMoveEvent, ElementId, MouseButton, MouseDownEvent, Pixels, Point, Window,
    canvas, div, prelude::*, px, relative, transparent_black,
};

use crate::scrollbar::dragged_to;
use crate::theme::theme;

/// Diameter of the knob, in pixels.
///
/// Also the height of the whole control, since the knob is the tallest thing in
/// it, and the length of track the knob covers — which is why the value `0.`
/// leaves the knob at the very start of the track rather than hanging off it.
const KNOB: f32 = 14.;

/// Half the knob, in pixels.
///
/// The distance from the knob's leading edge to its centre, which is where a
/// press on the bare track wants the knob to end up and where the filled part
/// of the track stops.
const HALF_KNOB: f32 = KNOB / 2.;

/// Thickness of the track, in pixels.
const TRACK: f32 = 4.;

/// Distance from the top of the control to the top of the track, in pixels.
///
/// Centres the thin track against the knob that rides it.
const TRACK_TOP: f32 = (KNOB - TRACK) / 2.;

/// How far an arrow key moves a slider that has not said otherwise.
///
/// Twenty steps from end to end: coarse enough to cross the range without
/// holding a key down, fine enough that a host measuring something in percent
/// lands on round numbers.
const DEFAULT_STEP: f32 = 0.05;

/// Gap [`Slider`] keeps between its own edge and the track it draws.
///
/// Room for the focus ring to be seen as a ring rather than as a border the
/// knob is pressed up against.
const RING_PAD: f32 = 3.;

/// How near a multiple of the step counts as sitting on it.
///
/// A fraction that arrived by arithmetic is rarely an exact multiple of
/// anything — `0.15` divided by `0.05` is `2.9999998`, not `3` — and without a
/// tolerance the first arrow key after that would snap to the value already
/// showing and appear to do nothing.
const ON_STEP: f32 = 1e-4;

/// Callback fired with the value the slider is moving to.
type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// A value pinned to the range a slider can show.
///
/// A host is free to hand over anything — a fraction it has not clamped yet, a
/// division that came out as `NaN` — and gets a slider that is drawable rather
/// than an argument.
fn fraction(value: f32) -> f32 {
    if value.is_nan() {
        return 0.;
    }
    value.clamp(0., 1.)
}

/// The value a pointer at `x` in window coordinates is asking for, given the
/// `track` it is over and how far into the knob — `grab` — the press landed.
///
/// The knob's travel is the track less the knob's own width, which is precisely
/// what [`dragged_to`] measures a scrollbar thumb against; passing the knob's
/// diameter as the thumb's length is the whole of the adaptation. `None` when
/// the track is too narrow to hold the knob, where there is nowhere to move it
/// and nothing to report.
fn value_at(track: Bounds<Pixels>, x: Pixels, grab: Pixels) -> Option<f32> {
    dragged_to(track.size.width, px(KNOB), x - track.origin.x, grab)
}

/// `value` moved one step, in the direction `up` points.
///
/// Steps land on multiples of `step` rather than on `value + step`: a slider
/// dragged to some fraction of nothing in particular is pulled onto the grid by
/// the first arrow key and stays on it afterwards, which is what makes a
/// keyboard-driven slider able to hit a round number at all. A value already on
/// the grid moves a whole step; one between grid points moves to the next one in
/// that direction, which may be less than a step away.
///
/// Both ends are hard stops. A step that is not a positive, finite number leaves
/// the value where it is, since there is no grid to snap to.
pub fn stepped(value: f32, step: f32, up: bool) -> f32 {
    let value = fraction(value);
    if !step.is_finite() || step <= 0. {
        return value;
    }

    let index = value / step;
    let nearest = index.round();
    let landed = (index - nearest).abs() < ON_STEP;
    let next = if up {
        if landed {
            nearest + 1.
        } else {
            index.floor() + 1.
        }
    } else if landed {
        nearest - 1.
    } else {
        index.ceil() - 1.
    };

    fraction(next * step)
}

/// What a knob drag carries with it.
///
/// Everything a listener needs to answer "where has this gone?" without having
/// seen the press: which slider it is, the track it runs in, and where the
/// pointer took hold. See the module docs for why it travels rather than being
/// read off the event.
pub struct DraggedKnob {
    /// The slider being dragged, so a view with several tells them apart.
    id: ElementId,
    /// The track in window coordinates.
    ///
    /// Shared with the [`canvas`] that measures it, which fills it during the
    /// prepaint of the frame this payload was built in and never touches it
    /// again — so by the time a drag can read it, it is the track as it stood
    /// when the drag began.
    track: Rc<Cell<Bounds<Pixels>>>,
    /// How far into the knob the press landed.
    ///
    /// gpui only offers this number to the closure that builds the drag
    /// preview, so that closure parks it here on the way past. A [`Cell`] rather
    /// than a plain field because the payload is only ever seen through a shared
    /// reference after that.
    grab: Cell<Pixels>,
}

impl DraggedKnob {
    /// The value this drag has reached, if it belongs to the slider `id`.
    pub fn value(&self, id: &ElementId, position: Point<Pixels>) -> Option<f32> {
        if self.id != *id {
            return None;
        }

        value_at(self.track.get(), position.x, self.grab.get())
    }
}

/// A stateless horizontal slider over a fraction from `0.` to `1.`.
///
/// The parent view passes the current value on every render and updates its own
/// state from [`Slider::on_change`], which receives the value the slider is
/// moving to. Values outside the range are clamped rather than refused, so a
/// host is never made to sanitise a number before it can draw.
///
/// `id` has to be unique among every slider that can be dragged at the same
/// time — in practice, within the window — because it is what tells one
/// slider's drag from another's.
///
/// ```ignore
/// let this = cx.entity();
/// Slider::new("volume")
///     .value(self.volume)
///     .step(0.1)
///     .tab_index(3)
///     .on_change(move |value, _window, cx| {
///         this.update(cx, |view, cx| {
///             view.volume = value;
///             cx.notify();
///         });
///     })
/// ```
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    step: f32,
    tab_index: Option<isize>,
    on_change: Option<ChangeHandler>,
}

impl Slider {
    /// Creates a slider at the start of its range.
    ///
    /// `id` must be unique among the sliders of the window; see the type docs
    /// for why the usual "unique among its siblings" is not enough here.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: 0.,
            step: DEFAULT_STEP,
            tab_index: None,
            on_change: None,
        }
    }

    /// Sets where the knob sits, as a fraction from `0.` to `1.`.
    ///
    /// Anything outside that range — or anything that is not a number at all —
    /// is pinned to it for drawing; the host's own value is left alone.
    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// Sets how far one arrow key moves the slider.
    ///
    /// Keyboard steps land on multiples of this, so it also decides which values
    /// a keyboard alone can reach. Defaults to `0.05`. A step that is not
    /// positive and finite disables stepping rather than freezing the arrow keys
    /// on one value.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    /// Places the slider at `index` in the window's tab order.
    ///
    /// A focused slider draws an accent outline. `Left` and `Down` move it one
    /// step towards the start, `Right` and `Up` one step towards the end, and
    /// `Home` and `End` go the whole way in either direction.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets the callback invoked with the value the slider is moving to.
    ///
    /// Fired by all three ways of moving it — a drag, a press on the track and
    /// an arrow key — and never with the value already showing.
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// The value `event` has dragged this slider to, or `None` when the drag
    /// belongs to another slider.
    ///
    /// Only for a host that wants to read the gesture itself: the slider already
    /// listens for its own drags and reports them through
    /// [`Slider::on_change`]. Doing both is harmless — they agree on the value —
    /// and is worth it when a view would rather handle every drag it sees,
    /// scrollbars included, in the one `on_drag_move` listener.
    pub fn dragged(&self, event: &DragMoveEvent<DraggedKnob>, cx: &App) -> Option<f32> {
        event.drag(cx).value(&self.id, event.event.position)
    }
}

impl RenderOnce for Slider {
    /// Four boxes over one measurement.
    ///
    /// The outer box is the control, as tall as the knob and as wide as it is
    /// given, and the [`canvas`] filling it is what every press and drag is
    /// measured against — so the geometry the pointer is read with is the box
    /// the eye sees, with nothing inferred.
    ///
    /// Inside it the groove runs the full width and the rail does not: the rail
    /// is inset by half a knob at each end, which makes it exactly the knob's
    /// travel, and both the knob and the end of the filled part are placed as a
    /// fraction of *it*. That is the whole trick — a percentage against the rail
    /// is a percentage of the travel, so the value can be drawn without knowing
    /// how wide the slider ended up. The filled part reaches back over the
    /// rail's leading edge to the groove's, so it starts where the track starts
    /// and stops under the knob's centre.
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let value = fraction(self.value);
        let step = self.step;
        let id = self.id;
        let on_change = self.on_change;

        // Filled by the canvas below during this frame's prepaint, and read by
        // every press and drag that follows it.
        let track: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

        let measure = {
            let track = track.clone();
            canvas(
                move |bounds, _window, _cx| track.set(bounds),
                |_bounds, _measured, _window, _cx| {},
            )
            .absolute()
            .size_full()
        };

        let groove = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(TRACK_TOP))
            .h(px(TRACK))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface);

        let filled = div()
            .absolute()
            .left(px(-HALF_KNOB))
            .right(relative(1. - value))
            .top(px(TRACK_TOP))
            .h(px(TRACK))
            .rounded_full()
            .bg(theme.accent);

        let knob = div()
            .id(ElementId::from((id.clone(), "knob")))
            .absolute()
            .left(px(-HALF_KNOB))
            .top_0()
            .size(px(KNOB))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .cursor_pointer()
            // Presses on the knob belong to the knob: without this the track
            // beneath would read them as a jump to wherever the knob already is.
            .occlude()
            .hover(|style| style.border_color(theme.accent))
            // An empty preview: the knob follows the pointer directly, so a
            // ghost trailing it would only be a second thing to watch.
            .on_drag(
                DraggedKnob {
                    id: id.clone(),
                    track: track.clone(),
                    grab: Cell::new(px(0.)),
                },
                |dragged, grab, _window, cx| {
                    dragged.grab.set(grab.x);
                    cx.new(|_| gpui::Empty)
                },
            );

        // Zero width, so the knob hung off it can be centred on the fraction
        // with a plain offset rather than by mixing a percentage and a pixel
        // count in one length.
        let anchor = div()
            .absolute()
            .left(relative(value))
            .top_0()
            .bottom_0()
            .w_0()
            .child(knob);

        let rail = div()
            .absolute()
            .left(px(HALF_KNOB))
            .right(px(HALF_KNOB))
            .top_0()
            .bottom_0()
            .child(filled)
            .child(anchor);

        let pressed = {
            let track = track.clone();
            let on_change = on_change.clone();
            move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                // Half a knob, because a press on the track asks for the knob's
                // centre to come to the pointer rather than its leading edge.
                if let Some(next) = value_at(track.get(), event.position.x, px(HALF_KNOB))
                    && next != value
                {
                    handler(next, window, cx);
                }
            }
        };

        let bar = div()
            .relative()
            .w_full()
            .h(px(KNOB))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, pressed)
            .child(measure)
            .child(groove)
            .child(rail);

        div()
            .id(id.clone())
            .w_full()
            .p(px(RING_PAD))
            .rounded_sm()
            // Transparent until focused, so the ring costs no layout.
            .border_1()
            .border_color(transparent_black())
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                let on_key = on_change.clone();
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.modifiers.modified() {
                            return;
                        }
                        let next = match event.keystroke.key.as_str() {
                            "left" | "down" => stepped(value, step, false),
                            "right" | "up" => stepped(value, step, true),
                            "home" => 0.,
                            "end" => 1.,
                            _ => return,
                        };
                        // Stopped either way: the slider owns those keys while
                        // it holds focus, whether or not the press moved it.
                        cx.stop_propagation();
                        if let Some(handler) = on_key.as_ref()
                            && next != value
                        {
                            handler(next, window, cx);
                        }
                    })
            })
            // The slider hears its own drags, so a host that only wants a value
            // back has nothing to wire up. gpui offers a drag move to every
            // listener of that payload type, hovered or not, which is what lets
            // a gesture that has wandered off the knob still arrive here.
            .on_drag_move(move |event: &DragMoveEvent<DraggedKnob>, window, cx| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                if let Some(next) = event.drag(cx).value(&id, event.event.position)
                    && next != value
                {
                    handler(next, window, cx);
                }
            })
            .child(bar)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use gpui::{
        Context, Modifiers, MouseUpEvent, Render, TestAppContext, VisualTestContext, point, size,
    };

    use super::*;

    /// How far the knob can travel in the windowed harness below.
    ///
    /// A round number, so that a column is a value and a value is a column with
    /// nothing to work out on the way.
    const TRAVEL: f32 = 200.;

    /// Width the harness gives the slider.
    ///
    /// The travel, plus the knob that has to fit at either end of it, plus what
    /// the slider keeps clear on both sides — everything between the slider's
    /// own edge and the track's.
    const HARNESS_WIDTH: f32 = TRAVEL + KNOB + 2. * INSET;

    /// How far in from the slider's edge its track starts: the ring's padding,
    /// and the one pixel of border the ring is drawn as.
    const INSET: f32 = RING_PAD + 1.;

    /// Element id of the harness's slider.
    const SLIDER: &str = "slider";

    /// Vertical middle of the track, which is that same inset plus half the
    /// control's height.
    const MIDDLE: f32 = INSET + HALF_KNOB;

    /// Floats that came through a division are compared with a tolerance, since
    /// none of these values are exactly representable.
    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-5
    }

    /// A track `width` wide, `offset` from the window's left edge.
    fn track(offset: f32, width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(offset), px(0.)), size(px(width), px(KNOB)))
    }

    /// A slider in a window, with everything a test needs to press it and read
    /// back what it answered.
    ///
    /// The point of running one for real is that the geometry above is only half
    /// the story: the other half is whether the track the [`canvas`] measures is
    /// the box the value is drawn against, and whether the knob takes the
    /// presses that land on it. Neither can be seen without a layout pass.
    struct Harness {
        value: Rc<Cell<f32>>,
        /// How many times the slider has asked to move, so that "it did not
        /// move" can be told from "it moved back to where it was".
        changes: Rc<Cell<usize>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            div().size_full().child(
                div().w(px(HARNESS_WIDTH)).child(
                    Slider::new(SLIDER)
                        .value(self.value.get())
                        .step(0.1)
                        .tab_index(0)
                        .on_change(move |value, _window, cx| {
                            this.update(cx, |harness, cx| {
                                harness.value.set(value);
                                harness.changes.set(harness.changes.get() + 1);
                                cx.notify();
                            });
                        }),
                ),
            )
        }
    }

    /// Opens a window on a slider showing `value`.
    fn open(
        value: f32,
        cx: &mut TestAppContext,
    ) -> (Rc<Cell<f32>>, Rc<Cell<usize>>, VisualTestContext) {
        cx.update(crate::init);

        let showing = Rc::new(Cell::new(value));
        let changes = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let showing = showing.clone();
            let changes = changes.clone();
            move |_, _| Harness {
                value: showing,
                changes,
            }
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();

        (showing, changes, cx)
    }

    /// The column a slider showing `value` puts the knob's centre at.
    fn column(value: f32) -> f32 {
        INSET + HALF_KNOB + value * TRAVEL
    }

    /// Presses and releases the left button over a column of the track.
    fn press(cx: &mut VisualTestContext, x: f32) {
        let position = point(px(x), px(MIDDLE));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        cx.run_until_parked();
    }

    /// A press on the bare track brings the knob's centre to the pointer, and
    /// the two ends of the track are the two ends of the range — which is the
    /// whole of "the box the canvas measured is the box the value is drawn
    /// against".
    #[gpui::test]
    fn a_press_on_the_track_moves_the_knob_to_it(cx: &mut TestAppContext) {
        let (value, _, mut cx) = open(0., cx);

        press(&mut cx, column(0.5));
        assert!(close(value.get(), 0.5), "{}", value.get());

        press(&mut cx, column(1.));
        assert!(close(value.get(), 1.), "{}", value.get());

        press(&mut cx, column(0.));
        assert!(close(value.get(), 0.), "{}", value.get());

        // The last of the track is past where the knob's centre can reach, and
        // a press there stops at the end rather than running on.
        press(&mut cx, column(1.) + HALF_KNOB - 1.);
        assert!(close(value.get(), 1.), "{}", value.get());
    }

    /// A press on the knob belongs to the knob: it is where a drag begins, and
    /// reading it as a jump would shift the knob by however far from its centre
    /// the hand landed.
    #[gpui::test]
    fn a_press_on_the_knob_moves_nothing(cx: &mut TestAppContext) {
        let (value, changes, mut cx) = open(0.5, cx);

        // Inside the knob, which reaches half its width either side of the
        // centre, but not on its centre — so a slider that mistook this for a
        // press on the track would visibly move.
        press(&mut cx, column(0.5) + HALF_KNOB - 2.);
        assert_eq!(
            changes.get(),
            0,
            "the knob let a press through to the track"
        );
        assert!(close(value.get(), 0.5), "{}", value.get());

        // Just past the knob's edge is the track again, and that one does move.
        press(&mut cx, column(0.5) + HALF_KNOB + 2.);
        assert_eq!(changes.get(), 1);
        assert!(close(value.get(), 0.5 + 9. / TRAVEL), "{}", value.get());
    }

    /// The whole drag path, which nothing short of a window exercises: the
    /// track the canvas measured reaches the payload, the point the knob was
    /// taken hold of stays under the pointer, and the slider hears its own drag
    /// without the host having wired anything up.
    #[gpui::test]
    fn dragging_the_knob_follows_the_pointer(cx: &mut TestAppContext) {
        let (value, _, mut cx) = open(0.25, cx);
        let knob_left = INSET + 0.25 * TRAVEL;

        // Down two pixels short of the knob's centre, then three along — just
        // past the two gpui tells a drag from a click by. gpui reads the grab
        // off the pointer at the moment it decides a drag has begun rather than
        // at the press, so the point held is that press plus those three, and a
        // slider that ignored the grab would put the knob's start there instead.
        let press = point(px(column(0.25) - 2.), px(MIDDLE));
        cx.simulate_mouse_move(press, None, Modifiers::none());
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(press.x + px(3.), press.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();
        let grab = f32::from(press.x) + 3. - knob_left;

        let along = point(px(INSET + grab + 0.6 * TRAVEL), press.y);
        cx.simulate_mouse_move(along, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        assert!(close(value.get(), 0.6), "{}", value.get());

        // Dragged clean off the end of the track, it stops at the end rather
        // than running on — and it is still listening out there, well outside
        // anything it drew.
        let past = point(px(column(1.) + 400.), press.y);
        cx.simulate_mouse_move(past, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(past, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        assert!(close(value.get(), 1.), "{}", value.get());
    }

    /// The arrow keys step a focused slider, and `Home` and `End` go the whole
    /// way — the part of the keyboard contract that only exists once something
    /// has focus to give.
    #[gpui::test]
    fn the_arrow_keys_step_a_focused_slider(cx: &mut TestAppContext) {
        let (value, changes, mut cx) = open(0.5, cx);
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();

        cx.simulate_keystrokes("right");
        assert!(close(value.get(), 0.6), "{}", value.get());

        cx.simulate_keystrokes("down");
        assert!(close(value.get(), 0.5), "{}", value.get());

        cx.simulate_keystrokes("end");
        assert!(close(value.get(), 1.), "{}", value.get());

        // Already at the end: nothing to report, so nothing is reported.
        let asked = changes.get();
        cx.simulate_keystrokes("up");
        assert_eq!(changes.get(), asked, "a slider at the end moved anyway");

        cx.simulate_keystrokes("home");
        assert!(close(value.get(), 0.), "{}", value.get());
    }

    /// Whatever the host has is drawable: out of range is pinned, and a value
    /// that is not a number at all starts the slider at the beginning rather
    /// than poisoning every length computed from it.
    #[test]
    fn a_value_outside_the_range_is_pinned_to_it() {
        assert_eq!(fraction(-3.), 0.);
        assert_eq!(fraction(0.25), 0.25);
        assert_eq!(fraction(9.), 1.);
        assert_eq!(fraction(f32::NAN), 0.);
        assert_eq!(fraction(f32::INFINITY), 1.);
    }

    /// The ends line up: grabbing the knob by its centre and holding it over
    /// either end of the track puts the slider at that end, and over the middle
    /// puts it halfway.
    #[test]
    fn the_knob_reaches_both_ends_of_its_track() {
        let track = track(0., 114.);

        assert_eq!(value_at(track, px(HALF_KNOB), px(HALF_KNOB)), Some(0.));
        assert_eq!(
            value_at(track, px(114. - HALF_KNOB), px(HALF_KNOB)),
            Some(1.)
        );
        assert_eq!(value_at(track, px(57.), px(HALF_KNOB)), Some(0.5));
    }

    /// The pointer is read against the track's own corner rather than the
    /// window's, so a slider halfway down a form is no different from one at the
    /// origin.
    #[test]
    fn the_pointer_is_read_from_the_tracks_corner() {
        let track = track(40., 114.);

        assert_eq!(value_at(track, px(40. + 57.), px(HALF_KNOB)), Some(0.5));
    }

    /// The point taken hold of stays under the pointer: a press near the knob's
    /// trailing edge does not jump the knob forward under it.
    #[test]
    fn a_drag_keeps_the_grabbed_point_under_the_pointer() {
        let track = track(0., 114.);

        // Grabbed at its far edge and held at 100: the knob's start is at 86,
        // which is 86/100 of the travel.
        let value = value_at(track, px(100.), px(KNOB)).expect("a knob with room to travel");
        assert!(close(value, 0.86), "{value}");
    }

    /// Dragged past either end, the slider stops at that end rather than
    /// running on.
    #[test]
    fn a_drag_past_either_end_stops_there() {
        let track = track(0., 114.);

        assert_eq!(value_at(track, px(9_000.), px(HALF_KNOB)), Some(1.));
        assert_eq!(value_at(track, px(-9_000.), px(HALF_KNOB)), Some(0.));
    }

    /// A slider too narrow to hold its own knob has nowhere to move it, and says
    /// so rather than dividing by zero — which is what the first frame of a
    /// slider looks like, before anything has been laid out.
    #[test]
    fn a_track_with_no_room_reports_nothing() {
        assert_eq!(value_at(track(0., 0.), px(5.), px(HALF_KNOB)), None);
        assert_eq!(value_at(track(0., KNOB), px(5.), px(HALF_KNOB)), None);
        assert_eq!(
            value_at(Bounds::default(), px(5.), px(HALF_KNOB)),
            None,
            "an unmeasured slider answered a press"
        );
    }

    /// A drag only answers to the slider it started on. Two sliders in one view
    /// see each other's moves, and must ignore them.
    #[test]
    fn a_drag_answers_only_to_its_own_slider() {
        let dragged = DraggedKnob {
            id: "mine".into(),
            track: Rc::new(Cell::new(track(0., 114.))),
            grab: Cell::new(px(HALF_KNOB)),
        };

        assert_eq!(
            dragged.value(&"mine".into(), point(px(57.), px(0.))),
            Some(0.5)
        );
        assert_eq!(
            dragged.value(&"theirs".into(), point(px(57.), px(0.))),
            None
        );
    }

    /// A value already on the step's grid moves a whole step, in either
    /// direction.
    #[test]
    fn a_step_from_the_grid_lands_on_the_next_grid_point() {
        assert!(close(stepped(0.5, 0.1, true), 0.6));
        assert!(close(stepped(0.5, 0.1, false), 0.4));

        // 0.15 / 0.05 is 2.9999998 rather than 3, and without a tolerance the
        // step up would snap back onto the value already showing.
        let up = stepped(0.15, 0.05, true);
        assert!(close(up, 0.2), "{up}");
        let down = stepped(0.15, 0.05, false);
        assert!(close(down, 0.1), "{down}");
    }

    /// A value between grid points is pulled onto the grid, which may be less
    /// than a whole step away — the point of stepping at all.
    #[test]
    fn a_step_from_between_grid_points_snaps_to_the_grid() {
        let up = stepped(0.42, 0.05, true);
        assert!(close(up, 0.45), "{up}");

        let down = stepped(0.42, 0.05, false);
        assert!(close(down, 0.4), "{down}");
    }

    /// Both ends are hard stops, and a step that overshoots one stops there
    /// rather than wrapping.
    #[test]
    fn stepping_stops_at_either_end() {
        assert_eq!(stepped(1., 0.05, true), 1.);
        assert_eq!(stepped(0., 0.05, false), 0.);
        assert_eq!(stepped(0.98, 0.3, true), 1.);
        assert_eq!(stepped(0.02, 0.3, false), 0.);

        // And a value the host never clamped is pinned before it is stepped.
        assert_eq!(stepped(4., 0.05, true), 1.);
        assert!(close(stepped(-4., 0.05, true), 0.05));
    }

    /// A step that is not a positive, finite number leaves the slider alone
    /// rather than freezing it on one value or filling it with `NaN`.
    #[test]
    fn a_nonsense_step_moves_nothing() {
        assert_eq!(stepped(0.4, 0., true), 0.4);
        assert_eq!(stepped(0.4, -0.1, false), 0.4);
        assert_eq!(stepped(0.4, f32::NAN, true), 0.4);
        assert_eq!(stepped(0.4, f32::INFINITY, true), 0.4);
    }
}
