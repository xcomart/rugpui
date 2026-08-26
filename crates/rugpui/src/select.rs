//! A dropdown that picks one string out of a list.
//!
//! Like every other widget here the control is stateless: the parent view owns
//! the selected value, the open flag and the list's [`ScrollHandle`], passes
//! them in on every render, and reacts to [`Select::on_select`] and
//! [`Select::on_open_change`].
//!
//! The list is drawn with [`deferred`] rather than inline, for the same reason
//! [`MenuButton`](crate::MenuButton) does it: a trigger that sits inside a
//! scrolling form would otherwise have its list clipped by that form.

use std::rc::Rc;

use gpui::{
    Anchor, AnchoredPositionMode, App, ElementId, MouseButton, Pixels, ScrollHandle, SharedString,
    Window, anchored, deferred, div, point, prelude::*, px, svg, transparent_black,
};

use crate::scrollbar::Scrollbar;
use crate::theme::theme;

/// Height of the trigger, matching [`TextInput`](crate::TextInput) so the two
/// line up when a form mixes them.
const TRIGGER_HEIGHT: f32 = 32.;

/// Vertical distance from the top of the trigger to the top of the list, so the
/// list clears the button it hangs from.
const DROP_OFFSET: f32 = TRIGGER_HEIGHT + 4.;

/// Width of the list when the caller sets no width of its own.
///
/// An `anchored` element is positioned absolutely and therefore cannot inherit
/// the trigger's width, so the list always needs a width in pixels.
const DEFAULT_WIDTH: f32 = 320.;

/// Height at which the list starts scrolling.
const LIST_MAX_HEIGHT: f32 = 260.;

/// Height of one option row.
const ROW_HEIGHT: f32 = 26.;

/// Distance the list keeps from the window edges when it would overflow.
const WINDOW_MARGIN: f32 = 6.;

/// Draw order of the click-catching backdrop, relative to other deferred draws.
const BACKDROP_PRIORITY: usize = 1;

/// Draw order of the list; above [`BACKDROP_PRIORITY`] so that the backdrop
/// never eats clicks meant for an option row.
const LIST_PRIORITY: usize = 2;

/// Glyph drawn at the right edge of the trigger.
const CHEVRON: &str = "\u{25be}";

/// Edge length of an option's leading and trailing icon.
///
/// Smaller than the 13 px row text is tall, so a mark reads as an ornament
/// beside the label rather than as a second thing competing with it.
const OPTION_ICON_SIZE: f32 = 14.;

/// Gap between an option's icons and its label, on the rows and on the trigger.
const OPTION_ICON_GAP: f32 = 6.;

/// Callback fired with the index and the text of the option the user picked.
type SelectHandler = Rc<dyn Fn(usize, &str, &mut Window, &mut App)>;

/// Callback fired when the list wants to open or close itself.
type OpenChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// One row of a [`Select`]: a label, and up to two icons to flank it with.
///
/// The label is still the option's identity — [`Select::selected`] matches on
/// it and [`Select::on_select`] hands it back — so the icons are decoration
/// only. Both are asset paths resolved through the application's
/// [`AssetSource`](gpui::AssetSource), like every other icon in this crate.
///
/// Nothing is reserved for an absent icon: a row without a leading mark starts
/// at the same x as the label of a row that has none, not indented to line up
/// under one that does. A list where only some rows are marked will therefore
/// look ragged, and whether that reads as sloppy or as meaningful — an error
/// badge on the two bad entries, say — is the caller's call to make, not this
/// widget's.
///
/// ```ignore
/// SelectOption::new("PostgreSQL")
///     .leading("icons/database.svg")
///     .trailing("icons/warning.svg")
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    /// The text of the option, which is also its identity.
    pub label: SharedString,
    /// Asset path of the icon drawn before the label.
    pub leading: Option<SharedString>,
    /// Asset path of the icon drawn after the label, at the row's right edge.
    pub trailing: Option<SharedString>,
}

