//! Short notices that come and go in a corner of the window.
//!
//! An application built on this crate had two places to tell the user how
//! something went: a dialog, which stops everything for an answer nobody asked
//! for, and the log, which nobody reads. A saved layout, a refused command, a
//! file that could not be read all ended up in the second. A toast is the
//! third place: a line in the corner, in the theme's colors for how serious it
//! is, that leaves by itself when it was only news and stays until it is
//! closed when it was a failure.
//!
//! The queue is a [`Global`], so anything holding an [`App`] can raise one —
//! a background task's completion, an action handler with no view of its own:
//!
//! ```ignore
//! rugpui_shell::show_toast(Toast::info(ts!("layout.saved")), cx);
//! rugpui_shell::show_toast(Toast::error(ts!("file.unreadable", path = path)), cx);
//! ```
//!
//! and one element, [`toast_stack`], draws it. The application puts it at the
//! end of its window's root, after every dialog, so that a notice raised while
//! a dialog is open is still seen.

use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, ElementId, FontWeight, Global, Hsla, Pixels, SharedString, Window, div,
    prelude::*, px,
};
use rugpui::{TextSize, editor_theme, scaled, theme, tooltip_label};

use crate::inject::label;

/// How serious a [`Toast`] is, which decides its color and how long it stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastLevel {
    /// Something worked. Leaves after [`INFO_DURATION`].
    Info,
    /// Something worked only in part, or was refused for a reason the user
    /// can fix. Leaves after [`WARNING_DURATION`].
    Warning,
    /// Something failed. Stays until it is closed.
    Error,
}

/// How long an [`ToastLevel::Info`] notice stays.
pub const INFO_DURATION: Duration = Duration::from_secs(4);

/// How long a [`ToastLevel::Warning`] notice stays: twice an info's, since it
/// usually says what to do next.
pub const WARNING_DURATION: Duration = Duration::from_secs(8);

/// The most notices on screen at once. Past it the oldest that would leave by
/// itself goes first, so that a burst of news cannot push a failure out.
pub const MAX_TOASTS: usize = 5;

/// Width of one notice, at the default UI scale.
const TOAST_WIDTH: f32 = 360.;

/// Distance from the window's right edge, and the default from its bottom.
const TOAST_MARGIN: f32 = 16.;

/// Width of a toast's close button, at the default UI scale.
const CLOSE_WIDTH: f32 = 28.;

/// One notice.
#[derive(Debug, Clone)]
pub struct Toast {
    level: ToastLevel,
    title: Option<SharedString>,
    message: SharedString,
    duration: Option<Duration>,
    window: Option<AnyWindowHandle>,
}

impl Toast {
    /// A notice of `level` saying `message`, in the application's words, that
    /// stays as long as `level` says.
    pub fn new(level: ToastLevel, message: impl Into<SharedString>) -> Self {
        Self {
            level,
            title: None,
            message: message.into(),
            duration: match level {
                ToastLevel::Info => Some(INFO_DURATION),
                ToastLevel::Warning => Some(WARNING_DURATION),
                ToastLevel::Error => None,
            },
            window: None,
        }
    }

    /// An [`ToastLevel::Info`] notice.
    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new(ToastLevel::Info, message)
    }

    /// A [`ToastLevel::Warning`] notice.
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(ToastLevel::Warning, message)
    }

    /// An [`ToastLevel::Error`] notice.
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(ToastLevel::Error, message)
    }

    /// A bold first line above the message.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How long the notice stays, or `None` for until it is closed — in place
    /// of what its level says.
    pub fn duration(mut self, duration: Option<Duration>) -> Self {
        self.duration = duration;
        self
    }

    /// Shows the notice in `window` alone. Without this every window's
    /// [`toast_stack`] shows it, which is right for an application with one.
    pub fn in_window(mut self, window: AnyWindowHandle) -> Self {
        self.window = Some(window);
        self
    }

    /// How serious the notice is.
    pub fn level(&self) -> ToastLevel {
        self.level
    }

    /// What the notice says.
    pub fn message(&self) -> &SharedString {
        &self.message
    }
}

/// Names one shown notice, for [`dismiss_toast`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

/// The notices on screen, oldest first.
#[derive(Default)]
struct Toasts {
    shown: Vec<(ToastId, Toast)>,
    next: u64,
}

impl Global for Toasts {}

/// Shows `toast`, and returns what names it.
///
/// A notice with a duration takes itself down when it is up; one without stays
/// until its close button is pressed or [`dismiss_toast`] is called.
pub fn show_toast(toast: Toast, cx: &mut App) -> ToastId {
    let duration = toast.duration;
    let queue = cx.default_global::<Toasts>();
    let id = ToastId(queue.next);
    queue.next += 1;
    queue.shown.push((id, toast));
    while queue.shown.len() > MAX_TOASTS {
        let oldest = queue
            .shown
            .iter()
            .position(|(_, toast)| toast.duration.is_some())
            .unwrap_or(0);
        queue.shown.remove(oldest);
    }
    if let Some(duration) = duration {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(duration).await;
            cx.update(|cx| dismiss_toast(id, cx));
        })
        .detach();
    }
    cx.refresh_windows();
    id
}

/// Takes notice `id` down. Nothing happens when it has already gone.
pub fn dismiss_toast(id: ToastId, cx: &mut App) {
    let Some(queue) = cx.try_global::<Toasts>() else {
        return;
    };
    if queue.shown.iter().any(|(shown, _)| *shown == id) {
        cx.global_mut::<Toasts>()
            .shown
            .retain(|(shown, _)| *shown != id);
        cx.refresh_windows();
    }
}

