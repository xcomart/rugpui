//! A colour picker: a saturation/value field over a hue strip, and an optional
//! row of preset swatches.
//!
//! The picker works in [`Hsv`] rather than in gpui's [`Hsla`], and that is the
//! one decision worth explaining. A colour drawn from the field's left edge or
//! its bottom row is a grey, and a grey has no hue — so a picker that kept only
//! the colour would forget the hue the moment the knob touched an edge, and the
//! field behind it would jump to red. Hue, saturation and value are the three
//! things the pointer moves, so they are what the host keeps, and a grey keeps
//! the hue it was picked under.
//!
//! Like the rest of the kit the picker is stateless: the owning view passes the
//! current [`Hsv`] on every render and updates its own state from
//! [`ColorPicker::on_change`]. What a colour is spelled as — `#rrggbb`, a
//! theme slot, a name — is the host's business; [`Hsv::to_hex`] and
//! [`Hsv::from_hex`] are there for the common case, and a host that wants a
//! hex field beside the picker puts its own [`crate::TextInput`] there.
//!
//! Four ways to move it, all ending at the same callback:
//!
//! * Pressing or dragging in the field, which sets saturation (across) and
//!   value (down).
//! * Pressing or dragging in the hue strip.
//! * The keyboard, while the picker holds focus: the arrows move the field's
//!   knob a hundredth at a time, or a tenth with `Shift`, and `PageUp` /
//!   `PageDown` turn the hue by ten degrees.
//! * Clicking a preset swatch, when the host gave some.
//!
//! A drag finds its way home the way a [`crate::Slider`]'s does: the pressed
//! area carries its own bounds, measured by a [`canvas`] during the frame it was
//! built in, inside a [`DraggedColor`], so a pointer position alone answers
//! "what colour is this now?" wherever the pointer has wandered.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, DragMoveEvent, ElementId, Hsla, MouseButton, MouseDownEvent, Pixels, Point, Rgba,
    Window, canvas, div, linear_color_stop, linear_gradient, prelude::*, px, relative,
    transparent_black,
};

use crate::menu::color_swatch;
use crate::theme::{parse_hex, theme};

/// Height of the saturation/value field unless the host names one; see
/// [`ColorPicker::field_height`].
const FIELD_HEIGHT: f32 = 150.;

/// Height of the hue strip.
const HUE_HEIGHT: f32 = 14.;

/// Space between the field, the strip and the presets.
const GAP: f32 = 10.;

/// Diameter of the field's knob.
const FIELD_KNOB: f32 = 12.;

/// Width of the hue strip's knob.
const HUE_KNOB: f32 = 6.;

/// Edge length of a preset swatch.
const PRESET: f32 = 18.;

/// Padding inside the focus ring, which is drawn as a one-pixel border.
const RING_PAD: f32 = 2.;

/// How far an arrow key moves the field's knob, and how far with `Shift`.
const FINE_STEP: f32 = 0.01;
const COARSE_STEP: f32 = 0.1;

/// How far `PageUp` / `PageDown` turn the hue: ten degrees.
const HUE_STEP: f32 = 10. / 360.;

/// Callback fired with the colour the picker is moving to.
type ChangeHandler = Rc<dyn Fn(Hsv, &mut Window, &mut App)>;

/// A colour as hue, saturation and value, each a fraction from `0.` to `1.`.
///
/// The space the picker is drawn in; see the module docs for why it is kept
/// rather than converted away. Fully opaque: the picker has no alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    /// Hue, `0.` and `1.` both being red.
    pub h: f32,
    /// Saturation: `0.` is grey, `1.` the pure hue.
    pub s: f32,
    /// Value: `0.` is black, `1.` the brightest the hue and saturation allow.
    pub v: f32,
}

impl Default for Hsv {
    /// Opaque white, under a red hue.
    fn default() -> Self {
        Self::new(0., 0., 1.)
    }
}

