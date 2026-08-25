//! The "About …" dialog.
//!
//! A read-only card: the wordmark, the version baked in at compile time, one
//! line of description, a link to the repository and the licence and credits.
//! It owns no form state, so unlike the other dialogs it has nothing to collect
//! or persist — it only reports that it was dismissed.
//!
//! Everything about it that differs per application is in [`AppIdentity`]: the
//! wordmark is [`AppIdentity::name`], the version [`AppIdentity::version`], and
//! the button [`AppIdentity::repository_label`] pointing at
//! [`AppIdentity::repository_url`]. The four lines of prose are the host's own
//! `about.*` keys.
//!
//! [`AppIdentity`]: crate::AppIdentity

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, Window,
    div, prelude::*, px,
};
use ruui::{Button, ButtonVariant, modal, theme};

use crate::inject::{identity, label, text};

/// Width of the dialog panel.
const DIALOG_WIDTH: f32 = 420.;

/// Licence the applications sharing this shell are distributed under.
///
/// A licence identifier, so it reads the same in every language, and a constant
/// rather than an identity field because all three of them are MIT and a
/// setting nobody varies is a setting nobody keeps correct.
const LICENSE: &str = "MIT";

/// Emitted by [`AboutDialog`] when the user closes it.
pub enum AboutDialogEvent {
    /// The dialog was dismissed; the host should restore focus.
    Dismissed,
}

/// Modal dialog describing the application.
///
/// Create it once with [`AboutDialog::new`], keep the handle, subscribe to
/// [`AboutDialogEvent`], and render it as the last child of a `relative()`
/// root. It renders nothing while [`AboutDialog::is_open`] is `false`, so it is
/// safe to render unconditionally.
pub struct AboutDialog {
    /// Whether the dialog is currently visible.
    open: bool,
    /// Focus of the dialog root; also the anchor for the `Escape` handler.
    focus_handle: FocusHandle,
    /// Whether focus should move into the dialog on the next render.
    pending_focus: bool,
}

impl AboutDialog {
    /// Builds the dialog, closed.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            focus_handle: cx.focus_handle(),
            pending_focus: false,
        }
    }

    /// Shows the dialog.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.pending_focus = true;
        cx.notify();
    }

    /// Whether the dialog is visible.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Hides the dialog without emitting an event.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.pending_focus = false;
        cx.notify();
    }

    /// Closes the dialog and reports it, so the host can restore focus.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(AboutDialogEvent::Dismissed);
        self.close(cx);
    }

    /// `Escape` dismisses the dialog from anywhere inside it.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.open && event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.dismiss(cx);
        }
    }

    /// Moves focus into the dialog when it opens, so `Escape` reaches it.
    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_focus {
            return;
        }
        self.pending_focus = false;
        let handle = self.focus_handle.clone();
        window.focus(&handle, cx);
        cx.notify();
    }
}

impl EventEmitter<AboutDialogEvent> for AboutDialog {}

impl Focusable for AboutDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AboutDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().id("about-dialog");
        }

        self.apply_pending_focus(window, cx);

        let theme = theme(cx);
        let app = identity(cx);
        let this = cx.entity();

        let heading = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(26.))
                    .text_color(theme.text)
                    .child(app.name),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(text(cx, "about.version", &[("version", app.version)])),
            );

        let repository = Button::new("about-repository", app.repository_label)
            .variant(ButtonVariant::Secondary)
            .full_width(true)
            .on_click(move |_, _window, cx| cx.open_url(app.repository_url));

        let footnotes = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .text_size(px(11.))
            .text_color(theme.text_muted)
            .child(div().child(text(cx, "about.license", &[("license", LICENSE)])))
            .child(div().child(label(cx, "about.credits")));

        let close = div().flex().flex_row().justify_end().child(
            Button::new("about-close", label(cx, "common.close"))
                .variant(ButtonVariant::Primary)
                .on_click({
                    let this = this.clone();
                    move |_, _window, cx| {
                        this.update(cx, |dialog, cx| dialog.dismiss(cx));
                    }
                }),
        );

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(heading)
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.text)
                    .child(label(cx, "about.tagline")),
            )
            .child(repository)
            .child(footnotes)
            .child(close);

        let title = label(cx, "about.title");
        let on_dismiss = {
            let this = cx.entity();
            move |_window: &mut Window, cx: &mut App| {
                this.update(cx, |dialog, cx| dialog.dismiss(cx));
            }
        };

        // Absolute and full-size for the same reason as the other dialogs: an
        // absolutely positioned child is laid out against its direct parent.
        div()
            .id("about-dialog")
            .absolute()
            .inset_0()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(modal(
                "about-modal",
                title,
                px(DIALOG_WIDTH),
                body,
                on_dismiss,
            ))
    }
}
