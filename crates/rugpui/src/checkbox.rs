//! A labeled on/off checkbox.

use gpui::{
    App, ClickEvent, ElementId, SharedString, TextStyleRefinement, Window, div, prelude::*, px,
};

use crate::scale::TextSize;
use crate::theme::theme;

/// Glyph painted inside the box while it is checked.
const CHECK_GLYPH: &str = "\u{2713}";

/// Callback fired with the value the checkbox is about to take.
type ToggleHandler = Box<dyn Fn(bool, &mut Window, &mut App)>;

/// A stateless checkbox with a clickable label.
///
/// Like [`Button`](crate::Button) it owns no state: the parent view passes the
/// current value on every render and updates its own state from
/// [`Checkbox::on_toggle`], which receives the *new* value. Clicking anywhere on
/// the row — box or label — toggles it.
///
/// ```ignore
/// Checkbox::new("remember", "Remember password")
///     .checked(self.remember)
///     .on_toggle(cx.processor(|this, checked, _window, cx| {
///         this.remember = checked;
///         cx.notify();
///     }))
/// ```
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: SharedString,
    checked: bool,
    tab_index: Option<isize>,
    on_toggle: Option<ToggleHandler>,
}

impl Checkbox {
    /// Creates an unchecked checkbox.
    ///
    /// `id` must be unique among the siblings of the checkbox.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked: false,
            tab_index: Some(0),
            on_toggle: None,
        }
    }

    /// Sets whether the box is ticked.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Places the checkbox at `index` in the window's tab order.
    ///
    /// A focused checkbox draws an accent outline and toggles on `Space` or
    /// `Enter`, which gpui delivers as an ordinary click.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Sets the callback invoked with the value the checkbox is toggling to.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let checked = self.checked;
        let next = !checked;
        let text_size = TextSize::Body.size(cx);

        let (box_bg, box_border, glyph_color) = if checked {
            (theme.accent, theme.accent, theme.background)
        } else {
            (theme.surface, theme.border, gpui::transparent_black())
        };

        let mark = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded_sm()
            .border_1()
            .border_color(box_border)
            .bg(box_bg)
            .text_size(TextSize::Xs.size(cx))
            .text_color(glyph_color)
            .child(if checked { CHECK_GLYPH } else { "" });
        // A label wraps, and the box stays beside its first line.
        let mark = first_line(mark, text_size, window);

        div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_start()
            // Lets the row be narrower than its label on one line, which is
            // what makes the label wrap rather than run past its container.
            .min_w_0()
            .gap(px(8.))
            .py(px(1.))
            .rounded_sm()
            // Transparent until focused, so the ring costs no layout.
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .text_size(text_size)
            .text_color(theme.text)
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
            })
            .when_some(self.on_toggle, |this, handler| {
                this.on_click(move |_: &ClickEvent, window, cx| handler(next, window, cx))
            })
            .child(mark)
            .child(div().min_w_0().child(self.label))
    }
}

/// `mark` centered on the first line of a label drawn at `text_size`.
///
/// The row aligns its children to the top, so that the box or the track stays
/// beside the first line of a label that wraps instead of drifting to the
/// middle of the paragraph; this box is one line tall, which puts the mark
/// where centering it on a one-line label always did.
pub(crate) fn first_line(
    mark: impl IntoElement,
    text_size: gpui::Pixels,
    window: &mut Window,
) -> impl IntoElement {
    let line_height = window.with_text_style(
        Some(TextStyleRefinement {
            font_size: Some(text_size.into()),
            ..TextStyleRefinement::default()
        }),
        |window| window.line_height(),
    );
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(line_height)
        .child(mark)
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestAppContext, VisualTestContext, size};

    use super::*;

    /// Long enough to need several lines in [`NARROW`].
    const LONG: &str = "Include this connection's custom variables, so the templates render the same on another computer";

    /// Width of the box the checkbox is laid out in.
    const NARROW: f32 = 160.;

    /// A checkbox labeled `label` in a box [`NARROW`] wide, as the only child
    /// of a column or of a row.
    struct Narrow {
        label: &'static str,
        row: bool,
    }

    impl Render for Narrow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let container = div()
                .flex()
                .w(px(NARROW))
                .debug_selector(|| "box".to_string());
            let container = if self.row {
                container.flex_row()
            } else {
                container.flex_col()
            };
            div()
                .size_full()
                .child(container.child(Checkbox::new("check", self.label)))
        }
    }

    fn laid_out(
        label: &'static str,
        row: bool,
        cx: &mut TestAppContext,
    ) -> gpui::Bounds<gpui::Pixels> {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| Narrow { label, row });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(size(px(600.), px(400.)));
        cx.run_until_parked();
        cx.debug_bounds("box").expect("the box is drawn")
    }

    /// A label longer than the room it has wraps onto more lines, in a column
    /// and in a row alike, rather than running past its container.
    #[gpui::test]
    fn a_long_label_wraps_in_a_narrow_box(cx: &mut TestAppContext) {
        for row in [false, true] {
            let short = laid_out("Remember", row, cx);
            let long = laid_out(LONG, row, cx);
            assert_eq!(long.size.width, px(NARROW));
            assert!(
                long.size.height > short.size.height * 2.,
                "the label did not wrap (row: {row}): {short:?} vs {long:?}"
            );
        }
    }

    /// A label that fits keeps the one-line row it always had, and so does a
    /// checkbox with no label at all — a table cell.
    #[gpui::test]
    fn a_label_that_fits_keeps_one_line(cx: &mut TestAppContext) {
        let short = laid_out("Remember", false, cx);
        let bare = laid_out("", false, cx);
        assert_eq!(bare.size.height, short.size.height);
        // One line of body text, the row's padding and the focus border.
        assert!(short.size.height < px(30.), "{short:?}");
    }
}
