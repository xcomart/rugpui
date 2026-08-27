//! A horizontal slider over an interval, with a knob at each end.
//!
//! Two knobs on one track and two numbers between `0.` and `1.` to say where
//! they sit. Like [`crate::slider`] it knows nothing of what those numbers
//! mean — a price band, a date window, a pair of gain limits are all the same
//! control — and turning the pair of fractions into whatever the host actually
//! stores is the host's business.
//!
//! Also like the slider it is stateless: the owning view passes `low` and
//! `high` on every render and updates its own state from
//! [`RangeSlider::on_change`], which is handed both numbers rather than the one
//! that moved. A host that stores an interval stores an interval, and never has
//! to work out which end it is being told about.
//!
//! Everything under [the slider's "how a drag finds its way
//! home"](crate::slider#how-a-drag-finds-its-way-home) applies here unchanged,
//! down to the payload: a knob carries a [`DraggedKnob`] holding *its own*
//! sub-id — `(id, "low")` or `(id, "high")` — so the one payload type tells the
//! two knobs of one range apart with the same comparison it uses to tell two
//! sliders apart.
//!
//! ## What the second knob adds
//!
//! * Neither knob may pass the other. A knob dragged into its neighbour stops
//!   there, and the two are allowed to meet: `low == high` is a legal, if
//!   empty, interval.
//! * A press on the bare track moves whichever knob is *nearer* to it, since
//!   there is no longer one obvious answer to "the knob comes here".
//! * The two knobs are two tab stops, and the arrow keys move the one that
//!   holds focus. So the ring is drawn around a knob rather than around the
//!   whole control.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, DragMoveEvent, ElementId, MouseButton, MouseDownEvent, Pixels, Window, canvas,
    div, prelude::*, px, relative, transparent_black,
};

use crate::slider::{
    DEFAULT_STEP, DraggedKnob, HALF_KNOB, KNOB, RING_PAD, TRACK, TRACK_TOP, fraction, stepped,
    value_at,
};
use crate::theme::theme;

/// How far outside a knob its focus ring is drawn, in pixels.
///
/// The same distance [`RangeSlider`] keeps between its own edge and the track —
/// [`RING_PAD`] plus the pixel the ring is drawn as — so a ring around a knob at
/// either end of the track lands exactly on the control's own edge instead of
/// spilling past it.
const KNOB_RING: f32 = RING_PAD + 1.;

/// Callback fired with the interval the range slider is moving to.
type ChangeHandler = Rc<dyn Fn(f32, f32, &mut Window, &mut App)>;

/// One end of the interval.
///
/// Only interesting to a host reading a drag itself through
/// [`RangeSlider::dragged`]; the callback reports both ends and never needs to
/// name one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knob {
    /// The knob at the start of the interval.
    Low,
    /// The knob at the end of it.
    High,
}

/// The interval that results from putting `knob` at `at`.
///
/// The moving knob stops at its neighbour rather than passing it, which is the
/// one rule that makes a pair of knobs a range: an interval whose ends have
/// crossed is not an interval. They are allowed to *meet*, because an empty
/// selection is a thing a user may well mean.
///
/// `low` and `high` must already be ordered, which is what the drawing pass
/// guarantees.
fn placed(low: f32, high: f32, knob: Knob, at: f32) -> (f32, f32) {
    match knob {
        Knob::Low => (fraction(at).min(high), high),
        Knob::High => (low, fraction(at).max(low)),
    }
}

/// Which knob a press at `at` is asking to move: the nearer of the two, and the
/// high one when they are equally far off.
///
/// A tie is not the corner case it looks like — it is every press exactly
/// halfway between the knobs, and both answers are as good as each other. It
/// only has to be decided the same way every time, or a press on that column
/// would move one knob or the other depending on rounding.
fn nearer(low: f32, high: f32, at: f32) -> Knob {
    if (at - low).abs() < (at - high).abs() {
        Knob::Low
    } else {
        Knob::High
    }
}