impl Hsv {
    /// A colour from its three parts, each clamped into `0.0..=1.0`. A NaN
    /// part — what a division by an empty area arrives at — counts as `0.`.
    pub fn new(h: f32, s: f32, v: f32) -> Self {
        let unit = |value: f32| {
            if value.is_nan() {
                0.
            } else {
                value.clamp(0., 1.)
            }
        };
        Self {
            h: unit(h),
            s: unit(s),
            v: unit(v),
        }
    }

    /// The colour `rgba` paints, its alpha ignored.
    ///
    /// A grey answers hue `0.`; use [`Hsv::keeping_hue`] to carry a hue over.
    pub fn from_rgba(rgba: Rgba) -> Self {
        let (r, g, b) = (rgba.r, rgba.g, rgba.b);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let h = if delta <= f32::EPSILON {
            0.
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.) / 6.
        } else if max == g {
            ((b - r) / delta + 2.) / 6.
        } else {
            ((r - g) / delta + 4.) / 6.
        };
        let s = if max <= f32::EPSILON { 0. } else { delta / max };
        Self::new(h, s, max)
    }

    /// The colour `hsla` paints, its alpha ignored.
    pub fn from_hsla(hsla: Hsla) -> Self {
        Self::from_rgba(hsla.to_rgb())
    }

    /// The colour a `#rrggbb` spells — the `#` optional, the digits in either
    /// case — or `None` for anything else.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits = hex.trim();
        let digits = digits.strip_prefix('#').unwrap_or(digits);
        if digits.len() != 6 {
            return None;
        }
        parse_hex(digits).map(Self::from_hsla)
    }

    /// This colour, but under `previous`'s hue when this one is a grey and so
    /// has none of its own — what a host does with a colour typed into a hex
    /// field, so the field behind the knob does not jump to red.
    pub fn keeping_hue(self, previous: Hsv) -> Self {
        if self.s <= f32::EPSILON || self.v <= f32::EPSILON {
            Self {
                h: previous.h,
                ..self
            }
        } else {
            self
        }
    }

    /// The colour, as gpui paints it.
    pub fn to_rgba(self) -> Rgba {
        let h = self.h.rem_euclid(1.) * 6.;
        let c = self.v * self.s;
        let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
        let m = self.v - c;
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.),
            1 => (x, c, 0.),
            2 => (0., c, x),
            3 => (0., x, c),
            4 => (x, 0., c),
            _ => (c, 0., x),
        };
        Rgba {
            r: r + m,
            g: g + m,
            b: b + m,
            a: 1.,
        }
    }

    /// The colour, as gpui's [`Hsla`].
    pub fn to_hsla(self) -> Hsla {
        self.to_rgba().into()
    }

    /// The colour as `#rrggbb`, in lower case.
    pub fn to_hex(self) -> String {
        let rgba = self.to_rgba();
        let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b)
        )
    }

    /// The pure hue behind this colour, at full saturation and value: what the
    /// field is filled with before its two gradients are laid over it.
    fn pure(self) -> Hsla {
        Self::new(self.h, 1., 1.).to_hsla()
    }
}

/// Which part of the picker a drag took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The saturation/value field.
    Field,
    /// The hue strip.
    Hue,
}

/// What a drag in a [`ColorPicker`] carries: which picker and which part of
/// it, and that part's bounds in window coordinates.
///
/// Public because gpui names a drag by its payload type, and a host that
/// listens for drags itself has to be able to name this one; see
/// [`DraggedColor::color`].
pub struct DraggedColor {
    /// The picker being dragged in.
    id: ElementId,
    /// The part the press landed in.
    part: Part,
    /// That part's bounds, shared with the canvas that measures them.
    area: Rc<Cell<Bounds<Pixels>>>,
}

impl DraggedColor {
    /// The colour this drag has reached from `current`, if it belongs to the
    /// picker `id`.
    pub fn color(&self, id: &ElementId, position: Point<Pixels>, current: Hsv) -> Option<Hsv> {
        if self.id != *id {
            return None;
        }
        let area = self.area.get();
        Some(match self.part {
            Part::Field => field_at(area, position, current),
            Part::Hue => hue_at(area, position, current),
        })
    }
}

