//! The small box that appears when the pointer rests on a control, and what
//! can be put inside it.
//!
//! gpui asks for tooltips as a *builder*: `.tooltip(f)` stores `f`, and calls it
//! to make a fresh view each time the pointer settles. The view has to be an
//! [`AnyView`], so a tooltip cannot be a plain element the way the other widgets
//! here are — it needs an entity behind it. [`tooltip_label`] hides that: it
//! takes the text once and hands back the closure `.tooltip` wants.
//!
//! Nothing here positions anything. gpui lays the view out by the pointer — in
//! the vendored copy, clear of the cursor's image: below the arrow, above the
//! pointer where the window ends too soon, slid sideways to stay inside the
//! content a client-decorated window shows (`tooltip_origin` in
//! `vendor/gpui/src/window.rs`). So the widget's only job is to be a box of the
//! right size. Adding an `anchored` or a `deferred` would fight machinery that
//! has already done the work, and so would a margin: the view is laid out as a
//! root, where a margin is not laid out at all.
//!
//! The styling is the menu panel's, one step quieter: a tooltip is read and
//! dismissed rather than clicked, so it takes [`Theme::surface`](crate::Theme#structfield.surface) instead of the
//! menu's page background and a softer shadow, which keeps it from reading as
//! something that can be pressed.
//!
//! # Three ways in, in order of how much the host has to say
//!
//! * [`tooltip_label`] — one line of text, and nothing to decide.
//! * [`Tooltip`] — a short column of parts: lines, a muted note, an image, an
//!   element of the host's own. The host names the parts and this module
//!   arranges them, so two hosts' rich tooltips look like each other.
//! * [`tooltip_with`] — the escape hatch. The host builds the whole inside as
//!   one element and gets nothing but the box around it.
//!
//! All three draw the same box, because all three go through
//! [`tooltip_frame`]. A host that wants the box around something this module
//! has never heard of should reach for `tooltip_frame` rather than restating
//! its padding and its shadow, so that a change to the tooltip's look reaches
//! every tooltip in the application.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, Div, ImageSource, Pixels, SharedString, Window, div, img, prelude::*,
    px,
};

use crate::theme::{Theme, theme};

/// Horizontal padding of the label, in pixels.
const PADDING_X: f32 = 7.;

/// Vertical padding of the label, in pixels.
const PADDING_Y: f32 = 3.;

/// Gap between two parts of a [`Tooltip`], in pixels.
const PART_GAP: f32 = 4.;

/// The box every tooltip in this crate is drawn in.
///
/// The padding, the corner, the surface, the border, the shadow and the type
/// style — everything that makes a tooltip look like a tooltip, and nothing
/// about what is inside it. A host building its own content puts it in here
/// rather than restating any of that:
///
/// ```ignore
/// tooltip_frame(&theme(cx)).child(my_own_element)
/// ```
///
/// The text size and colour are set on the frame rather than on its children,
/// so anything put inside inherits them and a plain string is already styled
/// correctly. What is deliberately *not* here is `whitespace_nowrap`: that
/// belongs to a one-line label, and a frame that forced it on every child would
/// stop a host from ever wrapping anything.
///
/// [`tooltip_with`] and [`Tooltip`] are this function with the contents filled
/// in; use them unless the content is unusual enough to want the frame bare.
pub fn tooltip_frame(theme: &Theme) -> Div {
    div()
        .flex_none()
        .px(px(PADDING_X))
        .py(px(PADDING_Y))
        .rounded_sm()
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .shadow_md()
        .text_size(px(11.))
        .text_color(theme.text)
}

/// Builds the callback `.tooltip` takes, showing `label`.
///
/// ```ignore
/// div().id("save").tooltip(tooltip_label("Save")).child(icon)
/// ```
///
/// The text is captured once and cloned per hover, so the caller can hand over
/// a localised string without keeping it alive itself.
pub fn tooltip_label(
    label: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_window, cx| {
        let label = label.clone();
        cx.new(|_| TooltipLabel { label }).into()
    }
}

/// The one-line tooltip view [`tooltip_label`] constructs.
struct TooltipLabel {
    /// Text shown in the box. Never wrapped: a tooltip that needs two lines is
    /// documentation, and belongs in the guide rather than under the pointer.
    label: SharedString,
}

impl Render for TooltipLabel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tooltip_frame(&theme(cx))
            .whitespace_nowrap()
            .child(self.label.clone())
    }
}

