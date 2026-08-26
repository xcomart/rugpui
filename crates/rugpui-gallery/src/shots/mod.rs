//! One widget, one state, one small window: the pictures the documentation is
//! illustrated with.
//!
//! The gallery window answers "what is in this repository". It cannot answer
//! "what does `compact` look like", because every widget in it is drawn in one
//! state and the interesting ones are the other states. So the same binary has
//! a second mode: `--shot <name>` opens a window the size of one widget with
//! exactly that widget in exactly that state, and `scripts/docshots.sh` walks
//! the registry below, photographing each one into `docs/screenshots/<name>.png`.
//!
//! A shot is deliberately *not* a test. Nothing here asserts; the whole of what
//! it produces is a picture, and the picture is checked by looking at it. What
//! the [test at the bottom](self#tests) does check is the registry's own shape,
//! because a duplicate name silently overwrites a file and a name with a slash
//! too many silently writes into a directory nobody looks in.
//!
//! # What can and cannot be shown
//!
//! Everything here is *host-driven* state: an open flag the host holds, a
//! selection the host set, a focus the host moved. Hover and press are the
//! window system's and there is no way to hold a pointer down while a
//! screenshot is taken, so no shot claims to show one — a page that needs a
//! hover state says so in words instead.
//!
//! # Naming
//!
//! `<page>/<option>`, where `<page>` is the file stem of the documentation page
//! the picture belongs on — `button`, `text-input`, `grid`, `shell` — so that
//! the tree under `docs/screenshots/` mirrors the tree under `docs/`.

use gpui::{
    AnyElement, AnyView, App, Bounds, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use rugpui::theme;

mod controls;
mod data;
mod overlays;
mod palettes;
mod shell;

/// The padding every shot leaves around its widget.
///
/// One number, so that two pictures on the same page sit at the same inset and
/// a reader comparing them is comparing the widgets rather than the margins.
pub const PADDING: f32 = 16.;

/// What a shot's builder gives back: the element to draw, rebuilt per frame.
///
/// A closure rather than a view of its own, because a shot holds no state that
/// changes — whatever entities it needs are made once by the builder and cloned
/// into here.
type Body = Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>;

/// Whether a shot stands still, and if it does not, what it is doing.
///
/// A still shot is one capture and one PNG. A moving one cannot be: the widget
/// animates itself off its own element id, so what a single capture catches is
/// one arbitrary instant of it. `scripts/docshots.sh` therefore takes a
/// *handful* of captures of the same window and `scripts/docshots_gif.py`
/// reassembles them into a GIF — which it can do because the variant below says
/// what the motion is and how long one cycle of it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// Nothing moves. One capture, one PNG.
    Still,
    /// An arc turning clockwise once per period — `rugpui::Spinner`.
    Spin {
        /// One turn, in milliseconds. `rugpui::spinner::PERIOD`.
        period_ms: u32,
    },
    /// A segment crossing its track from left to right once per period —
    /// `rugpui::ProgressBar::indeterminate`.
    Sweep {
        /// One pass, in milliseconds. `rugpui::progress::SWEEP_DURATION`.
        period_ms: u32,
    },
}

impl Motion {
    /// How the motion is spelled in the `--list-shots` column: `spin:800`,
    /// `sweep:1200`, or nothing at all for a still shot.
    ///
    /// One field rather than two, so that the shell reading the line can branch
    /// on emptiness and pass the rest straight through to the assembler.
    pub fn tag(self) -> String {
        match self {
            Motion::Still => String::new(),
            Motion::Spin { period_ms } => format!("spin:{period_ms}"),
            Motion::Sweep { period_ms } => format!("sweep:{period_ms}"),
        }
    }
}

/// One named picture: what it is called, how big a window it wants, and how to
/// build it.
pub struct Shot {
    /// `<page>/<option>`; see the module docs.
    pub name: &'static str,
    /// Width of the window's *content*, in logical pixels — which is what the
    /// capture tool saves, so it is also the width of the file.
    pub width: f32,
    /// Height of the same.
    pub height: f32,
    /// Where a shot taken once per palette is filed, with `%s` standing for the
    /// palette id — `theme/%s`, `editor/theme-%s`. Empty for a shot taken once,
    /// which is nearly all of them.
    pub per_theme: &'static str,
    /// Whether the picture is one capture or a reassembled cycle of them.
    pub motion: Motion,
    /// Builds the view the window is opened on.
    pub build: fn(&mut Window, &mut App) -> AnyView,
}

/// Every group of shots, in the order `--list-shots` prints them.
const GROUPS: &[&[Shot]] = &[
    controls::SHOTS,
    overlays::SHOTS,
    data::SHOTS,
    palettes::SHOTS,
    shell::SHOTS,
];

/// Every shot in the registry.
pub fn all() -> impl Iterator<Item = &'static Shot> {
    GROUPS.iter().copied().flatten()
}

/// The shot `name` names, if there is one.
pub fn find(name: &str) -> Option<&'static Shot> {
    all().find(|shot| shot.name == name)
}