impl SelectOption {
    /// Creates a bare option: a label, no icons.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            leading: None,
            trailing: None,
        }
    }

    /// Draws the asset at `path` before the label.
    pub fn leading(mut self, path: impl Into<SharedString>) -> Self {
        self.leading = Some(path.into());
        self
    }

    /// Draws the asset at `path` after the label, pushed to the right edge of
    /// the row.
    pub fn trailing(mut self, path: impl Into<SharedString>) -> Self {
        self.trailing = Some(path.into());
        self
    }
}

// The three `From` impls are what let `.options(…)` keep taking the plain
// string lists it always took: a caller that wants no icons should not have to
// learn this type exists.

impl From<SharedString> for SelectOption {
    fn from(label: SharedString) -> Self {
        Self::new(label)
    }
}

impl From<&'static str> for SelectOption {
    fn from(label: &'static str) -> Self {
        Self::new(label)
    }
}

impl From<String> for SelectOption {
    fn from(label: String) -> Self {
        Self::new(label)
    }
}

/// A stateless one-of-many dropdown.
///
/// The text of an option is also its identity, which keeps the widget usable
/// for lists the caller discovers at runtime — font families, for one —
/// without inventing ids for them. Options are therefore given as plain
/// strings; hand [`SelectOption`]s instead to flank a label with icons.
///
/// The control takes a single tab stop. `Enter` and `Space` toggle the list, as
/// they do for any focusable element in gpui, and while the list is open the
/// arrow keys move the selection without wrapping. Closing on `Escape` is left
/// to the parent, so that a dialog can decide whether the key belongs to the
/// dropdown or to itself.
///
/// ```ignore
/// Select::new("font")
///     .options(font_names)
///     .selected(self.font.clone())
///     .placeholder("System default")
///     .open(self.font_open)
///     .scroll_handle(self.font_scroll.clone())
///     .on_select(..)
///     .on_open_change(..)
/// ```
#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    options: Vec<SelectOption>,
    selected: Option<SharedString>,
    placeholder: SharedString,
    open: bool,
    width: Option<Pixels>,
    tab_index: Option<isize>,
    scroll_handle: Option<ScrollHandle>,
    scrollbar: Option<Scrollbar>,
    on_select: Option<SelectHandler>,
    on_open_change: Option<OpenChangeHandler>,
    chevron_icon: Option<SharedString>,
}