/// Where `position` falls across `area`, as a fraction clamped into `0..=1`.
fn across(area: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let width = f32::from(area.size.width);
    if width <= 0. {
        return 0.;
    }
    (f32::from(position.x - area.origin.x) / width).clamp(0., 1.)
}

/// Where `position` falls down `area`, as a fraction clamped into `0..=1`.
fn down(area: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let height = f32::from(area.size.height);
    if height <= 0. {
        return 0.;
    }
    (f32::from(position.y - area.origin.y) / height).clamp(0., 1.)
}

/// The colour a pointer at `position` picks in the field `area`: saturation
/// across, value down — brightest at the top — and `current`'s hue.
fn field_at(area: Bounds<Pixels>, position: Point<Pixels>, current: Hsv) -> Hsv {
    Hsv::new(current.h, across(area, position), 1. - down(area, position))
}

/// The colour a pointer at `position` picks in the hue strip `area`: the hue
/// across, and `current`'s saturation and value.
fn hue_at(area: Bounds<Pixels>, position: Point<Pixels>, current: Hsv) -> Hsv {
    Hsv::new(across(area, position), current.s, current.v)
}

/// The colour a key press asks for from `current`, or `None` for a key the
/// picker does not answer to.
fn keyed(current: Hsv, key: &str, shift: bool) -> Option<Hsv> {
    let step = if shift { COARSE_STEP } else { FINE_STEP };
    let Hsv { h, s, v } = current;
    Some(match key {
        "left" => Hsv::new(h, s - step, v),
        "right" => Hsv::new(h, s + step, v),
        "up" => Hsv::new(h, s, v + step),
        "down" => Hsv::new(h, s, v - step),
        // The hue wraps, since red sits at both ends of the strip.
        "pageup" => Hsv {
            h: (h + HUE_STEP).rem_euclid(1.),
            s,
            v,
        },
        "pagedown" => Hsv {
            h: (h - HUE_STEP).rem_euclid(1.),
            s,
            v,
        },
        _ => return None,
    })
}

/// A stateless colour picker.
///
/// The parent view passes the current colour on every render and updates its
/// own state from [`ColorPicker::on_change`]. It fills the width it is given;
/// the field is a fixed height, the hue strip under it, and the presets — if
/// any — under that.
///
/// `id` has to be unique among every picker that can be dragged at the same
/// time — in practice, within the window — because it is what tells one
/// picker's drag from another's.
///
/// ```ignore
/// let this = cx.entity();
/// ColorPicker::new("header-color")
///     .value(self.color)
///     .presets([rgb(0xdc2626).into(), rgb(0x2563eb).into()])
///     .tab_index(0)
///     .on_change(move |color, _window, cx| {
///         this.update(cx, |view, cx| {
///             view.color = color;
///             cx.notify();
///         });
///     })
/// ```
#[derive(IntoElement)]
pub struct ColorPicker {
    id: ElementId,
    value: Hsv,
    presets: Vec<Hsla>,
    field_height: Pixels,
    tab_index: Option<isize>,
    on_change: Option<ChangeHandler>,
}

