//! A titled section that folds its body away.
//!
//! A disclosure arrow, a title, and a body that is either drawn or is not
//! there at all. Every application built on this kit grows one sooner or
//! later — a connection dialog's "SSH tunnel" block, a terminal profile's
//! "Session overrides" — and each of them used to be hand-built out of a row,
//! a `bool` and a `then(..)`, which is three lines of layout repeated once per
//! optional block in the product.
//!
//! Like the rest of the kit it is stateless: the host owns the `bool`, passes
//! it in through [`Collapsible::open`] on every render, and writes the new one
//! back from [`Collapsible::on_toggle`], which receives the value the section
//! is about to take.
//!
//! ## Closed means *not rendered*
//!
//! A closed section drops its children rather than hiding them. The difference
//! matters more than a hidden box costing a layout pass: gpui keeps a focus
//! handle alive for as long as something in the tree tracks it, so a fold-away
//! block full of [`TextInput`](crate::TextInput)s that merely *hid* would go on
//! holding the caret, go on taking the tab ring's stops, and go on answering
//! keys typed at a section the user can no longer see. Not drawing it is what
//! makes folding it away mean anything.
//!
//! The entities themselves are the host's and outlive the fold — it is only
//! their elements that come and go — so a field's contents survive a section
//! being closed and opened again. What does not survive is focus, which is the
//! intended half of the bargain.
//!
//! ## Why the trailing slot does not toggle
//!
//! Section headers grow a control at the far end: a [`Switch`](crate::Switch)
//! that turns the whole block on, a count of what is inside, a button. That
//! control is a second target, not part of the first, so it sits *beside* the
//! clickable header rather than inside it — a press on it never reaches the
//! disclosure. A switch nested inside the header would flip the block off and
//! fold the section at the same time, which is one gesture doing two things
//! the user only asked one of.

use gpui::{
    AnyElement, App, ClickEvent, ElementId, SharedString, Window, div, prelude::*, px, svg,
};

use crate::icons::{CARET_DOWN, CARET_RIGHT};
use crate::theme::theme;

/// Width of the box the disclosure arrow is drawn in.
///
/// Also what the body is indented by, so its content starts under the title
/// rather than under the arrow.
///
/// The two arrow constants here are deliberately a second copy of the ones in
/// [`tree`](crate::tree): the two widgets have to draw the same chevron at the
/// same size to read as the same idea, and neither is worth making depend on
/// the other for two numbers.
const ARROW_WIDTH: f32 = 16.;

/// Edge length of the arrow icon.
///
/// Nearly the full width of the [`ARROW_WIDTH`] box rather than inset into it:
/// a drawn chevron carries its own margin inside its viewBox, so running it
/// edge to edge here is what makes it the size it looks.
const ARROW_ICON_SIZE: f32 = 14.;

/// Padding above and below the header, inside its clickable target.
///
/// Enough to give the row a hit area taller than the text and to keep the hover
/// wash from hugging the letters, and no more: a section header is a line in a
/// form, not a band across it.
const HEADER_PADDING: f32 = 4.;

/// Gap between the body's own children, and between the header and the body.
const BODY_GAP: f32 = 6.;

/// Callback fired with the value the section is about to take.
type ToggleHandler = Box<dyn Fn(bool, &mut Window, &mut App)>;

/// A stateless fold-away section: a header that discloses its children.
///
/// The children are the body — `.child(..)` and `.children(..)` fill it — and
/// they are rendered only while the section is open. See the module docs for
/// why they are dropped rather than hidden.
///
/// ```ignore
/// Collapsible::new("tunnel", "SSH tunnel")
///     .open(self.tunnel_open)
///     .on_toggle(cx.processor(|this, open, _window, cx| {
///         this.tunnel_open = open;
///         cx.notify();
///     }))
///     .trailing(Switch::new("tunnel-on", "").checked(self.tunnel_enabled))
///     .child(form_row("Host", self.tunnel_host.clone()))
///     .child(form_row("Port", self.tunnel_port.clone()))
/// ```
#[derive(IntoElement)]
pub struct Collapsible {
    id: ElementId,
    title: SharedString,
    open: bool,
    indent: bool,
    disabled: bool,
    tab_index: Option<isize>,
    arrow_icons: Option<(SharedString, SharedString)>,
    trailing: Option<AnyElement>,
    children: Vec<AnyElement>,
    on_toggle: Option<ToggleHandler>,
}

