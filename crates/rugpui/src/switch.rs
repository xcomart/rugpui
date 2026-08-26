//! A labelled on/off switch.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, ClickEvent, ElementId, Hsla, SharedString, Window, div,
    ease_in_out, prelude::*, px,
};

use crate::theme::{Theme, lerp, theme};

/// Callback fired with the value the switch is about to take.
type ToggleHandler = Box<dyn Fn(bool, &mut Window, &mut App)>;

/// How long the knob takes to cross the track.
///
/// Short enough that a setting still feels like it took effect on the click,
/// long enough that the eye follows the knob from one end to the other rather
/// than being shown two pictures.
const SLIDE: Duration = Duration::from_millis(150);

/// Width of the track, in pixels.
const TRACK_WIDTH: f32 = 30.;

/// Height of the track, in pixels.
const TRACK_HEIGHT: f32 = 16.;

/// Thickness of the track's outline, which `border_1` draws *inside* the
/// track's own box.
const TRACK_BORDER: f32 = 1.;

/// Gap between the outline and the knob at the end the knob is resting at.
const TRACK_INSET: f32 = 2.;

/// Diameter of the knob, in pixels.
const KNOB: f32 = 12.;

/// Where the knob sits on the vertical, measured from inside the outline.
///
/// The knob is taller than the room the insets leave, so it is centred on the
/// track rather than fitted between them — which is what the flex layout this
/// replaced did, and why the two draw the same picture.
const KNOB_TOP: f32 = (TRACK_HEIGHT - 2. * TRACK_BORDER - KNOB) / 2.;

/// Everything the switch remembers between frames.
///
/// Kept in gpui's element state under the switch's own id rather than in the
/// host's struct: which way the knob is travelling is a fact about one switch
/// on screen and no use to anybody else, and it should live exactly as long as
/// the switch is drawn — which is precisely the lifetime element state has.
///
/// `generation` counts the changes of value this switch has seen, and is what
/// the slide is keyed on: a fresh id restarts gpui's clock, so the knob leaves
/// on every flip and on no other frame. Zero means the value has never changed
/// since the switch was mounted, which is drawn as the finished state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SlideState {
    /// The value the last frame was drawn for.
    shown: bool,
    /// How many times the value has changed under this switch.
    generation: u32,
}

/// Where a switch files its slide.
///
/// One function so the key is written once: the state is looked up by this id
/// on every frame, and a switch whose key drifted between frames would get a
/// fresh, generation-zero state each time and never animate at all.
fn slide_key(id: &ElementId) -> ElementId {
    ElementId::from((id.clone(), "switch-slide"))
}

/// The id the `generation`th slide of the switch called `id` animates under.
///
/// gpui keeps an animation's start time in element state keyed by this id and
/// drops it once the id stops being drawn, so a new generation starts its
/// clock from zero while a generation that is still running keeps hers — which
/// is the whole of the "the knob moves when, and only when, the value changed"
/// rule.
fn slide_id(id: &ElementId, generation: u32) -> ElementId {
    ElementId::from((
        ElementId::from((id.clone(), "switch-knob")),
        generation.to_string(),
    ))
}

/// How far through the slide the frame at `delta` is.
///
/// A switch whose value has never changed draws its finished state whatever
/// the clock says, so a switch that has only just been mounted — one scrolled
/// into view, or drawn `checked(true)` from the start — does not open with a
/// knob sliding in from the wrong end.
fn phase(delta: f32, generation: u32) -> f32 {
    if generation == 0 { 1.0 } else { delta }
}

/// Where the knob rests when the switch is settled at `checked`, measured from
/// inside the track's outline.
fn knob_rest(checked: bool) -> f32 {
    if checked {
        TRACK_WIDTH - 2. * TRACK_BORDER - TRACK_INSET - KNOB
    } else {
        TRACK_INSET
    }
}

/// Where the knob is drawn on the frame at `delta` of the `generation`th slide
/// towards `checked`.
///
/// The switch has two positions, so the end the knob is leaving is always the
/// one the other value rests at and there is no start position to remember.
fn knob_left(checked: bool, delta: f32, generation: u32) -> f32 {
    let from = knob_rest(!checked);
    let to = knob_rest(checked);
    from + (to - from) * phase(delta, generation)
}

/// The track fill, the track outline and the knob, in the position `checked`
/// names.
fn colors(theme: &Theme, checked: bool) -> (Hsla, Hsla, Hsla) {
    if checked {
        (theme.accent, theme.accent, theme.background)
    } else {
        (theme.surface, theme.border, theme.text_muted)
    }
}