impl ColorPicker {
    /// Creates a picker showing white, with no presets and no tab stop.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: Hsv::default(),
            presets: Vec::new(),
            field_height: px(FIELD_HEIGHT),
            tab_index: None,
            on_change: None,
        }
    }

    /// Sets the height of the saturation/value field, 150 px unless set. The
    /// strip and the presets keep their own heights, so this is the one knob a
    /// host short of room turns.
    pub fn field_height(mut self, height: Pixels) -> Self {
        self.field_height = height;
        self
    }

    /// Sets the colour shown.
    pub fn value(mut self, value: Hsv) -> Self {
        self.value = value;
        self
    }

    /// Lays a row of swatches under the strip, each picking its colour on a
    /// click. The one matching the current colour is ringed.
    pub fn presets(mut self, presets: impl IntoIterator<Item = Hsla>) -> Self {
        self.presets = presets.into_iter().collect();
        self
    }

    /// Makes the picker a tab stop at `index`, answering the keys the module
    /// docs list while it holds focus.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets the callback run with the colour the picker is moving to.
    pub fn on_change(mut self, handler: impl Fn(Hsv, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// A canvas filling its parent, recording the parent's bounds into `area`
/// during prepaint and painting nothing.
fn measure(area: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(
        move |bounds, _window, _cx| area.set(bounds),
        |_bounds, _measured, _window, _cx| {},
    )
    .absolute()
    .size_full()
}

/// A press in `area` of `part`, answered with the colour it picks.
fn pressed(
    part: Part,
    area: Rc<Cell<Bounds<Pixels>>>,
    current: Hsv,
    on_change: Option<ChangeHandler>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        let Some(handler) = on_change.as_ref() else {
            return;
        };
        let next = match part {
            Part::Field => field_at(area.get(), event.position, current),
            Part::Hue => hue_at(area.get(), event.position, current),
        };
        if next != current {
            handler(next, window, cx);
        }
    }
}

/// The hue strip's fill: six two-stop gradients end to end, one per sixth of
/// the wheel, since a gradient in gpui has only the two stops.
fn hue_segments() -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_row()
        .children((0..6).map(|sixth| {
            let from = Hsv::new(sixth as f32 / 6., 1., 1.).to_hsla();
            let to = Hsv::new((sixth + 1) as f32 / 6., 1., 1.).to_hsla();
            // The last stop is red again, which `Hsv::new` would clamp to the
            // same red; spelled out so the strip closes on the colour it opened on.
            let to = if sixth == 5 {
                Hsv::new(0., 1., 1.).to_hsla()
            } else {
                to
            };
            div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ))
        }))
}

/// A ring the knobs are drawn as: white with a dark edge, so it reads on every
/// colour the field can show.
fn knob(size: f32) -> gpui::Div {
    div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .border_2()
        .border_color(gpui::white())
        .shadow_sm()
}

