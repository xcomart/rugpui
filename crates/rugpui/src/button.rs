//! A stateless push button.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, FocusHandle, Hsla, SharedString, TextRun, Window, div, prelude::*,
    px,
};

use crate::scale::{TextSize, scaled};
use crate::theme::{shift_lightness, theme};
use crate::tooltip::tooltip_label;

/// Visual weight of a [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Filled with the accent color; use for the single main action of a view.
    #[default]
    Primary,
    /// Filled with a neutral surface color and outlined.
    Secondary,
    /// No background until hovered; use inside dense toolbars.
    Ghost,
    /// Filled with the danger color; use for destructive actions.
    Danger,
}

/// The resolved colors of one button state.
struct ButtonColors {
    background: Hsla,
    hover: Hsla,
    active: Hsla,
    label: Hsla,
    border: Option<Hsla>,
}

/// Callback fired when a [`Button`] is clicked.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A stateless button element.
///
/// `Button` owns no state, so it is rebuilt on every render of its parent:
///
/// ```ignore
/// Button::new("connect", "Connect")
///     .variant(ButtonVariant::Primary)
///     .on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
/// ```
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    disabled: bool,
    full_width: bool,
    compact: bool,
    tab_index: Option<isize>,
    focus_handle: Option<FocusHandle>,
    truncate: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// Creates a [`ButtonVariant::Primary`] button.
    ///
    /// `id` must be unique among the siblings of the button.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::default(),
            disabled: false,
            full_width: false,
            compact: false,
            tab_index: Some(0),
            focus_handle: None,
            truncate: false,
            on_click: None,
        }
    }

    /// Sets the visual weight of the button.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Grays the button out and stops it from reacting to clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Stretches the button across the width of its parent.
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// Shrinks the button to fit inside a dense toolbar or status bar.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Places the button at `index` in the window's tab order.
    ///
    /// A focused button draws an accent outline and is activated by `Enter` or
    /// `Space`, which gpui turns into an ordinary click. Disabled buttons are
    /// skipped, mirroring how the platform treats a disabled control.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Tracks the button's focus with `handle`, the owner's.
    ///
    /// What lets a dialog focus one particular button as it opens —
    /// [`crate::Modal::initial_focus`] with the same handle — and lets the
    /// owner ask whether it has the focus. The button stays in the tab order at
    /// its [`Button::tab_index`]; a disabled one is still skipped.
    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    /// Lets the button shrink below its label, cutting the label off with an
    /// ellipsis and showing the whole of it as a tooltip while it is cut.
    ///
    /// Off by default, when the button is as wide as its label and never
    /// narrower. For a button in a column of fixed width whose label is data —
    /// a profile name and its host — rather than a command: the alternative is
    /// a label that runs out of the button. The tooltip only appears while the
    /// label is actually cut, so a label that fits says nothing twice.
    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// Sets the click callback. Ignored while the button is disabled.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

/// Resolves the palette for `variant` against the active theme.
fn colors_for(variant: ButtonVariant, cx: &App) -> ButtonColors {
    let theme = theme(cx);
    match variant {
        ButtonVariant::Primary => ButtonColors {
            background: theme.accent,
            hover: shift_lightness(theme.accent, 0.06),
            active: shift_lightness(theme.accent, -0.06),
            label: theme.background,
            border: None,
        },
        ButtonVariant::Secondary => ButtonColors {
            background: theme.surface_hover,
            hover: theme.surface_active,
            active: shift_lightness(theme.surface_active, -0.04),
            label: theme.text,
            border: Some(theme.border),
        },
        ButtonVariant::Ghost => ButtonColors {
            background: gpui::transparent_black(),
            hover: theme.surface_hover,
            active: theme.surface_active,
            label: theme.text,
            border: None,
        },
        ButtonVariant::Danger => ButtonColors {
            background: theme.danger,
            hover: shift_lightness(theme.danger, 0.06),
            active: shift_lightness(theme.danger, -0.06),
            label: theme.background,
            border: None,
        },
    }
}