/// Opens the window one shot is photographed in.
///
/// Sized to the shot rather than to the screen, and titled with the shot's own
/// name so that a run left half-finished says which picture it stopped on.
pub fn open(shot: &'static Shot, theme_id: &str, cx: &mut App) {
    let title = format!("rugpui shot — {} — {theme_id}", shot.name);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(shot.width), px(shot.height)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            app_id: Some("rugpui-gallery".into()),
            ..Default::default()
        },
        |window, cx| {
            let view = (shot.build)(window, cx);
            cx.new(|_cx| Frame { view })
        },
    )
    .expect("failed to open the shot window");
}

/// The window's root, which holds the view a builder made.
///
/// gpui wants an entity at the root of a window and a shot's builder already
/// produced one; this is the one line that puts the second inside the first.
struct Frame {
    /// What the shot built.
    view: AnyView,
}

impl Render for Frame {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.view.clone())
    }
}

/// How many frames a shot is drawn before it is left alone.
///
/// A live application repaints constantly and a shot does not: it is one static
/// view, so gpui draws it once and stops. That is a frame too few for anything
/// that measures itself — an overlay scroll bar has no thumb until the surface
/// under it has been laid out, a grid has no fitted column widths until its
/// text has been shaped — and the picture would be taken of the frame before
/// those answers arrived. So the panel asks for a handful more and then stops,
/// which is bounded and cheap and leaves the window idle by the time the
/// capture runs.
const SETTLE_FRAMES: usize = 4;

/// The surface a shot is drawn on: the theme's background, the theme's text,
/// and [`PADDING`] around whatever the builder made.
struct Panel {
    /// Whether the padding is applied. A dialog paints its own backdrop over
    /// the whole window and an inset one would read as a bug.
    pad: bool,
    /// How many frames are still owed; see [`SETTLE_FRAMES`].
    settling: usize,
    /// Rebuilt every frame, the way a stateless widget is.
    body: Body,
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.settling > 0 {
            self.settling -= 1;
            cx.notify();
        }
        let palette = theme(cx);
        let body = (self.body)(window, cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(px(13.))
            .when(self.pad, |this| this.p(px(PADDING)))
            .child(body)
    }
}

/// A shot drawn inside the standard padding. The usual case.
fn panel(cx: &mut App, body: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> AnyView {
    cx.new(|_cx| Panel {
        pad: true,
        settling: SETTLE_FRAMES,
        body: Box::new(body),
    })
    .into()
}

/// A shot that fills the window edge to edge: a dialog, a title bar, a splitter.
fn bare(cx: &mut App, body: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> AnyView {
    cx.new(|_cx| Panel {
        pad: false,
        settling: SETTLE_FRAMES,
        body: Box::new(body),
    })
    .into()
}

/// A row of widgets, evenly spaced and centred on each other.
fn row() -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(10.))
}

/// A column of widgets.
fn column() -> gpui::Div {
    div().flex().flex_col().gap(px(10.))
}

/// A muted caption naming what the thing beside it is.
///
/// Used only where a picture would otherwise be ambiguous — three sliders at
/// three values, say, where the values are the whole point and are invisible.
fn caption(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let palette = theme(cx);
    div()
        .flex_none()
        .text_size(px(10.5))
        .text_color(palette.text_muted)
        .child(text.into())
        .into_any_element()
}

/// The bordered box the larger widgets are dropped into, matching the gallery's.
fn framed(cx: &App) -> gpui::Div {
    crate::framed(&theme(cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two shots under one name would silently overwrite one file with the
    /// other, and the loser would go on being referenced by a page.
    #[test]
    fn every_name_is_unique() {
        let mut names: Vec<&str> = all().map(|shot| shot.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "two shots share a name");
    }

    /// `<page>/<option>`, lower case, one slash. The name becomes a path under
    /// `docs/screenshots/`, so anything else writes somewhere unintended.
    #[test]
    fn every_name_is_a_page_and_an_option() {
        for shot in all() {
            let (page, option) = shot
                .name
                .split_once('/')
                .unwrap_or_else(|| panic!("{:?} is not <page>/<option>", shot.name));
            assert!(
                !page.is_empty() && page.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{:?} has a page part that is not lower-case letters and dashes",
                shot.name
            );
            assert!(
                !option.is_empty()
                    && option
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{:?} has an option part that is not lower-case letters, digits and dashes",
                shot.name
            );
        }
    }

    /// A window of no size is a window with nothing in the picture.
    #[test]
    fn every_size_is_positive() {
        for shot in all() {
            assert!(
                shot.width > 0. && shot.height > 0.,
                "{:?} asks for a window of {}x{}",
                shot.name,
                shot.width,
                shot.height
            );
        }
    }

    /// A cycle of no length is a GIF the assembler cannot lay out in time.
    #[test]
    fn every_moving_shot_has_a_period() {
        for shot in all() {
            match shot.motion {
                Motion::Still => {}
                Motion::Spin { period_ms } | Motion::Sweep { period_ms } => assert!(
                    period_ms > 0,
                    "{:?} moves and yet its cycle takes no time at all",
                    shot.name
                ),
            }
        }
    }

    /// The per-palette template is what the script substitutes into; one
    /// without a `%s` would write every palette over the same file.
    #[test]
    fn every_per_theme_template_names_the_palette() {
        for shot in all() {
            if !shot.per_theme.is_empty() {
                assert!(
                    shot.per_theme.contains("%s"),
                    "{:?} is taken per palette but files them all under {:?}",
                    shot.name,
                    shot.per_theme
                );
            }
        }
    }
}