/// A stateless on/off switch with a clickable label.
///
/// Like [`Checkbox`](crate::Checkbox) — which it mirrors down to the shape of
/// its API — it owns no state of its own: the parent view passes the current
/// value on every render and updates its own state from [`Switch::on_toggle`],
/// which receives the *new* value. Clicking anywhere on the row — track or
/// label — flips it.
///
/// The knob slides between the ends of the track, and the track and knob
/// colors travel with it; the switch remembers the slide under its own element
/// id, so that id must be unique among its siblings and must not change
/// between frames.
///
/// ```ignore
/// Switch::new("notifications", "Enable notifications")
///     .checked(self.notifications)
///     .on_toggle(cx.processor(|this, checked, _window, cx| {
///         this.notifications = checked;
///         cx.notify();
///     }))
/// ```
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    label: SharedString,
    checked: bool,
    tab_index: Option<isize>,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    /// Creates a switch in the off position.
    ///
    /// `id` must be unique among the siblings of the switch.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked: false,
            tab_index: None,
            on_toggle: None,
        }
    }

    /// Sets whether the switch is on.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Places the switch at `index` in the window's tab order.
    ///
    /// A focused switch draws an accent outline and toggles on `Space` or
    /// `Enter`, which gpui delivers as an ordinary click.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets the callback invoked with the value the switch is toggling to.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let checked = self.checked;
        let next = !checked;

        // Looked up rather than passed in, and looked up again on every frame:
        // the entity gpui hands back is the same one for as long as the switch
        // keeps being drawn. It starts life already showing `checked`, so the
        // first frame is the settled picture rather than the end of a slide
        // nobody asked for.
        let slide = window.use_keyed_state(slide_key(&self.id), cx, move |_, _| SlideState {
            shown: checked,
            generation: 0,
        });
        // Deliberately without `cx.notify()`: `use_keyed_state` has this view
        // observing the entity, and notifying from inside the render that
        // already draws the new value would ask for that frame again forever.
        let generation = slide.update(cx, |state, _| {
            if state.shown != checked {
                state.shown = checked;
                state.generation += 1;
            }
            state.generation
        });

        let (from_track, from_border, from_knob) = colors(&theme, !checked);
        let (to_track, to_border, to_knob) = colors(&theme, checked);

        // One animation for the whole track, with the knob built inside it, so
        // the position and the three colors are read off a single clock and
        // cannot drift apart by a frame.
        let track = div()
            .flex()
            .flex_none()
            .relative()
            .w(px(TRACK_WIDTH))
            .h(px(TRACK_HEIGHT))
            .rounded_full()
            .border_1()
            .with_animation(
                slide_id(&self.id, generation),
                Animation::new(SLIDE).with_easing(ease_in_out),
                move |track, delta| {
                    let phase = phase(delta, generation);
                    track
                        .bg(lerp(from_track, to_track, phase))
                        .border_color(lerp(from_border, to_border, phase))
                        .child(
                            // Absolute, so the knob is placed by an offset the
                            // animation can name a fraction of; a flex
                            // alignment has only the two ends to offer.
                            div()
                                .absolute()
                                .top(px(KNOB_TOP))
                                .left(px(knob_left(checked, delta, generation)))
                                .size(px(KNOB))
                                .rounded_full()
                                .bg(lerp(from_knob, to_knob, phase)),
                        )
                },
            );

        div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .py(px(1.))
            .rounded_sm()
            // Transparent until focused, so the ring costs no layout.
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(theme.text)
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
            })
            .when_some(self.on_toggle, |this, handler| {
                this.on_click(move |_: &ClickEvent, window, cx| handler(next, window, cx))
            })
            .child(track)
            .child(div().child(self.label))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ops::Deref;
    use std::rc::Rc;

    use gpui::{Context, Entity, Render, TestAppContext, VisualTestContext, size};

    use super::*;

    /// Size of the window the harness opens in. Nothing is measured against it;
    /// it only has to be big enough to lay one switch out in.
    const HARNESS_WIDTH: f32 = 200.;

    /// Height of the same window.
    const HARNESS_HEIGHT: f32 = 60.;

    /// The knob rests at the same two places the flex layout this replaced put
    /// it: hard against the inset at either end of the track.
    #[test]
    fn the_knob_rests_against_either_inset() {
        assert_eq!(knob_rest(false), TRACK_INSET);
        assert_eq!(knob_rest(true), 14.);
        // Symmetric: the gap left at the far end is the gap at the near one.
        assert_eq!(
            knob_rest(true) + KNOB + TRACK_INSET,
            TRACK_WIDTH - 2. * TRACK_BORDER
        );
        // And the knob is centred on the track rather than fitted inside the
        // insets, which it is too tall for.
        assert_eq!(KNOB_TOP, 1.);
    }

    /// A slide runs from the end the other value rests at to the end this one
    /// does, and the frames in between are in between.
    #[test]
    fn the_knob_crosses_from_one_end_to_the_other() {
        assert_eq!(knob_left(true, 0.0, 1), knob_rest(false));
        assert_eq!(knob_left(true, 1.0, 1), knob_rest(true));
        assert_eq!(
            knob_left(true, 0.5, 1),
            (knob_rest(false) + knob_rest(true)) / 2.
        );

        // And the same journey the other way.
        assert_eq!(knob_left(false, 0.0, 3), knob_rest(true));
        assert_eq!(knob_left(false, 1.0, 3), knob_rest(false));
    }

    /// A switch that has never changed value is drawn settled, whatever the
    /// clock says — otherwise every switch on a freshly opened window would
    /// open with its knob sliding in from the wrong end.
    #[test]
    fn a_switch_that_never_changed_does_not_slide() {
        for delta in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(phase(delta, 0), 1.0);
            assert_eq!(knob_left(true, delta, 0), knob_rest(true));
            assert_eq!(knob_left(false, delta, 0), knob_rest(false));
        }

        // Once it has changed, the clock is what says where the knob is.
        assert_eq!(phase(0.25, 1), 0.25);
    }

    /// The two positions differ in all three colors, which is what makes a
    /// blend of them worth drawing.
    #[test]
    fn the_two_positions_are_drawn_in_different_colors() {
        let theme = Theme::dark();
        let (off_track, off_border, off_knob) = colors(&theme, false);
        let (on_track, on_border, on_knob) = colors(&theme, true);

        assert_ne!(off_track, on_track);
        assert_ne!(off_border, on_border);
        assert_ne!(off_knob, on_knob);
        assert_eq!(
            (on_track, on_border, on_knob),
            (theme.accent, theme.accent, theme.background)
        );
    }

    /// Where the harness leaves the switch's state for the test to find.
    ///
    /// An `Option` because it is only filled once a frame has been drawn, and a
    /// cell because the harness writes it from its own render.
    type Watched = Rc<RefCell<Option<Entity<SlideState>>>>;

    /// A window with one switch in it, whose value the test flips.
    struct Harness {
        /// What the switch is handed on the next render.
        checked: Rc<Cell<bool>>,
        /// Where the switch's own slide state is left for the test.
        slide: Watched,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let checked = self.checked.clone();
            // gpui isolates a stateless view's subtree under its type name —
            // see `ViewElement::request_layout` — so the switch's own element
            // ids sit one level below the harness's. Standing in the same place
            // is what makes this the switch's state rather than a second copy
            // of it, and the assertions below fail loudly if that stops being
            // true.
            //
            // The harness renders before the switch does, so this call is the
            // one that creates the state: it starts at `false`, which is what
            // the switch is first handed.
            let slide = window.with_id(
                ElementId::Name(std::any::type_name::<Switch>().into()),
                |window| {
                    window.use_keyed_state(slide_key(&"toggle".into()), cx, |_, _| {
                        SlideState::default()
                    })
                },
            );
            *self.slide.borrow_mut() = Some(slide);
            div().size_full().child(
                Switch::new("toggle", "Auto-reconnect")
                    .checked(checked.get())
                    .on_toggle(move |value, _window, cx| {
                        this.update(cx, |harness: &mut Harness, cx| {
                            harness.checked.set(value);
                            cx.notify();
                        });
                    }),
            )
        }
    }

    /// The state one switch keeps, reached from the test.
    fn slide_state(state: &Watched, cx: &mut VisualTestContext) -> SlideState {
        let state = state.borrow().clone().expect("a frame was drawn");
        cx.read(|cx| *state.read(cx))
    }

    /// A switch remembers the value it was last drawn for, and counts a slide
    /// only on the renders where that value changed — which is what keeps the
    /// knob still on every other frame the host draws.
    #[gpui::test]
    fn a_slide_is_counted_only_when_the_value_changes(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let checked = Rc::new(Cell::new(false));
        let slide: Watched = Rc::default();
        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), {
            let checked = checked.clone();
            let slide = slide.clone();
            move |_, _| Harness { checked, slide }
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();

        assert_eq!(
            slide_state(&slide, &mut cx),
            SlideState {
                shown: false,
                generation: 0
            },
            "a switch nobody has touched counted a slide anyway"
        );

        checked.set(true);
        cx.update(|_, cx| cx.refresh_windows());
        cx.run_until_parked();
        assert_eq!(
            slide_state(&slide, &mut cx),
            SlideState {
                shown: true,
                generation: 1
            },
            "the knob did not set off for the other end"
        );

        // Redrawn at the same value as often as the host likes: still one
        // slide, so the animation already running is not restarted.
        cx.update(|_, cx| cx.refresh_windows());
        cx.run_until_parked();
        assert_eq!(
            slide_state(&slide, &mut cx),
            SlideState {
                shown: true,
                generation: 1
            },
            "a redraw at an unchanged value restarted the slide"
        );

        checked.set(false);
        cx.update(|_, cx| cx.refresh_windows());
        cx.run_until_parked();
        assert_eq!(
            slide_state(&slide, &mut cx),
            SlideState {
                shown: false,
                generation: 2
            },
            "the way back was not counted as a slide of its own"
        );
    }
}