impl Select {
    /// Creates an empty, closed dropdown with nothing selected.
    ///
    /// `id` must be unique among the siblings of the control.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
            selected: None,
            placeholder: SharedString::default(),
            open: false,
            width: None,
            tab_index: None,
            scroll_handle: None,
            scrollbar: None,
            on_select: None,
            on_open_change: None,
            chevron_icon: None,
        }
    }

    /// Sets the options, in display order.
    ///
    /// Takes anything that converts into a [`SelectOption`], so a list of
    /// strings — the common case, and every case there was before icons
    /// existed — passes straight through.
    pub fn options(mut self, options: impl IntoIterator<Item = impl Into<SelectOption>>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the picked option. An option the list does not contain still shows
    /// on the trigger, it just highlights no row.
    pub fn selected(mut self, selected: Option<impl Into<SharedString>>) -> Self {
        self.selected = selected.map(Into::into);
        self
    }

    /// Sets the text shown muted on the trigger while nothing is selected.
    ///
    /// A list that offers an explicit "no choice" row should spell it the same
    /// way as the placeholder: that row is then highlighted while the selection
    /// is empty, so the open list always shows where the user stands.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets whether the list is currently shown.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the width of the trigger and the list.
    ///
    /// Without it the trigger fills its parent and the list falls back to a
    /// fixed width, because an absolutely positioned list cannot measure the
    /// trigger it hangs from.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Places the control at `index` in the window's tab order.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Attaches the scroll handle of the list.
    ///
    /// The handle belongs to the parent so that it can reveal the current
    /// option — with [`ScrollHandle::scroll_to_item`] — when it opens the list.
    /// Keyboard navigation scrolls through the same handle.
    pub fn scroll_handle(mut self, handle: ScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
        self
    }

    /// Draws `bar` down the open list as its overlay scroll indicator.
    ///
    /// Passed in rather than built here, and only while it should be on screen,
    /// for the same reason the handle above is: a bar comes and goes with the
    /// scrolling, and this control keeps no state between renders. The owner
    /// answers drags of it too, since the id it built the bar with is what tells
    /// that drag from any other.
    pub fn scrollbar(mut self, bar: Scrollbar) -> Self {
        self.scrollbar = Some(bar);
        self
    }

    /// Sets the callback invoked with the option the user picked.
    ///
    /// Receives both the zero-based index of the option and its text. The index
    /// is what a caller should key off when the list has a fixed shape — a
    /// leading "no choice" row, say — because the text is translated and
    /// comparing against it would break in every language but one.
    ///
    /// Fired by a click on a row and by the arrow keys; the list closes itself
    /// after a click, so the callback does not have to.
    pub fn on_select(
        mut self,
        handler: impl Fn(usize, &str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Called with the open state the control would like to be in.
    ///
    /// Fires with `true` when the trigger is activated while closed, and with
    /// `false` when it is activated again, when a row is clicked, or when the
    /// pointer goes down anywhere outside the list.
    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }

    /// Draws the asset at `path` in place of the `▾` glyph.
    ///
    /// Painted in `theme.text_muted` — the glyph's own tint — whether the list
    /// is open or closed: a select's chevron always points down, unlike a
    /// tree's arrow, so there is only ever the one path to hand over, not the
    /// open/closed pair [`TreeView::with_arrow_icons`](crate::TreeView::with_arrow_icons)
    /// and [`Collapsible::arrow_icons`](crate::Collapsible::arrow_icons) take.
    /// Hand this the same asset given to those two so a tree, a collapsible
    /// section and a dropdown all disclose with the one mark.
    pub fn chevron_icon(mut self, path: impl Into<SharedString>) -> Self {
        self.chevron_icon = Some(path.into());
        self
    }
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let viewport = window.viewport_size();
        let open = self.open;
        let id = self.id;
        let options = self.options;
        let placeholder = self.placeholder;
        let selected = self.selected;
        let on_select = self.on_select;
        let on_open_change = self.on_open_change;
        let scroll_handle = self.scroll_handle;
        let list_width = self.width.unwrap_or(px(DEFAULT_WIDTH));
        let chevron_icon = self.chevron_icon;

        // With nothing selected the row that repeats the placeholder counts as
        // the current one, so a list whose first entry means "no choice" still
        // marks itself while the selection is empty.
        let current = options.iter().position(|option| match &selected {
            Some(selected) => option.label == *selected,
            None => option.label == placeholder,
        });
        let label = selected.clone().unwrap_or_else(|| placeholder.clone());

        // The trigger wears the icons of the option it names, but only when
        // something really is selected: the placeholder row may well be one of
        // the options, and repeating its mark would make an empty dropdown look
        // like a made choice.
        let (trigger_leading, trigger_trailing) = match current.filter(|_| selected.is_some()) {
            Some(index) => {
                let option = &options[index];
                (option.leading.clone(), option.trailing.clone())
            }
            None => (None, None),
        };

        let trigger = div()
            .id(ElementId::from((id.clone(), "trigger")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .w_full()
            .h(px(TRIGGER_HEIGHT))
            .px(px(8.))
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_size(px(14.))
            .line_height(px(20.))
            .cursor_pointer()
            .hover(|style| style.bg(theme.surface_hover))
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                this.tab_index(index)
                    .focus(move |style| style.border_color(accent))
            })
            .when_some(on_open_change.clone(), |this, handler| {
                this.on_click(move |_, window, cx| handler(!open, window, cx))
            })
            .on_key_down({
                let options = options.clone();
                let on_select = on_select.clone();
                let scroll_handle = scroll_handle.clone();
                move |event, window, cx| {
                    if !open || event.keystroke.modifiers.modified() || options.is_empty() {
                        return;
                    }
                    let delta: isize = match event.keystroke.key.as_str() {
                        "up" => -1,
                        "down" => 1,
                        _ => return,
                    };
                    let last = options.len() - 1;
                    let next = match current {
                        Some(current) => {
                            (current as isize + delta).clamp(0, last as isize) as usize
                        }
                        None if delta > 0 => 0,
                        None => last,
                    };
                    cx.stop_propagation();
                    if let Some(handle) = scroll_handle.as_ref() {
                        handle.scroll_to_item(next);
                    }
                    if Some(next) != current
                        && let Some(handler) = on_select.as_ref()
                    {
                        handler(next, &options[next].label, window, cx);
                    }
                }
            })
            .children(trigger_leading.map(|path| {
                svg()
                    .size(px(OPTION_ICON_SIZE))
                    .flex_none()
                    .path(path)
                    .text_color(theme.icon)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if selected.is_some() {
                        theme.text
                    } else {
                        theme.text_muted
                    })
                    .child(label),
            )
            .children(trigger_trailing.map(|path| {
                svg()
                    .size(px(OPTION_ICON_SIZE))
                    .flex_none()
                    .path(path)
                    .text_color(theme.icon)
            }))
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme.text_muted)
                    .child(match chevron_icon.clone() {
                        // An SVG takes its colour from its own `text_color`,
                        // which — unlike a glyph's — does not inherit from the
                        // box around it, so the muted tint has to be handed to
                        // it directly.
                        Some(path) => svg()
                            .size(px(12.))
                            .flex_none()
                            .path(path)
                            .text_color(theme.text_muted)
                            .into_any_element(),
                        None => CHEVRON.into_any_element(),
                    }),
            );

        // A full-window sheet under the list: a pointer press anywhere it can
        // see closes the dropdown. It is deferred so that it covers the whole
        // window rather than just the row the trigger sits in.
        let backdrop = div()
            .id(ElementId::from((id.clone(), "backdrop")))
            .w(viewport.width)
            .h(viewport.height)
            .occlude()
            .when_some(on_open_change.clone(), |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    handler(false, window, cx)
                })
            });

        let row_theme = theme.clone();
        let rows = options.into_iter().enumerate().map(move |(index, option)| {
            let theme = &row_theme;
            let is_current = Some(index) == current;
            let on_select = on_select.clone();
            let on_open_change = on_open_change.clone();
            let SelectOption {
                label,
                leading,
                trailing,
            } = option;
            let value = label.clone();

            // An svg takes no colour from the box around it, so the row's own
            // tint has to be handed to each icon: the marks then go accent with
            // the label when this is the current row, instead of staying grey
            // beside highlighted text.
            let icon_color = if is_current { theme.accent } else { theme.icon };

            div()
                .id(ElementId::from(("select-option", index)))
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(OPTION_ICON_GAP))
                .h(px(ROW_HEIGHT))
                .px(px(10.))
                .mx(px(4.))
                .rounded_sm()
                .text_size(px(13.))
                .text_color(if is_current { theme.accent } else { theme.text })
                .bg(if is_current {
                    theme.surface_active
                } else {
                    transparent_black()
                })
                .cursor_pointer()
                .hover(|style| style.bg(theme.surface_hover))
                .on_click(move |_, window, cx| {
                    if let Some(handler) = on_select.as_ref() {
                        handler(index, &value, window, cx);
                    }
                    if let Some(handler) = on_open_change.as_ref() {
                        handler(false, window, cx);
                    }
                })
                .children(leading.map(|path| {
                    svg()
                        .size(px(OPTION_ICON_SIZE))
                        .flex_none()
                        .path(path)
                        .text_color(icon_color)
                }))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(trailing.map(|path| {
                    svg()
                        .size(px(OPTION_ICON_SIZE))
                        .flex_none()
                        .path(path)
                        .text_color(icon_color)
                }))
        });

        let list = div()
            .id(ElementId::from((id.clone(), "list")))
            .occlude()
            .flex()
            .flex_col()
            .flex_none()
            .w(list_width)
            .max_h(px(LIST_MAX_HEIGHT))
            .py(px(4.))
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .shadow_lg()
            .text_color(theme.text)
            .when_some(scroll_handle.as_ref(), |this, handle| {
                this.track_scroll(handle)
            })
            .children(rows);

        // The bar cannot go inside the list, whose children are what scroll
        // away underneath it; this box is the list's own size, so it is what the
        // thumb is placed against.
        let list = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .child(list)
            .children(self.scrollbar.and_then(|bar| bar.render(&theme)));

        // The list hangs off a zero-sized box laid out *before* the trigger,
        // not off the trigger itself: an `anchored` element is positioned
        // absolutely, and an absolutely positioned box is placed by its
        // parent's alignment, so giving it a box of its own is what pins it to
        // the trigger's top-left corner.
        let overlays = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(0.))
            .h(px(0.))
            .child(
                deferred(
                    anchored()
                        .position(point(px(0.), px(0.)))
                        .position_mode(AnchoredPositionMode::Window)
                        .child(backdrop),
                )
                .with_priority(BACKDROP_PRIORITY),
            )
            .child(
                deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .offset(point(px(0.), px(DROP_OFFSET)))
                        .snap_to_window_with_margin(px(WINDOW_MARGIN))
                        .child(list),
                )
                .with_priority(LIST_PRIORITY),
            );

        div()
            .id(id)
            .flex()
            .flex_col()
            .w_full()
            .when_some(self.width, |this, width| this.w(width))
            .children(open.then_some(overlays))
            .child(trigger)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use gpui::{Context, Render, TestAppContext, VisualTestContext, size};

    use super::*;

    /// Size of the window the render test opens.
    const HARNESS_WIDTH: f32 = 320.;
    const HARNESS_HEIGHT: f32 = 200.;

    /// A view holding one open dropdown wearing a chevron icon, as a form
    /// would.
    struct Harness;

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                Select::new("select")
                    .options([
                        SharedString::new_static("One"),
                        SharedString::new_static("Two"),
                    ])
                    .selected(Some("One"))
                    .open(true)
                    .chevron_icon("icons/chevron.svg"),
            )
        }
    }

    /// The same list, but with the icon slots in play: the selected row wears
    /// both, the second only a leading mark, the third none at all — the ragged
    /// case [`SelectOption`] warns about, drawn on purpose so the row layout is
    /// exercised with and without each slot.
    struct IconHarness;

    impl Render for IconHarness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                Select::new("select")
                    .options([
                        SelectOption::new("One")
                            .leading("icons/database.svg")
                            .trailing("icons/warning.svg"),
                        SelectOption::new("Two").leading("icons/database.svg"),
                        SelectOption::new("Three"),
                    ])
                    .selected(Some("One"))
                    .open(true),
            )
        }
    }

    /// [`Select::chevron_icon`] swaps the glyph for an `svg` at the given
    /// path. The test `AssetSource` answers every path with `None`, which
    /// gpui draws as nothing, so this only proves the swap does not panic
    /// layout or paint.
    #[gpui::test]
    fn a_chevron_icon_renders_without_panicking(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), |_, _| Harness);
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
    }

    /// Options carrying a leading icon, a trailing icon, both, or neither lay
    /// out and paint on the trigger and down the open list. As above, the test
    /// `AssetSource` draws every path as nothing, so what this proves is that
    /// the extra children upset neither layout nor paint.
    #[gpui::test]
    fn iconed_options_render_without_panicking(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let window = cx.open_window(size(px(HARNESS_WIDTH), px(HARNESS_HEIGHT)), |_, _| {
            IconHarness
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
    }

    /// The `From` impls are what keep every pre-icon caller compiling, so they
    /// have to yield the same bare option [`SelectOption::new`] does — a label
    /// and two empty slots.
    #[test]
    fn strings_convert_into_bare_options() {
        let expected = SelectOption::new("One");

        assert_eq!(SelectOption::from("One"), expected);
        assert_eq!(
            SelectOption::from(SharedString::new_static("One")),
            expected
        );
        assert_eq!(SelectOption::from("One".to_owned()), expected);
        assert!(expected.leading.is_none());
        assert!(expected.trailing.is_none());
    }
}