/// Takes every notice down.
pub fn clear_toasts(cx: &mut App) {
    if cx
        .try_global::<Toasts>()
        .is_some_and(|queue| !queue.shown.is_empty())
    {
        cx.global_mut::<Toasts>().shown.clear();
        cx.refresh_windows();
    }
}

/// The notices on screen, oldest first: what a test of the application asks
/// to see whether one was raised.
pub fn shown_toasts(cx: &App) -> Vec<(ToastId, Toast)> {
    cx.try_global::<Toasts>()
        .map(|queue| queue.shown.clone())
        .unwrap_or_default()
}

/// The element that draws the notices, stacked up from the bottom-right corner
/// of the window with the newest at the bottom.
///
/// Render it as the last child of the window's `relative()` root. Each notice
/// has a close button whose tooltip is the application's `common.close`.
pub fn toast_stack() -> ToastStack {
    ToastStack {
        bottom: px(TOAST_MARGIN),
    }
}

/// What [`toast_stack`] builds.
#[derive(IntoElement)]
pub struct ToastStack {
    bottom: Pixels,
}

impl ToastStack {
    /// Keeps the stack `inset` above the bottom of the window instead of the
    /// default 16 px — clear of a status bar, say.
    pub fn bottom_inset(mut self, inset: Pixels) -> Self {
        self.bottom = inset;
        self
    }
}

/// The stripe color of a notice of `level`.
fn level_color(level: ToastLevel, cx: &App) -> Hsla {
    match level {
        ToastLevel::Info => theme(cx).accent,
        // The chrome palette has no amber of its own; the editor's warning is
        // the one the application already marks a warning with.
        ToastLevel::Warning => editor_theme(cx).warning,
        ToastLevel::Error => theme(cx).danger,
    }
}

impl RenderOnce for ToastStack {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let here = window.window_handle();
        let shown: Vec<(ToastId, Toast)> = shown_toasts(cx)
            .into_iter()
            .filter(|(_, toast)| toast.window.is_none_or(|window| window == here))
            .collect();
        let palette = theme(cx);
        let close = label(cx, "common.close");
        let cards = shown.into_iter().map(|(id, toast)| {
            let stripe = level_color(toast.level, cx);
            div()
                .id(ElementId::from(("toast", id.0 as usize)))
                .occlude()
                .flex()
                .flex_row()
                .w(scaled(TOAST_WIDTH, cx))
                .max_w_full()
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(palette.border)
                .bg(palette.surface)
                .shadow_md()
                .child(div().flex_none().w(px(4.)).bg(stripe))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(TextSize::Body.size(cx))
                        .text_color(palette.text)
                        .when_some(toast.title.clone(), |this, title| {
                            this.child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        })
                        .child(div().child(toast.message.clone())),
                )
                .child(
                    div()
                        .id(ElementId::from(("toast-close", id.0 as usize)))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .w(scaled(CLOSE_WIDTH, cx))
                        .text_size(TextSize::Body.size(cx))
                        .text_color(palette.icon)
                        .cursor_pointer()
                        .hover(|style| style.bg(palette.surface_hover).text_color(palette.text))
                        .tooltip(tooltip_label(close.clone()))
                        .on_click(move |_, _, cx| dismiss_toast(id, cx))
                        .child("\u{2715}"),
                )
        });
        div()
            .absolute()
            .right(px(TOAST_MARGIN))
            .bottom(self.bottom)
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .children(cards)
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn an_info_leaves_by_itself_and_an_error_stays(cx: &mut TestAppContext) {
        cx.update(rugpui::init);
        let (info, error) = cx.update(|cx| {
            (
                show_toast(Toast::info("Saved"), cx),
                show_toast(Toast::error("Could not read the file"), cx),
            )
        });
        assert_eq!(cx.update(|cx| shown_toasts(cx).len()), 2);

        cx.executor()
            .advance_clock(INFO_DURATION + Duration::from_millis(10));
        cx.run_until_parked();
        let left: Vec<ToastId> =
            cx.update(|cx| shown_toasts(cx).into_iter().map(|(id, _)| id).collect());
        assert_eq!(left, [error], "the info went, the error stayed");

        cx.executor().advance_clock(Duration::from_secs(600));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| shown_toasts(cx).len()), 1);
        cx.update(|cx| {
            dismiss_toast(error, cx);
            dismiss_toast(info, cx);
        });
        assert!(cx.update(|cx| shown_toasts(cx).is_empty()));
    }

    /// Past the limit the oldest notice that would leave anyway goes first,
    /// so news cannot push a failure off the screen.
    #[gpui::test]
    fn a_burst_of_news_does_not_push_a_failure_out(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let failure = show_toast(Toast::error("Failed"), cx);
            for index in 0..MAX_TOASTS + 2 {
                show_toast(Toast::info(format!("News {index}")), cx);
            }
            let shown = shown_toasts(cx);
            assert_eq!(shown.len(), MAX_TOASTS);
            assert_eq!(shown[0].0, failure);
            assert_eq!(shown[MAX_TOASTS - 1].1.message().as_ref(), "News 6");
            clear_toasts(cx);
            assert!(shown_toasts(cx).is_empty());
        });
    }
}