impl Collapsible {
    /// Creates a closed section titled `title`.
    ///
    /// `id` must be unique among the siblings of the section.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            open: false,
            indent: true,
            disabled: false,
            tab_index: None,
            arrow_icons: None,
            trailing: None,
            children: Vec::new(),
            on_toggle: None,
        }
    }

    /// Sets whether the body is showing.
    ///
    /// Defaults to closed. A closed section does not render its children at
    /// all; see the module docs for what that means for entities inside it.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Puts `element` at the far end of the header, beside the disclosure.
    ///
    /// A switch that arms the whole block, a checkbox, a button, a count. It is
    /// a sibling of the clickable header rather than a child of it, so clicking
    /// it does not fold the section.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }

    /// Draws the disclosure with the host's own icons instead of
    /// [`CARET_RIGHT`]/[`CARET_DOWN`].
    ///
    /// Both paths are resolved by the application's `AssetSource` and are
    /// painted in `theme.icon`, so one monochrome pair follows the palette. The
    /// same two icons a [`TreeView`](crate::TreeView) is given are the ones to
    /// hand over here: a form's sections and a tree's branches disclose the same
    /// way and should not disagree about which way the chevron points.
    pub fn arrow_icons(
        mut self,
        closed: impl Into<SharedString>,
        open: impl Into<SharedString>,
    ) -> Self {
        self.arrow_icons = Some((closed.into(), open.into()));
        self
    }

    /// Places the header at `index` in the window's tab order.
    ///
    /// A focused header draws an accent outline and folds on `Space` or
    /// `Enter`, which gpui delivers as an ordinary click. A disabled section
    /// stays out of the ring entirely, as there is nothing there to activate.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets whether the body's content lines up with the title.
    ///
    /// On by default: the body is padded left by the width of the arrow box, so
    /// a form inside a section reads as belonging to its heading rather than
    /// starting a column of its own. Turn it off for a body that draws its own
    /// frame — a panel, a table — where a second indent would only be a step.
    pub fn indent(mut self, indent: bool) -> Self {
        self.indent = indent;
        self
    }

    /// Greys the header and stops it answering presses.
    ///
    /// The body is still drawn if [`Collapsible::open`] says it is open: a
    /// section can be frozen open as easily as frozen shut, and which of the
    /// two it is remains the host's to say.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the callback invoked with the value the section is folding to.
    ///
    /// Like [`Checkbox::on_toggle`](crate::Checkbox::on_toggle) it is handed the
    /// *next* value, so a host never has to write the `!` itself.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl ParentElement for Collapsible {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Collapsible {
    /// A column of one or two rows: the header always, the body only while open.
    ///
    /// The header is itself two elements — the clickable disclosure and the
    /// trailing slot beside it — because they are two targets and gpui has no
    /// way to carve one element into two hit areas. The disclosure takes
    /// `flex_1` so the whole width up to the trailing control answers a press:
    /// a header the user can only fold by hitting the triangle is a header with
    /// a 16 px target.
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = theme(cx);
        let open = self.open;
        let disabled = self.disabled;
        let next = !open;

        let (title_color, icon_color) = if disabled {
            (palette.text_muted, palette.text_muted)
        } else {
            (palette.text, palette.icon)
        };

        let (closed, opened) = match &self.arrow_icons {
            Some((closed, opened)) => (closed.clone(), opened.clone()),
            None => (CARET_RIGHT.into(), CARET_DOWN.into()),
        };
        let mark = svg()
            .size(px(ARROW_ICON_SIZE))
            .flex_none()
            .path(if open { opened } else { closed })
            // An SVG takes its tint from the element itself; unlike text it
            // does not inherit the one the box around it sets.
            .text_color(icon_color);

        let arrow = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(ARROW_WIDTH))
            .child(mark);

        let header = div()
            .id(self.id)
            .flex()
            .flex_row()
            .flex_1()
            .min_w_0()
            .items_center()
            .py(px(HEADER_PADDING))
            .rounded_sm()
            // Transparent until focused, so the ring costs no layout.
            .border_1()
            .border_color(gpui::transparent_black())
            .text_color(title_color)
            .child(arrow)
            .child(div().min_w_0().child(self.title))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(palette.surface_hover))
                    .when_some(self.tab_index, |this, index| {
                        let accent = palette.accent;
                        this.tab_index(index)
                            .focus(move |style| style.border_color(accent))
                    })
                    .when_some(self.on_toggle, |this, handler| {
                        this.on_click(move |_: &ClickEvent, window, cx| handler(next, window, cx))
                    })
            });

        // Built before the column, so that a closed section's children are
        // dropped here rather than handed to a hidden box.
        let body = open.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(BODY_GAP))
                .pt(px(BODY_GAP))
                .when(self.indent, |this| this.pl(px(ARROW_WIDTH)))
                .children(self.children)
        });

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(HEADER_PADDING))
                    .child(header)
                    .children(self.trailing),
            )
            .children(body)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ops::Deref;
    use std::rc::Rc;

    use gpui::{
        Context, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, Render, TestAppContext,
        VisualTestContext, point, size,
    };

    use super::*;

    /// Width of the window every test here runs in.
    ///
    /// Wide enough that the header, which is `flex_1`, and the trailing control
    /// beside it are far apart in x — the two targets are told apart by where
    /// the press lands and nothing else.
    const HARNESS_WIDTH: f32 = 300.;

    /// Height of the same window.
    const HARNESS_HEIGHT: f32 = 200.;

    /// A row comfortably inside the header, and inside the shorter trailing
    /// control centred in it, whatever font the platform picks.
    const ON_THE_HEADER: f32 = 14.;

    /// A column inside the header's clickable half, past the arrow box.
    const IN_THE_TITLE: f32 = 40.;

    /// A column inside the trailing control, which rides the right edge.
    const ON_THE_TRAILING: f32 = HARNESS_WIDTH - 10.;

    /// Something in the body that says out loud when it is drawn.
    ///
    /// A body that is merely hidden still renders; one that is not there does
    /// not. Counting renders is the only way to tell those apart from outside,
    /// since both leave the same nothing on screen.
    #[derive(IntoElement)]
    struct Probe {
        drawn: Rc<Cell<usize>>,
    }

    impl RenderOnce for Probe {
        fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
            self.drawn.set(self.drawn.get() + 1);
            div().size_full().child("body")
        }
    }

    /// What the section under test reported, and what it drew.
    #[derive(Clone, Default)]
    struct Tally {
        /// Values `on_toggle` was handed, in order.
        toggles: Rc<std::cell::RefCell<Vec<bool>>>,
        /// How many times the body was rendered.
        drawn: Rc<Cell<usize>>,
        /// How many times the trailing control was clicked.
        trailing: Rc<Cell<usize>>,
    }

    /// One section in a window, in whichever of its states the test asks for.
    ///
    /// The host keeps `open` here exactly as a real one would, but does *not*
    /// write the reported value back: every test wants to see what the widget
    /// asked for, and a harness that folded itself would hide a second toggle
    /// behind the first.
    struct Harness {
        open: bool,
        disabled: bool,
        focusable: bool,
        tally: Tally,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let Tally {
                toggles,
                drawn,
                trailing,
            } = self.tally.clone();

            // A column, so that the section stretches across the window and
            // the trailing control ends up on the right edge where the tests
            // aim for it.
            div().size_full().flex_col().child(
                Collapsible::new("section", "Advanced options")
                    .open(self.open)
                    .disabled(self.disabled)
                    .when(self.focusable, |this| this.tab_index(0))
                    .trailing(
                        div()
                            .id("trailing")
                            .w(px(24.))
                            .h(px(16.))
                            .child("x")
                            .on_click(move |_, _, _| trailing.set(trailing.get() + 1)),
                    )
                    .on_toggle(move |value, _window, _cx| toggles.borrow_mut().push(value))
                    .child(Probe { drawn }),
            )
        }
    }

    /// Opens a window on one section and hands back what it reports.
    fn open_section(
        cx: &mut TestAppContext,
        open: bool,
        disabled: bool,
        focusable: bool,
    ) -> (Tally, VisualTestContext) {
        cx.update(crate::init);

        let tally = Tally::default();
        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), {
            let tally = tally.clone();
            move |_, _| Harness {
                open,
                disabled,
                focusable,
                tally,
            }
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();

        (tally, cx)
    }

    /// Clicks a column of the header row, hovering first so that anything
    /// revealed by a hover is there when the press lands.
    fn click(cx: &mut VisualTestContext, x: f32) {
        let position = point(px(x), px(ON_THE_HEADER));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.simulate_click(position, Modifiers::none());
        cx.run_until_parked();
    }

    /// A closed section's children are never rendered — not hidden, not laid
    /// out, not there. This is the whole point of folding one away: anything
    /// inside would otherwise go on holding focus and go on taking tab stops
    /// while invisible.
    #[gpui::test]
    fn a_closed_section_never_renders_its_body(cx: &mut TestAppContext) {
        let (tally, _cx) = open_section(cx, false, false, false);

        assert_eq!(
            tally.drawn.get(),
            0,
            "a closed section drew the body it was supposed to have dropped"
        );
    }

    /// Open, the same body is drawn. The header is still there in both states,
    /// so the difference is the body alone.
    #[gpui::test]
    fn an_open_section_renders_its_body(cx: &mut TestAppContext) {
        let (tally, _cx) = open_section(cx, true, false, false);

        assert!(
            tally.drawn.get() >= 1,
            "an open section never drew its body"
        );
    }

    /// A press anywhere on the disclosure — the arrow, the title, the empty
    /// width after it — asks for the other state, and asks for it once.
    #[gpui::test]
    fn clicking_the_header_asks_for_the_other_state(cx: &mut TestAppContext) {
        let (closed, mut closed_cx) = open_section(cx, false, false, false);

        click(&mut closed_cx, IN_THE_TITLE);
        assert_eq!(
            closed.toggles.borrow().as_slice(),
            &[true],
            "a closed section asked for something other than opening"
        );

        // And the arrow box, at the very start of the row, is the same target
        // rather than a second one.
        click(&mut closed_cx, ARROW_WIDTH / 2.);
        assert_eq!(closed.toggles.borrow().as_slice(), &[true, true]);
    }

    /// The value handed over is the next one and never the one already
    /// showing, so an open section asks to close.
    #[gpui::test]
    fn an_open_section_asks_to_close(cx: &mut TestAppContext) {
        let (tally, mut cx) = open_section(cx, true, false, false);

        click(&mut cx, IN_THE_TITLE);

        assert_eq!(tally.toggles.borrow().as_slice(), &[false]);
    }

    /// The trailing slot is a second target, not part of the first: a switch at
    /// the end of a header must not fold the section it arms.
    #[gpui::test]
    fn clicking_the_trailing_element_does_not_fold(cx: &mut TestAppContext) {
        let (tally, mut cx) = open_section(cx, false, false, false);

        click(&mut cx, ON_THE_TRAILING);

        assert_eq!(
            tally.trailing.get(),
            1,
            "the trailing control never saw the click"
        );
        assert!(
            tally.toggles.borrow().is_empty(),
            "a press on the trailing control folded the section as well"
        );
    }

    /// A disabled header answers nothing, wherever it is pressed — and the
    /// trailing control beside it is untouched by that, since it is the host's
    /// element and the host disables it if it means to.
    #[gpui::test]
    fn a_disabled_header_does_not_fold(cx: &mut TestAppContext) {
        let (tally, mut cx) = open_section(cx, false, true, false);

        click(&mut cx, IN_THE_TITLE);
        click(&mut cx, ARROW_WIDTH / 2.);

        assert!(
            tally.toggles.borrow().is_empty(),
            "a disabled section folded anyway"
        );
    }

    /// With a `tab_index` the header is a tab stop, and gpui turns a `Space`
    /// press and release on a focused element into an ordinary click — so the
    /// one `on_click` covers the pointer and the keyboard both.
    #[gpui::test]
    fn a_focused_header_folds_from_the_keyboard(cx: &mut TestAppContext) {
        let (tally, mut cx) = open_section(cx, false, false, true);

        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();

        let keystroke = Keystroke::parse("space").expect("space parses");
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        cx.run_until_parked();

        assert_eq!(
            tally.toggles.borrow().as_slice(),
            &[true],
            "the focused header did not answer the space bar"
        );
    }
}