impl RenderOnce for ColorPicker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let id = self.id;
        let value = self.value;
        let on_change = self.on_change;

        let field_area: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
        let hue_area: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

        // White fading in from the left takes the saturation away; black
        // fading in from the top takes the value. Both over the pure hue.
        let field = div()
            .id(ElementId::from((id.clone(), "field")))
            .relative()
            .w_full()
            .h(self.field_height)
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(value.pure())
            .cursor_crosshair()
            .child(measure(field_area.clone()))
            .child(div().absolute().inset_0().rounded_md().bg(linear_gradient(
                90.,
                linear_color_stop(gpui::white(), 0.),
                linear_color_stop(gpui::white().opacity(0.), 1.),
            )))
            .child(div().absolute().inset_0().rounded_md().bg(linear_gradient(
                180.,
                linear_color_stop(gpui::black().opacity(0.), 0.),
                linear_color_stop(gpui::black(), 1.),
            )))
            .child(
                // Zero-sized, at the colour's place, with the knob centred on
                // it by a plain offset.
                div()
                    .absolute()
                    .left(relative(value.s))
                    .top(relative(1. - value.v))
                    .size_0()
                    .child(
                        div()
                            .absolute()
                            .left(px(-FIELD_KNOB / 2.))
                            .top(px(-FIELD_KNOB / 2.))
                            .child(knob(FIELD_KNOB).bg(value.to_hsla())),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                pressed(Part::Field, field_area.clone(), value, on_change.clone()),
            )
            .on_drag(
                DraggedColor {
                    id: id.clone(),
                    part: Part::Field,
                    area: field_area,
                },
                |_, _, _, cx| cx.new(|_| gpui::Empty),
            );

        let hue = div()
            .id(ElementId::from((id.clone(), "hue")))
            .relative()
            .w_full()
            .h(px(HUE_HEIGHT))
            .rounded_sm()
            .overflow_hidden()
            .cursor_pointer()
            .child(measure(hue_area.clone()))
            .child(hue_segments())
            .child(
                div()
                    .absolute()
                    .left(relative(value.h))
                    .top_0()
                    .bottom_0()
                    .w_0()
                    .child(
                        div()
                            .absolute()
                            .left(px(-HUE_KNOB / 2.))
                            .top_0()
                            .bottom_0()
                            .w(px(HUE_KNOB))
                            .rounded_sm()
                            .border_2()
                            .border_color(gpui::white())
                            .shadow_sm(),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                pressed(Part::Hue, hue_area.clone(), value, on_change.clone()),
            )
            .on_drag(
                DraggedColor {
                    id: id.clone(),
                    part: Part::Hue,
                    area: hue_area,
                },
                |_, _, _, cx| cx.new(|_| gpui::Empty),
            );

        let current_hex = value.to_hex();
        let presets = (!self.presets.is_empty()).then(|| {
            div().flex().flex_row().flex_wrap().gap(px(6.)).children(
                self.presets.into_iter().enumerate().map(|(index, color)| {
                    let picked = Hsv::from_hsla(color);
                    let current = picked.to_hex() == current_hex;
                    let on_change = on_change.clone();
                    div()
                        .id(ElementId::from(("preset", index)))
                        .p(px(1.))
                        .rounded_sm()
                        .border_1()
                        .border_color(if current {
                            theme.accent
                        } else {
                            transparent_black()
                        })
                        .cursor_pointer()
                        .child(color_swatch(color, &theme).size(px(PRESET)))
                        .on_click(move |_, window, cx| {
                            if let Some(handler) = on_change.as_ref() {
                                // A grey preset keeps the hue the field was
                                // under, like a typed grey does.
                                handler(picked.keeping_hue(value), window, cx);
                            }
                        })
                }),
            )
        });

        div()
            .id(id.clone())
            .flex()
            .flex_col()
            .gap(px(GAP))
            .w_full()
            .p(px(RING_PAD))
            .rounded_md()
            // Transparent until focused, so the ring costs no layout.
            .border_1()
            .border_color(transparent_black())
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                let on_key = on_change.clone();
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
                    .on_key_down(move |event, window, cx| {
                        let modifiers = event.keystroke.modifiers;
                        if modifiers.control || modifiers.alt || modifiers.platform {
                            return;
                        }
                        let Some(next) = keyed(value, &event.keystroke.key, modifiers.shift) else {
                            return;
                        };
                        // Stopped either way: the picker owns those keys while
                        // it holds focus, whether or not the press moved it.
                        cx.stop_propagation();
                        if let Some(handler) = on_key.as_ref()
                            && next != value
                        {
                            handler(next, window, cx);
                        }
                    })
            })
            // The picker hears its own drags, wherever the pointer wanders,
            // because gpui offers a drag move to every listener of that type.
            .on_drag_move(move |event: &DragMoveEvent<DraggedColor>, window, cx| {
                let Some(handler) = on_change.as_ref() else {
                    return;
                };
                if let Some(next) = event.drag(cx).color(&id, event.event.position, value)
                    && next != value
                {
                    handler(next, window, cx);
                }
            })
            .child(field)
            .child(hue)
            .children(presets)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use gpui::{
        Context, Modifiers, MouseUpEvent, Render, TestAppContext, VisualTestContext, point, rgb,
        size,
    };

    use super::*;

    /// Width the harness gives the picker.
    const WIDTH: f32 = 200.;

    /// How far in from the picker's edge its field starts: the ring's padding,
    /// the ring's own pixel, and the field's border.
    const INSET: f32 = RING_PAD + 1.;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-3
    }

    fn area(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn hsv_round_trips_through_rgb_and_hex() {
        for hex in [
            "#ff0000", "#00ff00", "#0000ff", "#2563eb", "#7f1d1d", "#000000", "#ffffff",
        ] {
            let color = Hsv::from_hex(hex).expect("a colour");
            assert_eq!(color.to_hex(), hex);
        }
        let blue = Hsv::from_hex("2563EB").expect("the # is optional");
        assert!(close(blue.to_hsla().to_rgb().b, 0xeb as f32 / 255.));
        assert_eq!(Hsv::from_hex("#abc"), None);
        assert_eq!(Hsv::from_hex("#12345g"), None);
        assert_eq!(Hsv::from_hex("#11223344"), None);
    }

    #[test]
    fn the_parts_of_a_colour_are_read_back() {
        let red = Hsv::from_hsla(rgb(0xff0000).into());
        assert!(close(red.h, 0.) && close(red.s, 1.) && close(red.v, 1.));
        let cyan = Hsv::from_hsla(rgb(0x00ffff).into());
        assert!(close(cyan.h, 0.5), "{}", cyan.h);
        let grey = Hsv::from_hsla(rgb(0x808080).into());
        assert!(close(grey.s, 0.));
        assert!(close(grey.v, 128. / 255.));
    }

    #[test]
    fn a_grey_keeps_the_hue_it_was_picked_under() {
        let previous = Hsv::new(0.6, 0.8, 0.8);
        let grey = Hsv::from_hex("#808080").expect("grey");
        assert!(close(grey.keeping_hue(previous).h, 0.6));
        // A colour with a hue of its own keeps it.
        let red = Hsv::from_hex("#ff0000").expect("red");
        assert!(close(red.keeping_hue(previous).h, 0.));
    }

    #[test]
    fn parts_are_clamped_and_nan_is_zero() {
        let color = Hsv::new(1.5, -1., f32::NAN);
        assert_eq!(color, Hsv::new(1., 0., 0.));
    }

    #[test]
    fn the_field_reads_saturation_across_and_value_down() {
        let field = area(10., 20., 100., 50.);
        let current = Hsv::new(0.3, 0., 0.);
        let picked = field_at(field, point(px(60.), px(20.)), current);
        assert!(close(picked.h, 0.3) && close(picked.s, 0.5) && close(picked.v, 1.));
        let picked = field_at(field, point(px(10.), px(70.)), current);
        assert!(close(picked.s, 0.) && close(picked.v, 0.));
        // Past the edges, the edges.
        let picked = field_at(field, point(px(500.), px(-40.)), current);
        assert!(close(picked.s, 1.) && close(picked.v, 1.));
    }

    #[test]
    fn the_strip_reads_the_hue_and_keeps_the_rest() {
        let strip = area(0., 0., 200., 14.);
        let picked = hue_at(strip, point(px(50.), px(7.)), Hsv::new(0.9, 0.4, 0.6));
        assert!(close(picked.h, 0.25) && close(picked.s, 0.4) && close(picked.v, 0.6));
        // An area with no room answers the start rather than NaN.
        let picked = hue_at(area(0., 0., 0., 0.), point(px(5.), px(5.)), Hsv::default());
        assert!(close(picked.h, 0.));
    }

    #[test]
    fn the_keys_step_the_field_and_turn_the_hue() {
        let current = Hsv::new(0.5, 0.5, 0.5);
        assert!(close(keyed(current, "right", false).expect("key").s, 0.51));
        assert!(close(keyed(current, "down", true).expect("key").v, 0.4));
        assert!(close(
            keyed(current, "pageup", false).expect("key").h,
            0.5 + HUE_STEP
        ));
        // The hue wraps past red.
        let red = Hsv::new(0., 1., 1.);
        assert!(close(
            keyed(red, "pagedown", false).expect("key").h,
            1. - HUE_STEP
        ));
        assert_eq!(keyed(current, "a", false), None);
    }

    #[test]
    fn a_drag_answers_only_to_its_own_picker() {
        let dragged = DraggedColor {
            id: ElementId::from("one"),
            part: Part::Hue,
            area: Rc::new(Cell::new(area(0., 0., 100., 10.))),
        };
        let position = point(px(25.), px(5.));
        assert!(
            dragged
                .color(&ElementId::from("two"), position, Hsv::default())
                .is_none()
        );
        let picked = dragged
            .color(&ElementId::from("one"), position, Hsv::default())
            .expect("its own drag");
        assert!(close(picked.h, 0.25));
    }

    /// A picker in a window, recording what it answered.
    struct Harness {
        value: Rc<Cell<Hsv>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            div().size_full().child(
                div().w(px(WIDTH)).child(
                    ColorPicker::new("picker")
                        .value(self.value.get())
                        .presets([rgb(0x2563eb).into()])
                        .on_change(move |color, _window, cx| {
                            this.update(cx, |harness, cx| {
                                harness.value.set(color);
                                cx.notify();
                            });
                        }),
                ),
            )
        }
    }

    fn open(value: Hsv, cx: &mut TestAppContext) -> (Rc<Cell<Hsv>>, VisualTestContext) {
        cx.update(crate::init);
        let showing = Rc::new(Cell::new(value));
        let window = cx.add_window({
            let showing = showing.clone();
            move |_, _| Harness { value: showing }
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        (showing, cx)
    }

    fn press(cx: &mut VisualTestContext, x: f32, y: f32) {
        let position = point(px(x), px(y));
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

    /// A press in the field and one in the strip land where they were made:
    /// the box the canvas measured is the box the colour is drawn against.
    #[gpui::test]
    fn presses_in_the_field_and_the_strip_pick_what_is_under_them(cx: &mut TestAppContext) {
        let (value, mut cx) = open(Hsv::new(0.5, 0., 0.), cx);
        let inner = WIDTH - 2. * INSET;

        // The middle of the field, halfway across and halfway down.
        press(&mut cx, INSET + inner / 2., INSET + FIELD_HEIGHT / 2.);
        let picked = value.get();
        assert!(close(picked.h, 0.5), "{picked:?}");
        assert!((picked.s - 0.5).abs() < 0.02, "{picked:?}");
        assert!((picked.v - 0.5).abs() < 0.02, "{picked:?}");

        // A quarter of the way along the strip.
        let strip = INSET + FIELD_HEIGHT + GAP + HUE_HEIGHT / 2.;
        press(&mut cx, INSET + inner / 4., strip);
        let picked = value.get();
        assert!((picked.h - 0.25).abs() < 0.02, "{picked:?}");
        assert!(
            (picked.s - 0.5).abs() < 0.02,
            "the strip moved the field: {picked:?}"
        );

        // The preset under the strip.
        let preset = strip + HUE_HEIGHT / 2. + GAP + 2. + PRESET / 2.;
        press(&mut cx, INSET + 2. + PRESET / 2., preset);
        assert_eq!(value.get().to_hex(), "#2563eb");
    }

    /// A drag in the field follows the pointer, and keeps answering once the
    /// pointer has left everything the picker drew.
    #[gpui::test]
    fn dragging_in_the_field_follows_the_pointer(cx: &mut TestAppContext) {
        let (value, mut cx) = open(Hsv::new(0.5, 0., 0.), cx);
        let inner = WIDTH - 2. * INSET;

        let start = point(px(INSET + 20.), px(INSET + 20.));
        cx.simulate_mouse_move(start, None, Modifiers::none());
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(start.x + px(3.), start.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();

        let along = point(px(INSET + 0.75 * inner), px(INSET + 0.25 * FIELD_HEIGHT));
        cx.simulate_mouse_move(along, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        let picked = value.get();
        assert!((picked.s - 0.75).abs() < 0.02, "{picked:?}");
        assert!((picked.v - 0.75).abs() < 0.02, "{picked:?}");

        let past = point(px(WIDTH + 400.), px(-400.));
        cx.simulate_mouse_move(past, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(past, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let picked = value.get();
        assert!(close(picked.s, 1.) && close(picked.v, 1.), "{picked:?}");
        assert!(
            close(picked.h, 0.5),
            "a drag in the field moved the hue: {picked:?}"
        );
    }
}