/// A stateless horizontal slider over an interval `low..high` within `0.` to
/// `1.`.
///
/// The parent view passes both ends on every render and updates its own state
/// from [`RangeSlider::on_change`], which receives the pair the control is
/// moving to. Values outside the range are clamped and a `low` above its `high`
/// is pinned down to it, so a host is never made to sanitise a pair before it
/// can draw.
///
/// `id` has to be unique among every range slider and [`crate::Slider`] that can
/// be dragged at the same time — in practice, within the window — because it is
/// what tells one control's drag from another's.
///
/// ```ignore
/// let this = cx.entity();
/// RangeSlider::new("band")
///     .low(self.band.0)
///     .high(self.band.1)
///     .step(0.05)
///     .tab_index(3)
///     .on_change(move |low, high, _window, cx| {
///         this.update(cx, |view, cx| {
///             view.band = (low, high);
///             cx.notify();
///         });
///     })
/// ```
#[derive(IntoElement)]
pub struct RangeSlider {
    id: ElementId,
    low: f32,
    high: f32,
    step: f32,
    tab_index: Option<isize>,
    on_change: Option<ChangeHandler>,
}

impl RangeSlider {
    /// Creates a range slider selecting the whole range.
    ///
    /// `id` must be unique among the sliders of the window; see the type docs
    /// for why the usual "unique among its siblings" is not enough here.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            low: 0.,
            high: 1.,
            step: DEFAULT_STEP,
            tab_index: None,
            on_change: None,
        }
    }

    /// Sets where the interval starts, as a fraction from `0.` to `1.`.
    ///
    /// Anything outside that range — or anything that is not a number at all —
    /// is pinned to it for drawing, and a `low` above the `high` is drawn *at*
    /// the high knob. The host's own values are left alone until something
    /// moves; the pair the callback then reports is the ordered one.
    pub fn low(mut self, low: f32) -> Self {
        self.low = low;
        self
    }

    /// Sets where the interval ends, as a fraction from `0.` to `1.`.
    ///
    /// Clamped for drawing exactly as [`RangeSlider::low`] is.
    pub fn high(mut self, high: f32) -> Self {
        self.high = high;
        self
    }

    /// Sets how far one arrow key moves the focused knob.
    ///
    /// Keyboard steps land on multiples of this, so it also decides which values
    /// a keyboard alone can reach. Defaults to `0.05`. A step that is not
    /// positive and finite disables stepping rather than freezing the arrow keys
    /// on one value.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    /// Places the two knobs at `index` and `index + 1` in the window's tab
    /// order.
    ///
    /// Two knobs are two tab stops, so a range slider takes two indices where
    /// every other widget in the kit takes one — worth knowing when numbering
    /// the rest of a form.
    ///
    /// The focused knob draws an accent ring and is the only one the keys move:
    /// `Left` and `Down` step it towards the start, `Right` and `Up` towards the
    /// end, and `Home` and `End` send it the whole way — as far as the other
    /// knob, which is where "the whole way" ends for a knob that may not pass
    /// its neighbour.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets the callback invoked with the interval the slider is moving to.
    ///
    /// Fired by all three ways of moving a knob — a drag, a press on the track
    /// and an arrow key — with both ends every time, and never with the interval
    /// already showing.
    pub fn on_change(
        mut self,
        handler: impl Fn(f32, f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Which knob `event` is dragging and where it has reached, or `None` when
    /// the drag belongs to another control.
    ///
    /// Only for a host that wants to read the gesture itself: the slider already
    /// listens for its own drags and reports them through
    /// [`RangeSlider::on_change`]. Doing both is harmless — the value here is
    /// the one the callback would be given for that knob, stopped at its
    /// neighbour and all.
    pub fn dragged(&self, event: &DragMoveEvent<DraggedKnob>, cx: &App) -> Option<(Knob, f32)> {
        let high = fraction(self.high);
        let low = fraction(self.low).min(high);
        let dragged = event.drag(cx);
        let position = event.event.position;

        for (knob, tail) in [(Knob::Low, "low"), (Knob::High, "high")] {
            let id = ElementId::from((self.id.clone(), tail));
            if let Some(at) = dragged.value(&id, position) {
                let (next_low, next_high) = placed(low, high, knob, at);
                return Some(match knob {
                    Knob::Low => (knob, next_low),
                    Knob::High => (knob, next_high),
                });
            }
        }
        None
    }
}

impl RenderOnce for RangeSlider {
    /// The slider's four boxes with a second knob hung off the same rail.
    ///
    /// The outer box is the control and the [`canvas`] filling it is what every
    /// press and drag is measured against; the groove runs its full width and
    /// the rail is inset by half a knob at each end, making the rail exactly a
    /// knob's travel so that a percentage against it is a percentage of the
    /// value. See [`crate::slider`] for the whole of that arrangement.
    ///
    /// What differs is the filled part. A plain slider fills from the start of
    /// the track to the knob; this one fills between the two knob centres, which
    /// is why it needs no reach back over the rail's leading edge — both of its
    /// ends are values, and both are placed as percentages of the rail.
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let accent = theme.accent;
        let border = theme.border;
        let background = theme.background;

        // Drawn ordered even when the host's pair is not, so that a state that
        // is momentarily inside out — a text field being typed into, a load that
        // has half arrived — is a picture rather than a panic.
        let high = fraction(self.high);
        let low = fraction(self.low).min(high);
        let step = self.step;
        let tab_index = self.tab_index;
        let id = self.id;
        let low_id = ElementId::from((id.clone(), "low"));
        let high_id = ElementId::from((id.clone(), "high"));
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
            .border_color(border)
            .bg(theme.surface);

        // Between the two knob centres, which are both fractions of the rail.
        let filled = div()
            .absolute()
            .left(relative(low))
            .right(relative(1. - high))
            .top(px(TRACK_TOP))
            .h(px(TRACK))
            .rounded_full()
            .bg(accent);

        // One knob, its ring and its anchor. Written once and called twice
        // because the two differ only in which number they draw and which end
        // of the interval their keys are allowed to reach.
        let knob_at = |which: Knob| {
            let (value, tail, ring_tail, index) = match which {
                Knob::Low => (low, "low", "low-ring", tab_index),
                Knob::High => (high, "high", "high-ring", tab_index.map(|index| index + 1)),
            };

            let knob = div()
                .id(ElementId::from((id.clone(), tail)))
                .absolute()
                // Inside the ring's border, which is the one pixel between the
                // ring's own box and the box its children are placed in.
                .left(px(KNOB_RING - 1.))
                .top(px(KNOB_RING - 1.))
                .size(px(KNOB))
                .rounded_full()
                .border_1()
                .border_color(border)
                .bg(background)
                .cursor_pointer()
                // Presses on a knob belong to the knob: without this the track
                // beneath would read them as a jump to wherever it already is.
                .occlude()
                .hover(|style| style.border_color(accent))
                // An empty preview: the knob follows the pointer directly, so a
                // ghost trailing it would only be a second thing to watch.
                .on_drag(
                    DraggedKnob {
                        id: ElementId::from((id.clone(), tail)),
                        track: track.clone(),
                        grab: Cell::new(px(0.)),
                    },
                    |dragged, grab, _window, cx| {
                        dragged.grab.set(grab.x);
                        cx.new(|_| gpui::Empty)
                    },
                );

            // The ring is a knob's tab stop as well as its focus ring: two knobs
            // are two places the keyboard can be, so what is focusable here is
            // half the control rather than all of it. It does not occlude, so
            // the track still hears a press that lands in the ring's margin.
            let ring = div()
                .id(ElementId::from((id.clone(), ring_tail)))
                .absolute()
                .left(px(-(HALF_KNOB + KNOB_RING)))
                .top(px(-KNOB_RING))
                .size(px(KNOB + 2. * KNOB_RING))
                .rounded_full()
                // Transparent until focused, so the ring costs no layout.
                .border_1()
                .border_color(transparent_black())
                .when_some(index, |this, index| {
                    let on_key = on_change.clone();
                    this.tab_index(index)
                        .focus(move |style| style.border_color(accent))
                        .on_key_down(move |event, window, cx| {
                            if event.keystroke.modifiers.modified() {
                                return;
                            }
                            let at = match event.keystroke.key.as_str() {
                                "left" | "down" => stepped(value, step, false),
                                "right" | "up" => stepped(value, step, true),
                                // The whole way is as far as the other knob.
                                "home" => match which {
                                    Knob::Low => 0.,
                                    Knob::High => low,
                                },
                                "end" => match which {
                                    Knob::Low => high,
                                    Knob::High => 1.,
                                },
                                _ => return,
                            };
                            // Stopped either way: the knob owns those keys while
                            // it holds focus, whether or not the press moved it.
                            cx.stop_propagation();
                            let next = placed(low, high, which, at);
                            if let Some(handler) = on_key.as_ref()
                                && next != (low, high)
                            {
                                handler(next.0, next.1, window, cx);
                            }
                        })
                })
                .child(knob);

            // Zero width, so the ring hung off it can be centred on the fraction
            // with a plain offset rather than by mixing a percentage and a pixel
            // count in one length.
            div()
                .absolute()
                .left(relative(value))
                .top_0()
                .bottom_0()
                .w_0()
                .child(ring)
        };

        // Where the two sit on top of each other only the one drawn last takes
        // the press, so the one with somewhere left to go is drawn last: at the
        // very end of the track that is the low knob, since the high one cannot
        // move up any further, and everywhere else it is the high knob.
        let (under, over) = if high >= 1. {
            (knob_at(Knob::High), knob_at(Knob::Low))
        } else {
            (knob_at(Knob::Low), knob_at(Knob::High))
        };

        let rail = div()
            .absolute()
            .left(px(HALF_KNOB))
            .right(px(HALF_KNOB))
            .top_0()
            .bottom_0()
            .child(filled)
            .child(under)
            .child(over);

        let pressed = {
            let track = track.clone();
            let on_change = on_change.clone();
            move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                // Half a knob, because a press on the track asks for a knob's
                // centre to come to the pointer rather than its leading edge.
                if let Some(at) = value_at(track.get(), event.position.x, px(HALF_KNOB)) {
                    let next = placed(low, high, nearer(low, high, at), at);
                    if next != (low, high) {
                        handler(next.0, next.1, window, cx);
                    }
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
            // No ring of its own — the knobs draw theirs — but the same border
            // the slider reserves one with, so both controls inset their track
            // by the same amount and line up in a column.
            .border_1()
            .border_color(transparent_black())
            // The slider hears its own drags, so a host that only wants an
            // interval back has nothing to wire up. gpui offers a drag move to
            // every listener of that payload type, hovered or not, which is what
            // lets a gesture that has wandered off a knob still arrive here.
            .on_drag_move(move |event: &DragMoveEvent<DraggedKnob>, window, cx| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                let dragged = event.drag(cx);
                let position = event.event.position;
                let moved = if let Some(at) = dragged.value(&low_id, position) {
                    placed(low, high, Knob::Low, at)
                } else if let Some(at) = dragged.value(&high_id, position) {
                    placed(low, high, Knob::High, at)
                } else {
                    return;
                };
                if moved != (low, high) {
                    handler(moved.0, moved.1, window, cx);
                }
            })
            .child(bar)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use gpui::{
        Context, Modifiers, MouseUpEvent, Point, Render, TestAppContext, VisualTestContext, point,
        px,
    };

    use super::*;

    /// How far a knob can travel in the windowed harness below.
    ///
    /// A round number, so that a column is a value and a value is a column with
    /// nothing to work out on the way.
    const TRAVEL: f32 = 200.;

    /// How far in from the control's edge its track starts: the ring's padding,
    /// and the one pixel of border the ring is drawn as.
    const INSET: f32 = RING_PAD + 1.;

    /// Width the harness gives the slider: the travel, plus the knob that has to
    /// fit at either end of it, plus what the control keeps clear on both sides.
    const HARNESS_WIDTH: f32 = TRAVEL + KNOB + 2. * INSET;

    /// Vertical middle of the track, which is that same inset plus half the
    /// control's height.
    const MIDDLE: f32 = INSET + HALF_KNOB;

    /// Element id of the harness's slider.
    const RANGE: &str = "range";

    /// Floats that came through a division are compared with a tolerance, since
    /// none of these values are exactly representable.
    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-5
    }

    /// A range slider in a window, with everything a test needs to press it and
    /// read back what it answered.
    ///
    /// The point of running one for real is that the geometry is only half the
    /// story: the other half is whether the track the [`canvas`] measured is the
    /// box the values are drawn against, whether each knob takes the presses
    /// that land on it, and whether a drag payload built by one knob is told
    /// apart from the other's — none of which can be seen without a layout pass.
    struct Harness {
        range: Rc<Cell<(f32, f32)>>,
        /// How many times the slider has asked to move, so that "it did not
        /// move" can be told from "it moved back to where it was".
        changes: Rc<Cell<usize>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let (low, high) = self.range.get();
            div().size_full().child(
                div().w(px(HARNESS_WIDTH)).child(
                    RangeSlider::new(RANGE)
                        .low(low)
                        .high(high)
                        .step(0.1)
                        .tab_index(0)
                        .on_change(move |low, high, _window, cx| {
                            this.update(cx, |harness, cx| {
                                harness.range.set((low, high));
                                harness.changes.set(harness.changes.get() + 1);
                                cx.notify();
                            });
                        }),
                ),
            )
        }
    }

    /// What [`open`] hands back: the interval the harness is showing, how many
    /// times it has been asked to move it, and the window to press.
    type Opened = (Rc<Cell<(f32, f32)>>, Rc<Cell<usize>>, VisualTestContext);

    /// Opens a window on a range slider showing `low` and `high`.
    fn open(low: f32, high: f32, cx: &mut TestAppContext) -> Opened {
        cx.update(crate::init);

        let showing = Rc::new(Cell::new((low, high)));
        let changes = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let showing = showing.clone();
            let changes = changes.clone();
            move |_, _| Harness {
                range: showing,
                changes,
            }
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();

        (showing, changes, cx)
    }

    /// The column a knob showing `value` has its centre at.
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

    /// Takes hold of the knob whose centre is at `value` and returns where the
    /// pointer ended up, along with how far into the knob it is holding.
    ///
    /// gpui reads the grab off the pointer at the moment it decides a drag has
    /// begun rather than at the press, so the press is followed by the three
    /// pixels that decide it — two is the threshold — and the point held is the
    /// press plus those.
    fn grab(cx: &mut VisualTestContext, value: f32, nudge: f32) -> (Point<Pixels>, f32) {
        let at = point(px(column(value)), px(MIDDLE));
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        let moved = point(at.x + px(nudge), at.y);
        cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();

        (moved, HALF_KNOB + nudge)
    }

    /// Drags a knob already taken hold of to `value` and lets go.
    fn drop_at(cx: &mut VisualTestContext, held: Point<Pixels>, grabbed: f32, value: f32) {
        let to = point(px(INSET + grabbed + value * TRAVEL), held.y);
        cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
    }

    /// A press on the bare track moves whichever knob is nearer to it — the
    /// whole of "which knob did you mean" — and a press exactly between them
    /// moves the high one.
    #[gpui::test]
    fn a_press_on_the_track_moves_the_nearer_knob(cx: &mut TestAppContext) {
        let (range, _, mut cx) = open(0.25, 0.75, cx);

        // Equally far from both, which is a real column rather than a corner
        // case, and is settled the same way every time.
        press(&mut cx, column(0.5));
        let (low, high) = range.get();
        assert!(close(low, 0.25), "{low}");
        assert!(close(high, 0.5), "{high}");

        press(&mut cx, column(0.3));
        let (low, high) = range.get();
        assert!(close(low, 0.3), "{low}");
        assert!(close(high, 0.5), "{high}");

        press(&mut cx, column(0.9));
        let (low, high) = range.get();
        assert!(close(low, 0.3), "{low}");
        assert!(close(high, 0.9), "{high}");
    }

    /// The low knob stops at the high one rather than passing it, and the two
    /// are allowed to meet.
    #[gpui::test]
    fn dragging_the_low_knob_stops_at_the_high_one(cx: &mut TestAppContext) {
        let (range, _, mut cx) = open(0.3, 0.6, cx);

        let (held, grabbed) = grab(&mut cx, 0.3, 3.);
        drop_at(&mut cx, held, grabbed, 0.45);
        let (low, high) = range.get();
        assert!(close(low, 0.45), "{low}");
        assert!(close(high, 0.6), "{high}");

        // Dragged well past its neighbour, it stops on it.
        let (held, grabbed) = grab(&mut cx, 0.45, 3.);
        drop_at(&mut cx, held, grabbed, 0.95);
        let (low, high) = range.get();
        assert!(close(low, 0.6), "{low}");
        assert!(close(high, 0.6), "{high}");
    }

    /// And the high knob stops at the low one, coming the other way.
    #[gpui::test]
    fn dragging_the_high_knob_stops_at_the_low_one(cx: &mut TestAppContext) {
        let (range, _, mut cx) = open(0.3, 0.6, cx);

        let (held, grabbed) = grab(&mut cx, 0.6, -3.);
        drop_at(&mut cx, held, grabbed, 0.05);
        let (low, high) = range.get();
        assert!(close(low, 0.3), "{low}");
        assert!(close(high, 0.3), "{high}");
    }

    /// The arrow keys move the knob that holds focus and leave the other where
    /// it is — the part of the keyboard contract a second knob adds, and the
    /// reason the tab stop is a knob rather than the control.
    #[gpui::test]
    fn the_arrow_keys_step_the_focused_knob(cx: &mut TestAppContext) {
        let (range, changes, mut cx) = open(0.3, 0.6, cx);
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();

        cx.simulate_keystrokes("right");
        let (low, high) = range.get();
        assert!(close(low, 0.4), "{low}");
        assert!(close(high, 0.6), "the other knob moved: {high}");

        cx.simulate_keystrokes("down");
        assert!(close(range.get().0, 0.3), "{}", range.get().0);

        // `End` for the low knob is as far as the high one, and a knob already
        // pressed up against its neighbour has nothing to report.
        cx.simulate_keystrokes("end");
        assert!(close(range.get().0, 0.6), "{}", range.get().0);
        let asked = changes.get();
        cx.simulate_keystrokes("up");
        assert_eq!(changes.get(), asked, "a knob at its neighbour moved anyway");

        // The second tab stop is the other knob.
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("right");
        let (low, high) = range.get();
        assert!(close(low, 0.6), "the other knob moved: {low}");
        assert!(close(high, 0.7), "{high}");

        cx.simulate_keystrokes("end");
        assert!(close(range.get().1, 1.), "{}", range.get().1);
    }

    /// A pair the host has crossed is a picture rather than a panic: the low
    /// knob is drawn at the high one, and the first thing that moves reports an
    /// ordered pair back.
    #[gpui::test]
    fn a_low_above_its_high_still_draws(cx: &mut TestAppContext) {
        let (range, _, mut cx) = open(0.8, 0.2, cx);

        // Both knobs are drawn at 0.2, so a press at the far end is a tie and
        // moves the high one; what matters is that the pair that comes back is
        // in order rather than the host's inside-out one.
        press(&mut cx, column(0.9));
        let (low, high) = range.get();
        assert!(low <= high, "({low}, {high})");
        assert!(close(low, 0.2), "{low}");
        assert!(close(high, 0.9), "{high}");
    }

    /// A knob stops at its neighbour and the two may meet, whichever end is
    /// moving.
    #[test]
    fn a_knob_stops_at_its_neighbour() {
        assert_eq!(placed(0.2, 0.8, Knob::Low, 0.5), (0.5, 0.8));
        assert_eq!(placed(0.2, 0.8, Knob::Low, 0.9), (0.8, 0.8));
        assert_eq!(placed(0.2, 0.8, Knob::High, 0.5), (0.2, 0.5));
        assert_eq!(placed(0.2, 0.8, Knob::High, 0.1), (0.2, 0.2));

        // And a value the host never clamped is pinned before it is placed.
        assert_eq!(placed(0.2, 0.8, Knob::High, 4.), (0.2, 1.));
        assert_eq!(placed(0.2, 0.8, Knob::Low, f32::NAN), (0., 0.8));
    }

    /// The nearer knob takes the press, and a tie goes to the high one.
    #[test]
    fn the_nearer_knob_takes_the_press() {
        assert_eq!(nearer(0.2, 0.8, 0.3), Knob::Low);
        assert_eq!(nearer(0.2, 0.8, 0.7), Knob::High);
        assert_eq!(nearer(0.2, 0.8, 0.5), Knob::High);
        assert_eq!(nearer(0.4, 0.4, 0.9), Knob::High);
        assert_eq!(nearer(0.4, 0.4, 0.1), Knob::High);
    }
}