/// What a host hands [`tooltip_with`], and what a [`Tooltip`] keeps its custom
/// parts as.
///
/// An [`Rc`] rather than a `Box`, because the outer closure has to survive
/// being called again: gpui keeps the builder for as long as the element is on
/// screen and asks it for a new view on every hover, so each call needs its own
/// handle on the same code.
type BuildElement = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// Builds the callback `.tooltip` takes, over content the host draws itself.
///
/// The escape hatch under [`tooltip_label`] and [`Tooltip`]: `build` is called
/// once per hover and answers with the whole inside of the box, which is then
/// put in a [`tooltip_frame`] so that the result still looks like every other
/// tooltip in the application.
///
/// ```ignore
/// div()
///     .id("row-3")
///     .tooltip(tooltip_with(|_window, cx| {
///         let palette = theme(cx);
///         div()
///             .flex()
///             .flex_col()
///             .child("public.orders")
///             .child(div().text_color(palette.text_muted).child("12 rows"))
///             .into_any_element()
///     }))
///     .child(row)
/// ```
///
/// `build` is handed the same `Window` and `App` the tooltip is being made in,
/// so it can read a global, measure text, or ask the theme what colour
/// something is — but it must not *keep* anything, since it will be called
/// again for the next hover.
///
/// Reach for [`Tooltip`] first: a column of lines, notes and an image is what
/// nearly every rich tooltip turns out to be, and it does not need a host to
/// hand-style its rows.
pub fn tooltip_with<F>(build: F) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static
where
    F: Fn(&mut Window, &mut App) -> AnyElement + 'static,
{
    let build: BuildElement = Rc::new(build);
    move |_window, cx| {
        let build = build.clone();
        cx.new(|_| TooltipContent { build }).into()
    }
}

/// The view [`tooltip_with`] constructs: a frame, and whatever the host builds.
struct TooltipContent {
    /// The host's content, rebuilt on every render of this view.
    build: BuildElement,
}

impl Render for TooltipContent {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = (self.build)(window, cx);
        tooltip_frame(&theme(cx)).child(content)
    }
}

/// One row of a [`Tooltip`].
#[derive(Clone)]
enum Part {
    /// A line of text in [`Theme::text`](crate::Theme#structfield.text).
    Text(SharedString),
    /// A line of text in [`Theme::text_muted`](crate::Theme#structfield.text_muted).
    Note(SharedString),
    /// A picture, at a fixed width.
    Image {
        /// Where the picture comes from.
        source: ImageSource,
        /// How wide to draw it; the height follows the aspect ratio.
        width: Pixels,
    },
    /// Anything else, built by the host on every hover.
    Element(BuildElement),
}

/// A tooltip built out of a few named parts.
///
/// The step between [`tooltip_label`] and [`tooltip_with`]. A rich tooltip is
/// nearly always a short column — a title, a muted caption, sometimes a
/// thumbnail or a snippet of code — and this builds that column so that a host
/// does not hand-style its rows and two hosts' tooltips do not drift apart.
///
/// ```ignore
/// div()
///     .id("orders-preview")
///     .tooltip(
///         Tooltip::new()
///             .image("icons/preview.svg", px(96.))
///             .text("public.orders")
///             .note("12 rows")
///             .build(),
///     )
///     .child("orders")
/// ```
///
/// Parts are drawn top to bottom in the order they were added, so the calls
/// above are the layout. There is no separate "title" part: the first
/// [`Tooltip::text`] is the title because it is first.
///
/// # State the host keeps
///
/// None, as with [`tooltip_label`]. `Tooltip` is [`Clone`] and cheap to clone —
/// a `Vec` of parts, each of them a shared handle — and [`Tooltip::build`]
/// consumes one into the closure gpui stores, which then rebuilds the column on
/// every hover. A host that shows the same tooltip on several elements clones
/// the builder rather than writing it twice.
#[derive(Clone, Default)]
pub struct Tooltip {
    /// The rows, in the order they will be drawn.
    parts: Vec<Part>,
    /// A cap on the width of the column, if the host set one.
    max_width: Option<Pixels>,
}