/// Whether a [`Button::truncate`] button's label was cut off when it was last
/// laid out. Element state, measured at prepaint and read by the next render,
/// which is the first that can decide whether there is a tooltip.
#[derive(Default)]
struct Truncation(Rc<Cell<bool>>);

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let accent = theme(cx).accent;
        let colors = colors_for(self.variant, cx);
        let disabled = self.disabled;
        let label_color = if disabled {
            colors.label.opacity(0.5)
        } else {
            colors.label
        };
        // The outline is always present, merely transparent, so that gaining
        // focus recolors it instead of resizing the button.
        let border = match colors.border {
            Some(border) if disabled => border.opacity(0.5),
            Some(border) => border,
            None => gpui::transparent_black(),
        };

        let (height, pad_x, text) = if self.compact {
            (20., 8., TextSize::Xs)
        } else {
            (30., 12., TextSize::Body)
        };
        let text = text.size(cx);

        // Only a truncating button keeps anything between frames; every other
        // button stays the stateless element it always was.
        let truncation = self.truncate.then(|| {
            window
                .use_keyed_state((self.id.clone(), "truncation"), cx, |_, _| {
                    Truncation::default()
                })
                .read(cx)
                .0
                .clone()
        });
        let truncated = truncation.as_ref().is_some_and(|cut| cut.get());
        let label = self.label.clone();

        div()
            .id(self.id)
            .flex()
            .when(!self.truncate, |this| this.flex_none())
            .when(self.truncate, |this| {
                this.flex_shrink(1.).min_w_0().overflow_hidden()
            })
            .items_center()
            .justify_center()
            .h(scaled(height, cx))
            .px(px(pad_x))
            .rounded_md()
            .text_size(text)
            .whitespace_nowrap()
            .bg(if disabled {
                colors.background.opacity(0.5)
            } else {
                colors.background
            })
            .text_color(label_color)
            .border_1()
            .border_color(border)
            .when_some(self.focus_handle.filter(|_| !disabled), |this, handle| {
                this.track_focus(&handle)
            })
            .when_some(self.tab_index.filter(|_| !disabled), |this, index| {
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
            })
            .when(self.full_width, |this| this.w_full())
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(colors.hover))
                    .active(|style| style.bg(colors.active))
            })
            .when_some(self.on_click.filter(|_| !disabled), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when(truncated, |this| this.tooltip(tooltip_label(label.clone())))
            .map(|this| match truncation {
                None => this.child(self.label),
                Some(cut) => this.child(
                    div()
                        .flex()
                        .min_w_0()
                        .overflow_hidden()
                        .child(div().min_w_0().text_ellipsis().child(self.label))
                        // Measured where the inherited font is known: the
                        // label's own width against the box it was given.
                        .on_children_prepainted(move |bounds, window, _cx| {
                            let Some(given) = bounds.first() else {
                                return;
                            };
                            let font = window.text_style().font();
                            let run = TextRun {
                                len: label.len(),
                                font,
                                color: Hsla::default(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let wanted = window
                                .text_system()
                                .shape_line(label.clone(), text, &[run], None)
                                .width;
                            let now = wanted > given.size.width + px(0.5);
                            if now != cut.get() {
                                cut.set(now);
                                window.refresh();
                            }
                        }),
                ),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{Context, Render, ScrollHandle, TestAppContext, VisualTestContext, div, px};

    use super::*;

    const LONG: &str = "production-reporting · reporting@db-replica-3.internal:5432";

    /// Two buttons with the same long label in a column too narrow for it.
    struct Column {
        cut: ScrollHandle,
        whole: ScrollHandle,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for Column {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            div()
                .flex()
                .flex_col()
                .w(px(120.))
                .child(
                    div().id("cut").track_scroll(&self.cut).flex().child(
                        Button::new("truncating", LONG)
                            .truncate(true)
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                    ),
                )
                .child(
                    div()
                        .id("whole")
                        .track_scroll(&self.whole)
                        .flex()
                        .child(Button::new("plain", LONG)),
                )
        }
    }

    /// A truncating button stays inside its column and still clicks; a plain
    /// one keeps the width of its label, as it always did.
    #[gpui::test]
    fn a_truncating_button_fits_its_column(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let clicks = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let clicks = clicks.clone();
            move |_, _| Column {
                cut: ScrollHandle::new(),
                whole: ScrollHandle::new(),
                clicks,
            }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let (cut, whole) = window
            .update(&mut cx, |column, _, _| {
                (column.cut.bounds(), column.whole.bounds())
            })
            .unwrap();
        assert!(cut.size.width <= px(120.), "{:?}", cut.size.width);
        let inside = cut.center();
        cx.simulate_click(inside, gpui::Modifiers::none());
        assert_eq!(clicks.get(), 1);
        // The plain button's row is the column's width, but the button in it
        // overflows to the width of its label; the tooltip-less layout is the
        // old one.
        assert_eq!(whole.size.width, px(120.));
    }

    struct Sized {
        frame: ScrollHandle,
    }

    impl Render for Sized {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().items_start().child(
                div()
                    .id("frame")
                    .track_scroll(&self.frame)
                    .child(Button::new("b", "OK")),
            )
        }
    }

    /// The button's height follows the UI scale, and is the old 30 px without
    /// one.
    #[gpui::test]
    fn the_height_follows_the_ui_scale(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| Sized {
            frame: ScrollHandle::new(),
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        for (scale, height) in [(1.0, 30.), (1.5, 45.)] {
            cx.update(|_, cx| crate::set_ui_scale(scale, cx));
            cx.run_until_parked();
            let frame = window
                .update(&mut cx, |sized, _, _| sized.frame.bounds())
                .unwrap();
            assert_eq!(frame.size.height, px(height), "at {scale}");
        }
    }
}
