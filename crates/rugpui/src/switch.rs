//! A labelled on/off switch.

use gpui::{App, ClickEvent, ElementId, SharedString, Window, div, prelude::*, px};

use crate::theme::theme;

/// Callback fired with the value the switch is about to take.
type ToggleHandler = Box<dyn Fn(bool, &mut Window, &mut App)>;

/// A stateless on/off switch with a clickable label.
///
/// Like [`Checkbox`](crate::Checkbox) — which it mirrors down to the shape of
/// its API — it owns no state of its own: the parent view passes the current
/// value on every render and updates its own state from [`Switch::on_toggle`],
/// which receives the *new* value. Clicking anywhere on the row — track or
/// label — flips it.
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let checked = self.checked;
        let next = !checked;

        let (track_bg, track_border, knob_color) = if checked {
            (theme.accent, theme.accent, theme.background)
        } else {
            (theme.surface, theme.border, theme.text_muted)
        };

        let track = div()
            .flex()
            .flex_none()
            .flex_row()
            .items_center()
            .when(checked, |this| this.justify_end())
            .when(!checked, |this| this.justify_start())
            .w(px(30.))
            .h(px(16.))
            .p(px(2.))
            .rounded_full()
            .border_1()
            .border_color(track_border)
            .bg(track_bg)
            .child(div().size(px(12.)).rounded_full().bg(knob_color));

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