impl Tooltip {
    /// An empty tooltip. Add parts to it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a line of text in [`Theme::text`](crate::Theme#structfield.text).
    ///
    /// Never wrapped, like [`tooltip_label`]'s: a line that needs to wrap is
    /// documentation and belongs in a guide. Use several `text` calls for
    /// several lines.
    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.parts.push(Part::Text(text.into()));
        self
    }

    /// Adds a line of text in [`Theme::text_muted`](crate::Theme#structfield.text_muted).
    ///
    /// For the caption under a title, a row count, a hint about what pressing
    /// the control will do — anything the eye should reach second. Also never
    /// wrapped.
    pub fn note(mut self, note: impl Into<SharedString>) -> Self {
        self.parts.push(Part::Note(note.into()));
        self
    }

    /// Adds a picture `width` wide; its height follows its aspect ratio.
    ///
    /// `source` is anything gpui's [`img`] takes. A `&'static str` or a
    /// [`SharedString`] that does not parse as a URL becomes an *embedded*
    /// resource, which gpui resolves through the application's
    /// [`AssetSource`](gpui::AssetSource) — the same path a widget's icon takes
    /// — while one that does parse as a URL is fetched over HTTP; a `PathBuf`
    /// reads from disk, and a decoded [`Image`](gpui::Image) skips loading
    /// altogether.
    ///
    /// Note that [`img`] is not [`svg`](gpui::svg). The `svg` element throws a
    /// file's colours away and keeps only its coverage, so an icon drawn with
    /// it takes the element's `text_color`; `img` rasterises the same file with
    /// the colours that are written in it. A thumbnail in a tooltip wants the
    /// second, which is why this part is an `img`.
    pub fn image(mut self, source: impl Into<ImageSource>, width: Pixels) -> Self {
        self.parts.push(Part::Image {
            source: source.into(),
            width,
        });
        self
    }

    /// Adds an element the host builds itself, on every hover.
    ///
    /// The way anything this module has never heard of gets into the column —
    /// a highlighted snippet
    /// (`rugpui-editor`'s `CodeSnippet`), a swatch, a tiny
    /// chart. `build` is called with the `Window` and `App` the tooltip is
    /// being made in and must not keep anything between calls.
    pub fn element<F>(mut self, build: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> AnyElement + 'static,
    {
        self.parts.push(Part::Element(Rc::new(build)));
        self
    }

    /// Caps the width of the column.
    ///
    /// Off by default, which lets the box be as wide as its widest part. The
    /// cap bounds what can be *measured* — an image, a snippet, a host element
    /// — and not the text lines, which never wrap; a `text` longer than the cap
    /// still draws at full width. Set it when a part could be arbitrarily wide
    /// and the tooltip should not follow it across the screen.
    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Turns the builder into the callback `.tooltip` takes.
    ///
    /// The parts are laid out as a column inside a [`tooltip_frame`], so the
    /// result sits beside a [`tooltip_label`] without looking like a different
    /// kind of thing.
    pub fn build(self) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        tooltip_with(move |window, cx| {
            let palette = theme(cx);
            let mut column = div()
                .flex()
                .flex_col()
                .gap(px(PART_GAP))
                .when_some(self.max_width, |this, width| this.max_w(width));
            for part in &self.parts {
                column = column.child(match part {
                    Part::Text(text) => div()
                        .whitespace_nowrap()
                        .child(text.clone())
                        .into_any_element(),
                    Part::Note(note) => div()
                        .whitespace_nowrap()
                        .text_color(palette.text_muted)
                        .child(note.clone())
                        .into_any_element(),
                    Part::Image { source, width } => {
                        img(source.clone()).w(*width).into_any_element()
                    }
                    Part::Element(build) => build(window, cx),
                });
            }
            column.into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::ops::Deref;
    use std::time::Duration;

    use gpui::{
        Bounds, Modifiers, Point, Size, TestAppContext, VisualTestContext, canvas, point, size,
    };

    /// A window with one tooltip view in it, laid out the way gpui would lay
    /// it out under the pointer.
    struct Harness {
        /// The view the tooltip builder produced.
        view: AnyView,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.view.clone())
        }
    }

    /// Opens a window over the view `builder` makes and draws two frames.
    ///
    /// Everything here is a "does it lay out at all" test: a tooltip has no
    /// state and no input, so what can go wrong is a run that does not tile its
    /// text or an element that panics in prepaint, and both need a real frame
    /// to show up.
    fn draw(
        builder: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
        cx: &mut TestAppContext,
    ) -> VisualTestContext {
        cx.update(crate::init);
        let window = cx.add_window(|window, cx| Harness {
            view: builder(window, cx),
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        cx.refresh().expect("the window redraws");
        cx.run_until_parked();
        cx
    }

    #[test]
    fn parts_are_kept_in_the_order_they_were_added() {
        let tooltip = Tooltip::new()
            .text("public.orders")
            .image("icons/preview.svg", px(96.))
            .note("12 rows")
            .element(|_window, _cx| div().into_any_element())
            .max_width(px(240.));

        assert_eq!(tooltip.parts.len(), 4);
        assert!(matches!(&tooltip.parts[0], Part::Text(text) if text == "public.orders"));
        assert!(matches!(&tooltip.parts[1], Part::Image { width, .. } if *width == px(96.)));
        assert!(matches!(&tooltip.parts[2], Part::Note(note) if note == "12 rows"));
        assert!(matches!(tooltip.parts[3], Part::Element(_)));
        assert_eq!(tooltip.max_width, Some(px(240.)));
    }

    #[test]
    fn an_empty_tooltip_has_no_parts_and_no_cap() {
        let tooltip = Tooltip::new();
        assert!(tooltip.parts.is_empty());
        assert_eq!(tooltip.max_width, None);

        // Cloning is what the per-hover closure does, and it has to keep the
        // parts rather than the identity of the builder.
        let clone = tooltip.text("one").clone();
        assert_eq!(clone.parts.len(), 1);
    }

    #[gpui::test]
    fn a_label_tooltip_draws(cx: &mut TestAppContext) {
        draw(tooltip_label("Rests here to show a tooltip"), cx);
    }

    #[gpui::test]
    fn a_custom_tooltip_draws(cx: &mut TestAppContext) {
        draw(
            tooltip_with(|_window, cx| {
                let palette = theme(cx);
                div()
                    .text_color(palette.text_muted)
                    .child("built by the host")
                    .into_any_element()
            }),
            cx,
        );
    }

    #[gpui::test]
    fn a_composite_tooltip_draws(cx: &mut TestAppContext) {
        draw(
            Tooltip::new()
                .text("public.orders")
                .note("12 rows")
                .element(|_window, _cx| div().child("select 1").into_any_element())
                .max_width(px(240.))
                .build(),
            cx,
        );
    }

    /// The parts builder is used up by `build`, so a host that shows the same
    /// tooltip twice clones it — and both clones have to work.
    #[gpui::test]
    fn a_cloned_builder_draws_twice(cx: &mut TestAppContext) {
        let tooltip = Tooltip::new().text("shared").note("by two elements");
        cx.update(crate::init);
        let first = tooltip.clone().build();
        let second = tooltip.build();
        let window = cx.add_window(|window, cx| Harness {
            view: first(window, cx),
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = second(window, cx);
        });
        cx.refresh().expect("the window redraws");
        cx.run_until_parked();
    }

    // --- placement ----------------------------------------------------------

    /// Long enough for gpui's hover delay to run out.
    const REST: Duration = Duration::from_millis(600);

    /// The window most placement tests rest the pointer in.
    const WINDOW: Size<Pixels> = size(px(800.), px(600.));

    /// A one-line tooltip's size, near enough.
    const LINE: Size<Pixels> = size(px(120.), px(22.));

    /// What the arrow cursor covers with its hot spot at `at`.
    ///
    /// Measured from the Breeze and Adwaita themes: at the usual 24 px size the
    /// arrow reaches a pixel above and left of its tip, 12 px right and 21 px
    /// down; this is wide enough for Breeze at 30 px, which reaches 15 right
    /// and 23 down.
    fn cursor(at: Point<Pixels>) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(at.x - px(2.), at.y - px(2.)),
            point(at.x + px(16.), at.y + px(24.)),
        )
    }

    /// The tooltip the placement tests hover into view: a frame of a fixed
    /// size, with a canvas over its inside that writes down where it was put.
    struct Probe {
        size: Size<Pixels>,
        seen: Rc<Cell<Option<Bounds<Pixels>>>>,
    }

    impl Render for Probe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            tooltip_frame(&theme(cx))
                .relative()
                .w(self.size.width)
                .h(self.size.height)
                .child(
                    canvas(move |bounds, _, _| seen.set(Some(bounds)), |_, _, _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                )
        }
    }

    /// A window that is one element with a [`Probe`] tooltip on it.
    struct Hovered {
        size: Size<Pixels>,
        seen: Rc<Cell<Option<Bounds<Pixels>>>>,
    }

    impl Render for Hovered {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let (size, seen) = (self.size, self.seen.clone());
            div().id("hovered").size_full().tooltip(move |_window, cx| {
                let seen = seen.clone();
                cx.new(|_| Probe { size, seen }).into()
            })
        }
    }

    /// Rests the pointer at `mouse` in a fresh `window`-sized window until a
    /// `tooltip`-sized tip shows, and answers the box that was drawn and the
    /// content bounds it had to stay in.
    ///
    /// A fresh window each time, because gpui keeps a tooltip where it first
    /// showed for as long as the pointer stays on the element. `inset` is the
    /// shadow band of a client-decorated window.
    fn rest(
        cx: &mut TestAppContext,
        window: Size<Pixels>,
        inset: Option<Pixels>,
        mouse: Point<Pixels>,
        tooltip: Size<Pixels>,
    ) -> (Bounds<Pixels>, Bounds<Pixels>) {
        let seen = Rc::new(Cell::new(None));
        let handle = cx.add_window({
            let seen = seen.clone();
            move |_, _| Hovered {
                size: tooltip,
                seen,
            }
        });
        let mut cx = VisualTestContext::from_window(*handle.deref(), cx);
        cx.simulate_resize(window);
        let content = cx.update(|window, _| {
            if let Some(inset) = inset {
                window.set_client_inset(inset);
            }
            window.client_content_bounds()
        });
        cx.run_until_parked();

        cx.simulate_mouse_move(mouse, None, Modifiers::none());
        cx.executor().advance_clock(REST);
        cx.run_until_parked();

        // The canvas covers the inside of the frame's one-pixel border.
        let frame = seen.get().expect("the tooltip never showed").dilate(px(1.));
        assert_eq!(frame.size, tooltip, "the probe did not draw at its size");
        (frame, content)
    }

    /// Asserts that `frame` is out from under the cursor at `mouse` and
    /// inside `content`.
    fn assert_clear(frame: Bounds<Pixels>, mouse: Point<Pixels>, content: Bounds<Pixels>) {
        assert!(
            !frame.intersects(&cursor(mouse)),
            "the tooltip {frame:?} is under the cursor at {mouse:?}"
        );
        assert!(
            frame.left() >= content.left()
                && frame.top() >= content.top()
                && frame.right() <= content.right()
                && frame.bottom() <= content.bottom(),
            "the tooltip {frame:?} left the content {content:?}"
        );
    }

    #[gpui::test]
    fn a_tooltip_sits_below_the_cursor_and_not_under_it(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mouse = point(px(200.), px(200.));
        let (frame, content) = rest(cx, WINDOW, None, mouse, LINE);
        assert_clear(frame, mouse, content);
        assert!(
            frame.top() > mouse.y,
            "the tooltip is not below the pointer"
        );
        assert!(
            frame.top() - mouse.y < px(32.),
            "the tooltip is too far below the pointer to read as its own"
        );
        assert_eq!(
            frame.left(),
            mouse.x,
            "the tooltip is not under the pointer"
        );
    }

    #[gpui::test]
    fn at_the_foot_of_the_window_a_tooltip_flips_above_the_pointer(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for mouse in [
            point(px(200.), WINDOW.height - px(10.)),
            // The bottom right corner: flipped up and slid left at once.
            point(WINDOW.width - px(5.), WINDOW.height - px(5.)),
        ] {
            let (frame, content) = rest(cx, WINDOW, None, mouse, LINE);
            assert_clear(frame, mouse, content);
            assert!(
                frame.bottom() < mouse.y,
                "the tooltip at {mouse:?} did not go above the pointer"
            );
        }
    }

    #[gpui::test]
    fn at_the_right_edge_a_tooltip_slides_in_and_stays_below(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let mouse = point(WINDOW.width - px(10.), px(200.));
        let (frame, content) = rest(cx, WINDOW, None, mouse, LINE);
        assert_clear(frame, mouse, content);
        assert!(frame.top() > mouse.y, "the tooltip left the space below");
        assert_eq!(
            frame.right(),
            WINDOW.width,
            "the tooltip slid further than it had to"
        );
    }

    #[gpui::test]
    fn a_tooltip_too_tall_for_either_side_stands_beside_the_cursor(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = size(px(400.), px(200.));
        let tall = size(px(120.), px(150.));
        for mouse in [
            point(px(100.), px(100.)),
            // No room right of the arrow: the box goes left of the hot spot.
            point(px(390.), px(100.)),
        ] {
            let (frame, content) = rest(cx, window, None, mouse, tall);
            assert_clear(frame, mouse, content);
        }
    }

    #[gpui::test]
    fn a_tooltip_stays_out_of_a_client_decorated_window_s_shadow(cx: &mut TestAppContext) {
        cx.update(crate::init);
        // Near the foot of the content but well clear of the surface's: placed
        // against the surface, the box would land in the shadow band and be
        // clipped there.
        let inset = px(30.);
        let mouse = point(px(200.), WINDOW.height - inset - px(10.));
        let (frame, content) = rest(cx, WINDOW, Some(inset), mouse, LINE);
        assert_eq!(content.bottom(), WINDOW.height - inset);
        assert_clear(frame, mouse, content);
        assert!(
            frame.bottom() < mouse.y,
            "the tooltip did not go above the pointer"
        );
    }
}
