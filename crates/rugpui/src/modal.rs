//! Centered dialog rendered on top of a translucent backdrop, and the labeled
//! rows its body is usually built out of.
//!
//! [`modal`] takes the five things every dialog has and hands back a [`Modal`],
//! so that the one call every existing dialog makes still draws exactly what it
//! drew. What a dialog may want beyond that is opt-in, each through one builder
//! method:
//!
//! * **Where the focus goes.** [`Modal::initial_focus`] focuses a control of the
//!   dialog as it opens — the default button, or Cancel when the button that
//!   would be the default destroys something — and [`Modal::autofocus`]
//!   focuses the panel itself. Either way the focus that was there before is
//!   given back when the dialog goes, unless the host has put it somewhere else
//!   by then; [`FocusReturn`] is the same thing for an overlay that is not a
//!   modal.
//! * **What `Enter` and `Escape` do.** [`Modal::on_confirm`] answers `Enter`
//!   pressed on the panel or in a one-line field that has no submit of its own,
//!   and [`Modal::escape_dismisses`] sends `Escape` to the dismiss handler. A
//!   focused button, checkbox or open dropdown keeps the key: `Enter` on Cancel
//!   is Cancel.
//! * **When a dismissal is allowed.** [`Modal::backdrop_dismisses`] turns the
//!   backdrop click off, and [`Modal::dismiss_guard`] is asked before either
//!   gesture dismisses — which is how a dialog holding unsaved input ignores
//!   a stray click and asks before throwing the input away on `Escape`.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, ElementId, FocusHandle, Font, Hsla, KeyBinding,
    Pixels, SharedString, TextRun, Window, actions, div, prelude::*, px,
};

use crate::overlay;
use crate::scale::{TextSize, scaled};
use crate::theme::theme;

actions!(
    rugpui_modal,
    [
        /// `Enter` inside a dialog: its [`Modal::on_confirm`].
        Confirm,
        /// `Escape` inside a dialog: its dismiss handler, when
        /// [`Modal::escape_dismisses`] is set.
        Cancel,
    ]
);

/// Key context of a dialog's panel, which the two keys above are bound in.
///
/// Bound on the panel rather than on the host's root so that a control inside
/// with a binding of its own — a dropdown's `Escape`, a multi-line field's
/// `Enter` — sits deeper and keeps it.
const KEY_CONTEXT: &str = "RugpuiModal";

/// The key context a [`crate::TextInput`] carries, which is the one control
/// whose `Enter` the dialog answers: a one-line field with no submit of its
/// own passes the key on.
const FIELD_CONTEXT: &str = "TextInput";

/// Registers the two keys a dialog answers.
///
/// Both handlers pass the key on when the dialog has not asked for it, so a
/// dialog that sets neither option behaves exactly as one did before the
/// bindings existed: the host's own `Escape` binding further out still runs.
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(KEY_CONTEXT)),
    ]);
}

/// Which gesture is asking a dialog to go away; see [`Modal::dismiss_guard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DismissReason {
    /// A press on the backdrop around the panel.
    Backdrop,
    /// `Escape` with the focus inside the dialog.
    Escape,
}

/// Callback fired when the dialog is dismissed.
type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Callback asked whether a dismissal may go ahead.
type DismissGuard = Rc<dyn Fn(DismissReason, &mut Window, &mut App) -> bool>;

/// Callback fired by `Enter`.
type ConfirmHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Space kept clear above and below the panel, in pixels, top and bottom
/// combined, inside the client-side shadow band if the window has one.
const VIEWPORT_MARGIN: f32 = 64.;

/// Floor for the panel's height cap, in pixels.
///
/// Without it a window shorter than [`VIEWPORT_MARGIN`] would cap the panel at
/// zero — or a negative value — and hide it entirely.
const MIN_PANEL_HEIGHT: f32 = 160.;

/// Floor for the panel's width cap, in pixels — [`MIN_PANEL_HEIGHT`] across.
const MIN_PANEL_WIDTH: f32 = 240.;

/// Height of the band the title sits in, at the default UI scale — the least
/// it is: a title that wraps makes it taller.
const TITLE_HEIGHT: f32 = 44.;

/// Builds a modal dialog.
///
/// The returned element positions itself absolutely, so it must be rendered
/// inside a `relative()` ancestor that spans the window — typically the root
/// element of the view — and it should be the last child so that it paints on
/// top of everything else.
///
/// Tab and Shift+Tab cycle through the topmost dialog's enabled controls.
/// Existing host key bindings keep their behavior within that boundary.
///
/// Clicks on the panel itself are swallowed; only clicks on the backdrop invoke
/// `on_dismiss`. The backdrop occludes everything under it, so while a dialog
/// is up the window behind it hears no press, wheel or hover: a click that
/// dismisses the dialog does not also land on whatever it was drawn over.
///
/// The panel is as tall as its content but never taller than the window, so a
/// `body` that can grow past that should put its own scroll area inside.
///
/// ```ignore
/// modal("connect", "New connection", px(420.), body, cx.listener(..))
///     .initial_focus(&self.connect_focus)
///     .on_confirm(cx.listener(|this, _, window, cx| this.connect(window, cx)))
///     .escape_dismisses(true)
/// ```
pub fn modal<E: IntoElement>(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    width: Pixels,
    body: E,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Modal {
    Modal {
        id: id.into(),
        title: title.into(),
        width,
        body: body.into_any_element(),
        on_dismiss: Rc::new(on_dismiss),
        backdrop_dismisses: true,
        escape_dismisses: false,
        guard: None,
        on_confirm: None,
        initial_focus: None,
        autofocus: false,
    }
}

/// The dialog [`modal`] builds, with the options it can be given.
#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    title: SharedString,
    width: Pixels,
    body: AnyElement,
    on_dismiss: DismissHandler,
    backdrop_dismisses: bool,
    escape_dismisses: bool,
    guard: Option<DismissGuard>,
    on_confirm: Option<ConfirmHandler>,
    initial_focus: Option<FocusHandle>,
    autofocus: bool,
}

impl Modal {
    /// Whether a press on the backdrop dismisses the dialog. On by default.
    ///
    /// Off, the backdrop still dims the window and still keeps the pointer
    /// from it; it simply does nothing when pressed. For a dialog whose input
    /// would be lost to a click that missed the panel by a pixel.
    pub fn backdrop_dismisses(mut self, dismisses: bool) -> Self {
        self.backdrop_dismisses = dismisses;
        self
    }

    /// Asks `guard` before the backdrop or `Escape` dismisses the dialog; the
    /// dismissal goes ahead only when it answers `true`.
    ///
    /// The guard is where "this dialog holds changes" is decided, and it may do
    /// more than answer: a guard that answers `false` to [`DismissReason::Escape`]
    /// can open its own "discard the changes?" question first. A Close or
    /// Cancel button in the body is the host's own and is not asked about.
    pub fn dismiss_guard(
        mut self,
        guard: impl Fn(DismissReason, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.guard = Some(Rc::new(guard));
        self
    }

    /// Whether `Escape` pressed inside the dialog dismisses it, through the
    /// [`Modal::dismiss_guard`] when there is one. Off by default, which leaves
    /// the key to whatever the host bound it to further out.
    pub fn escape_dismisses(mut self, dismisses: bool) -> Self {
        self.escape_dismisses = dismisses;
        self
    }

    /// Runs `handler` when `Enter` is pressed inside the dialog, as its
    /// default action.
    ///
    /// Only where nothing else wants the key: on the panel itself, or in a
    /// one-line [`crate::TextInput`] that has no `on_submit` of its own. A
    /// focused button is activated instead, a multi-line field takes a line
    /// break and an open dropdown takes its row, so a dialog that focuses its
    /// Cancel button gets Cancel from `Enter`, which is the point of focusing
    /// it.
    pub fn on_confirm(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    /// Focuses `handle` on the frame the dialog first appears.
    ///
    /// A control in the body tracks the handle — [`crate::Button::focus_handle`]
    /// or a field's own — and the host keeps it beside the rest of the
    /// dialog's state. The focus the window had before is given back when the
    /// dialog goes away, unless the host has moved it elsewhere in the
    /// meantime.
    pub fn initial_focus(mut self, handle: &FocusHandle) -> Self {
        self.initial_focus = Some(handle.clone());
        self
    }

    /// Focuses the panel itself on the frame the dialog first appears, so that
    /// `Enter`, `Escape` and `Tab` reach the dialog rather than whatever was
    /// focused behind it. Ignored when [`Modal::initial_focus`] names a
    /// control; the focus is given back the same way.
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }
}

/// What a dialog remembers about the focus between frames.
///
/// Element state, so it lives exactly as long as the dialog is drawn: a dialog
/// the host stops rendering drops it, and dropping it is the moment the focus is
/// given back.
struct ModalFocus {
    /// The panel's own handle, which `Enter` and `Escape` are answered at and
    /// which [`Modal::autofocus`] focuses.
    panel: FocusHandle,
    /// The window the dialog is drawn in.
    window: AnyWindowHandle,
    /// Whether the opening frame has been drawn.
    opened: bool,
    /// The focus before the dialog took it, given back when it goes.
    previous: Option<FocusHandle>,
    /// Whether the dialog took the focus at all; one that never did has
    /// nothing to give back.
    took_focus: bool,
    /// The last focus seen inside the dialog. Still being the window's focus
    /// once the dialog has gone means nobody has moved it since.
    last_inside: Option<FocusHandle>,
}

impl ModalFocus {
    fn new(window: &Window, cx: &mut Context<Self>) -> Self {
        cx.on_release(|state, cx| state.give_back(cx)).detach();
        Self {
            panel: cx.focus_handle(),
            window: window.window_handle(),
            opened: false,
            previous: None,
            took_focus: false,
            last_inside: None,
        }
    }

    /// Hands the focus back now, for a dismissal the dialog itself performs:
    /// the host's `on_dismiss` runs after this, so whatever it focuses wins,
    /// and the release has nothing left to do.
    fn give_back_now(&mut self, window: &mut Window, cx: &mut App) {
        if !std::mem::take(&mut self.took_focus) {
            return;
        }
        if let Some(previous) = self.previous.take() {
            previous.focus(window, cx);
        }
    }

    /// Hands the focus back to where it was before the dialog took it, if it
    /// is still on something the dialog held — or on nothing at all, which is
    /// where a focus inside a dialog that has gone ends up.
    fn give_back(&mut self, cx: &mut App) {
        if !self.took_focus {
            return;
        }
        let Some(previous) = self.previous.take() else {
            return;
        };
        let last_inside = self.last_inside.take();
        let panel = self.panel.clone();
        let window = self.window;
        // Deferred: a release can arrive while the window is being drawn.
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let lost = match window.focused(cx) {
                    None => true,
                    Some(current) => current == panel || last_inside.as_ref() == Some(&current),
                };
                if lost {
                    previous.focus(window, cx);
                }
            });
        });
    }
}

/// Whether `handler`, if any, lets a `reason` dismissal go ahead.
fn allowed(
    guard: &Option<DismissGuard>,
    reason: DismissReason,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    guard.as_ref().is_none_or(|guard| guard(reason, window, cx))
}

impl RenderOnce for Modal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let max_height =
            overlay::max_height(window, px(VIEWPORT_MARGIN / 2.)).max(px(MIN_PANEL_HEIGHT));
        // The width the caller asked for — which an application at a large UI
        // scale may have scaled past what the window holds — never wider than
        // the window less the same margin kept above and below.
        let max_width =
            overlay::max_width(window, px(VIEWPORT_MARGIN / 2.)).max(px(MIN_PANEL_WIDTH));

        let focus = window.use_keyed_state((self.id.clone(), "focus"), cx, |window, cx| {
            ModalFocus::new(window, cx)
        });
        let panel = focus.update(cx, |state, cx| {
            if !state.opened {
                state.opened = true;
                state.previous = window.focused(cx);
                let target = self
                    .initial_focus
                    .clone()
                    .or_else(|| self.autofocus.then(|| state.panel.clone()));
                if let Some(target) = target {
                    state.took_focus = true;
                    window.defer(cx, move |window, cx| target.focus(window, cx));
                }
            } else if state.panel.contains_focused(window, cx) {
                state.last_inside = window.focused(cx);
            }
            state.panel.clone()
        });

        let on_dismiss = self.on_dismiss;
        let guard = self.guard;
        let backdrop_dismisses = self.backdrop_dismisses;
        let escape_dismisses = self.escape_dismisses;
        let on_confirm = self.on_confirm;
        let confirm_panel = panel.clone();

        div()
            .id(self.id.clone())
            .absolute()
            .inset_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.overlay)
            // The window behind a dialog is not to be used while the dialog is
            // up: without this the dimmed surfaces still take the pointer, so
            // a wheel scrolls them and a press on the backdrop reaches them as
            // well as dismissing the dialog.
            .occlude()
            .when(backdrop_dismisses, |this| {
                let on_dismiss = on_dismiss.clone();
                let guard = guard.clone();
                let focus = focus.clone();
                this.on_click(move |_, window, cx| {
                    if allowed(&guard, DismissReason::Backdrop, window, cx) {
                        focus.update(cx, |state, cx| state.give_back_now(window, cx));
                        on_dismiss(window, cx);
                    }
                })
            })
            .child(
                div()
                    .id(ElementId::from((self.id, "panel")))
                    .key_context(KEY_CONTEXT)
                    .track_focus(&panel)
                    .occlude()
                    .tab_group()
                    .focus_trap()
                    // `Enter` is the dialog's only where nothing more specific
                    // wants it — see `Modal::on_confirm`. Passing it on is what
                    // lets a focused button turn it into its own click.
                    .on_action(move |_: &Confirm, window, cx| {
                        let Some(confirm) = on_confirm.as_ref() else {
                            cx.propagate();
                            return;
                        };
                        let in_field = window
                            .context_stack()
                            .last()
                            .is_some_and(|context| context.contains(FIELD_CONTEXT));
                        if confirm_panel.is_focused(window) || in_field {
                            confirm(window, cx);
                        } else {
                            cx.propagate();
                        }
                    })
                    .on_action(move |_: &Cancel, window, cx| {
                        if !escape_dismisses {
                            cx.propagate();
                            return;
                        }
                        if allowed(&guard, DismissReason::Escape, window, cx) {
                            focus.update(cx, |state, cx| state.give_back_now(window, cx));
                            on_dismiss(window, cx);
                        }
                    })
                    .flex()
                    .flex_col()
                    .w(self.width)
                    .max_w(max_width)
                    .max_h(max_height)
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_lg()
                    .shadow_lg()
                    .text_color(theme.text)
                    .child(
                        // At least the band's height, and taller when a long
                        // title wraps: a title that cannot wrap runs out of
                        // the panel, past its border.
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .px(px(16.))
                            .py(px(8.))
                            .min_h(scaled(TITLE_HEIGHT, cx))
                            .border_b_1()
                            .border_color(theme.border)
                            .text_size(TextSize::Title.size(cx))
                            .debug_selector(|| "modal-title".to_string())
                            .child(div().min_w_0().child(self.title)),
                    )
                    .child(
                        // `min_h_0` is what lets the body shrink once the panel
                        // hits `max_height`: a flex item's default minimum size
                        // is its content, which would push the panel past the
                        // cap instead of handing the overflow to a scroll area
                        // inside the body.
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_h_0()
                            .gap(px(12.))
                            .p(px(16.))
                            .child(self.body),
                    ),
            )
    }
}

/// The focus to go back to once an overlay of the host's own has closed.
///
/// [`Modal::initial_focus`] and [`Modal::autofocus`] give the focus back by
/// themselves. A surface that is not a [`modal`] — a panel the host slides in,
/// a find bar — captures the focus as it opens and restores it as it closes:
///
/// ```ignore
/// self.returning = Some(FocusReturn::capture(window, cx));
/// // … and when it closes:
/// if let Some(focus) = self.returning.take() { focus.restore(window, cx) }
/// ```
#[derive(Debug, Clone)]
pub struct FocusReturn(Option<FocusHandle>);

impl FocusReturn {
    /// Remembers what has the focus in `window` now.
    pub fn capture(window: &Window, cx: &App) -> Self {
        Self(window.focused(cx))
    }

    /// The remembered handle, if anything had the focus.
    pub fn handle(&self) -> Option<&FocusHandle> {
        self.0.as_ref()
    }

    /// Focuses the remembered handle again. Nothing happens when nothing had
    /// the focus.
    pub fn restore(self, window: &mut Window, cx: &mut App) {
        if let Some(handle) = self.0 {
            handle.focus(window, cx);
        }
    }
}

/// Width of a [`form_row`]'s label column at the default UI scale, and the
/// floor [`fit_label_width`] never goes under.
pub const FORM_LABEL_WIDTH: f32 = 96.;

/// The widest [`fit_label_width`] makes a label column, at the default UI
/// scale.
///
/// A label is the name of a field, not a sentence about it: past this it wraps
/// onto a second line, at a space, rather than leaving the field beside it too
/// narrow to type into.
pub const FORM_LABEL_MAX_WIDTH: f32 = 200.;

/// Builds a labeled form row: a label column followed by `control`.
///
/// Intended for the body of a [`modal`], but usable anywhere a label/control
/// pair is needed. The label column is [`FORM_LABEL_WIDTH`] wide unless
/// [`FormRow::label_width`] says otherwise — what [`fit_label_width`] measures
/// for a whole form — and [`FormRow::stacked`] puts the label above the
/// control instead.
pub fn form_row<E: IntoElement>(label: impl Into<SharedString>, control: E) -> FormRow {
    FormRow {
        label: label.into(),
        control: control.into_any_element(),
        label_width: None,
        stacked: false,
    }
}

/// The row [`form_row`] builds.
#[derive(IntoElement)]
pub struct FormRow {
    label: SharedString,
    control: AnyElement,
    label_width: Option<Pixels>,
    stacked: bool,
}

impl FormRow {
    /// Draws the label column `width` wide instead of [`FORM_LABEL_WIDTH`].
    ///
    /// Give every row of a form the same width — the one [`fit_label_width`]
    /// measured over all their labels — or the controls stop lining up.
    pub fn label_width(mut self, width: Pixels) -> Self {
        self.label_width = Some(width);
        self
    }

    /// Puts the label on a line of its own above the control, which then has
    /// the whole width of the row.
    ///
    /// For a form too narrow for a label column — a side panel, a translation
    /// whose labels are sentences.
    pub fn stacked(mut self, stacked: bool) -> Self {
        self.stacked = stacked;
        self
    }

    /// The label text, for a form measuring its own labels.
    pub fn label_text(&self) -> &SharedString {
        &self.label
    }
}

impl RenderOnce for FormRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let label = div()
            .text_size(TextSize::Body.size(cx))
            .text_color(theme.text_muted)
            .child(self.label);

        if self.stacked {
            return div()
                .flex()
                .flex_col()
                .w_full()
                .gap(px(4.))
                .child(label)
                .child(div().w_full().min_w_0().child(self.control));
        }

        div()
            .flex()
            .flex_row()
            // The containing form supplies the width. Avoid measuring the
            // control subtree again just to discover the row's intrinsic width.
            .w_full()
            .items_center()
            .gap(px(12.))
            .child(
                label.flex_none().w(self
                    .label_width
                    .unwrap_or_else(|| scaled(FORM_LABEL_WIDTH, cx))),
            )
            .child(div().flex_grow_1().min_w_0().child(self.control))
    }
}

/// The label column width that fits the widest of `labels` on one line, held
/// between [`FORM_LABEL_WIDTH`] and [`FORM_LABEL_MAX_WIDTH`] at the UI scale.
///
/// Measured with the text system at the size [`form_row`] draws its labels at,
/// so a form whose labels are short keeps the column it always had, and a
/// translation whose longest label is "Verbindungszeitlimit" gets a column it
/// fits in rather than one that breaks it in the middle of the word:
///
/// ```ignore
/// let width = fit_label_width([ts!("host"), ts!("port"), ts!("timeout")], window, cx);
/// form_row(ts!("host"), host).label_width(width)
/// ```
pub fn fit_label_width<L: AsRef<str>>(
    labels: impl IntoIterator<Item = L>,
    window: &Window,
    cx: &App,
) -> Pixels {
    let size = TextSize::Body.size(cx);
    let font = window.text_style().font();
    let widest = labels
        .into_iter()
        .map(|label| shaped_width(label.as_ref(), &font, size, window))
        .fold(0., f32::max);
    // A pixel past the shaped width: a box exactly as wide as its text wraps
    // the last glyph on a rounding error.
    px((widest + 1.).ceil()).clamp(
        scaled(FORM_LABEL_WIDTH, cx),
        scaled(FORM_LABEL_MAX_WIDTH, cx),
    )
}

/// How wide `text` comes out in `font` at `size`.
fn shaped_width(text: &str, font: &Font, size: Pixels, window: &Window) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), size, &[run], None)
        .width
        .into()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::{Button, TextInput};
    use gpui::{
        Context, Entity, FocusHandle, Focusable, Render, TestAppContext, VisualTestContext,
    };

    struct Harness {
        root: FocusHandle,
        background: Entity<TextInput>,
        first: Entity<TextInput>,
        second: Entity<TextInput>,
        disabled: Entity<TextInput>,
        show_dialog: bool,
        top_overlay: bool,
        top_field: Entity<TextInput>,
        clicks: usize,
        tab_handling: u8,
    }

    struct FormLayout {
        body: gpui::ScrollHandle,
        controls: [gpui::ScrollHandle; 2],
    }

    impl Render for FormLayout {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let body = div()
                .id("form-body")
                .track_scroll(&self.body)
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(self.controls.iter().enumerate().map(|(index, bounds)| {
                    form_row(
                        "Label",
                        div()
                            .id(("form-control", index))
                            .track_scroll(bounds)
                            .h(px(28.)),
                    )
                }));
            div()
                .size_full()
                .child(modal("form", "Form", px(420.), body, |_, _| {}))
        }
    }

    #[gpui::test]
    fn form_rows_share_the_padded_body_width(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| FormLayout {
            body: gpui::ScrollHandle::new(),
            controls: [gpui::ScrollHandle::new(), gpui::ScrollHandle::new()],
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        for (width, height) in [(800., 600.), (500., 400.)] {
            cx.simulate_resize(gpui::size(px(width), px(height)));
            cx.run_until_parked();
            window
                .update(&mut cx, |form, _, _| {
                    let body = form.body.bounds();
                    let first = form.controls[0].bounds();
                    let second = form.controls[1].bounds();
                    // The modal's border and padding stay outside the form.
                    assert_eq!(body.size.width, px(420. - 2. - 32.));
                    assert_eq!(first.left(), body.left() + px(96. + 12.));
                    assert_eq!(first.right(), body.right());
                    assert_eq!(first.size.width, second.size.width);
                    assert_eq!(first.left(), second.left());
                    assert!(second.top() >= first.bottom() + px(8.));
                })
                .unwrap();
        }
    }

    /// A form taller than any window, inside a client-side frame: the content
    /// sits inside the shadow band, as `rugpui-shell`'s chrome lays it out.
    struct Framed {
        content: gpui::ScrollHandle,
        footer: gpui::ScrollHandle,
    }

    const BAND: f32 = 36.;

    impl Render for Framed {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            window.set_client_inset(px(BAND));
            let body = div()
                .flex()
                .flex_col()
                .min_h_0()
                .child(
                    div()
                        .id("framed-body")
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(div().flex_none().h(px(2000.))),
                )
                .child(
                    div()
                        .id("framed-footer")
                        .track_scroll(&self.footer)
                        .flex_none()
                        .h(px(32.)),
                );
            div().size_full().p(px(BAND)).child(
                div()
                    .id("framed-content")
                    .track_scroll(&self.content)
                    .relative()
                    .size_full()
                    .child(modal("framed", "Framed", px(420.), body, |_, _| {})),
            )
        }
    }

    /// The panel's cap leaves out the shadow band, so a tall dialog in a short
    /// window keeps its margin inside the content instead of being cut off at
    /// the top and bottom.
    #[gpui::test]
    fn a_tall_dialog_stays_inside_the_shadow_band(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| Framed {
            content: gpui::ScrollHandle::new(),
            footer: gpui::ScrollHandle::new(),
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        for height in [700., 480., 320.] {
            cx.simulate_resize(gpui::size(px(800.), px(height)));
            cx.run_until_parked();
            window
                .update(&mut cx, |framed, _, _| {
                    let content = framed.content.bounds();
                    let footer = framed.footer.bounds();
                    assert!(
                        content.bottom() - footer.bottom() >= px(VIEWPORT_MARGIN / 2.),
                        "at {height} px the footer ends {} px above the content edge",
                        f32::from(content.bottom() - footer.bottom())
                    );
                })
                .unwrap();
        }
    }

    /// A dialog as wide as it was asked to be.
    struct Wide(f32);

    impl Render for Wide {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(modal("wide", "Title", px(self.0), div(), |_, _| {}))
        }
    }

    /// A width that fits is drawn as asked; one wider than the window — what
    /// a dialog scaled for a large interface asks for — stops at the window
    /// less the margin, so the panel never runs off either side.
    #[gpui::test]
    fn a_dialog_is_never_wider_than_its_window(cx: &mut TestAppContext) {
        cx.update(crate::init);
        for (asked, drawn) in [(420., 420.), (1700., 1200. - VIEWPORT_MARGIN)] {
            let window = cx.add_window(move |_, _| Wide(asked));
            let mut cx = VisualTestContext::from_window(window.into(), cx);
            cx.simulate_resize(gpui::size(px(1200.), px(800.)));
            cx.run_until_parked();
            let title = cx.debug_bounds("modal-title").expect("the title band");
            // The title band fills the panel inside its one-pixel border.
            assert_eq!(
                title.size.width,
                px(drawn - 2.),
                "asked for {asked} px in a 1200 px window"
            );
        }
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let owner = cx.entity();
            let tab_handling = self.tab_handling;
            let form = div()
                .child(self.first.clone())
                .child(self.disabled.clone())
                .child(self.second.clone())
                .child(Button::new("disabled-button", "Disabled").disabled(true))
                .child(Button::new("accept", "Accept").on_click(move |_, _, cx| {
                    owner.update(cx, |harness, _| harness.clicks += 1);
                }));
            div()
                .relative()
                .size_full()
                .track_focus(&self.root)
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "tab" {
                        match tab_handling {
                            1 => cx.stop_propagation(),
                            2 => window.prevent_default(),
                            _ => {}
                        }
                    }
                })
                .child(self.background.clone())
                .map(|root| {
                    if !self.show_dialog {
                        return root.child(form);
                    }
                    root.child(modal("test-modal", "Dialog", px(400.), form, |_, _| {}))
                })
                .when(self.top_overlay, |root| {
                    root.child(modal(
                        "front-modal",
                        "Confirm",
                        px(300.),
                        self.top_field.clone(),
                        |_, _| {},
                    ))
                })
        }
    }

    fn activate(cx: &mut VisualTestContext) {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }

    #[gpui::test]
    fn tab_enters_and_wraps_a_dialog_and_skips_disabled_controls(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, cx| Harness {
            root: cx.focus_handle(),
            background: cx.new(TextInput::new),
            first: cx.new(TextInput::new),
            second: cx.new(TextInput::new),
            disabled: cx.new(|cx| TextInput::new(cx).disabled(true)),
            show_dialog: true,
            top_overlay: false,
            top_field: cx.new(TextInput::new),
            clicks: 0,
            tab_handling: 0,
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(&mut cx, |harness, window, cx| {
                harness.root.focus(window, cx)
            })
            .unwrap();
        cx.simulate_keystrokes("tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(harness.first.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        cx.simulate_keystrokes("tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(harness.second.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        cx.simulate_keystrokes("tab");
        activate(&mut cx);
        window
            .update(&mut cx, |harness, _, _| assert_eq!(harness.clicks, 1))
            .unwrap();
        cx.simulate_keystrokes("tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(harness.first.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        cx.simulate_keystrokes("shift-tab");
        activate(&mut cx);
        cx.simulate_keystrokes("shift-tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert_eq!(harness.clicks, 2);
                assert!(harness.second.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        // A second overlay owns the ring until it disappears.
        window
            .update(&mut cx, |harness, window, cx| {
                harness.top_overlay = true;
                harness.root.focus(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes("tab tab shift-tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(
                    harness
                        .top_field
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
                harness.top_overlay = false;
                harness.root.focus(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes("tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(harness.first.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();

        // Existing host actions that call focus_next/prev must obey the same boundary.
        window
            .update(&mut cx, |harness, window, cx| {
                harness.first.read(cx).focus_handle(cx).focus(window, cx);
                window.focus_prev(cx);
                window.focus_next(cx);
                assert!(harness.first.read(cx).focus_handle(cx).is_focused(window));
                harness.show_dialog = false;
                harness
                    .background
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(&mut cx, |harness, window, cx| {
                window.focus_next(cx);
                assert!(harness.first.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
    }

    #[gpui::test]
    fn tab_traverses_nonmodal_forms_without_host_bindings(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, cx| Harness {
            root: cx.focus_handle(),
            background: cx.new(TextInput::new),
            first: cx.new(TextInput::new),
            second: cx.new(TextInput::new),
            disabled: cx.new(|cx| TextInput::new(cx).disabled(true)),
            show_dialog: false,
            top_overlay: false,
            top_field: cx.new(TextInput::new),
            clicks: 0,
            tab_handling: 0,
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        window
            .update(&mut cx, |harness, window, cx| {
                harness.root.focus(window, cx)
            })
            .unwrap();
        cx.run_until_parked();

        for field in [0, 1, 2] {
            cx.simulate_keystrokes("tab");
            window
                .update(&mut cx, |harness, window, cx| {
                    let input = [&harness.background, &harness.first, &harness.second][field];
                    assert!(input.read(cx).focus_handle(cx).is_focused(window));
                })
                .unwrap();
        }
        cx.simulate_keystrokes("tab");
        activate(&mut cx);
        cx.simulate_keystrokes("tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert_eq!(harness.clicks, 1);
                assert!(
                    harness
                        .background
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
        cx.simulate_keystrokes("shift-tab");
        activate(&mut cx);
        cx.simulate_keystrokes("shift-tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert_eq!(harness.clicks, 2);
                assert!(harness.second.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        cx.simulate_keystrokes("ctrl-tab alt-tab");
        window
            .update(&mut cx, |harness, window, cx| {
                assert!(harness.second.read(cx).focus_handle(cx).is_focused(window));
            })
            .unwrap();
        // Editors and terminals can consume Tab before the fallback runs.
        for tab_handling in [1, 2] {
            window
                .update(&mut cx, |harness, _, cx| {
                    harness.tab_handling = tab_handling;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            cx.simulate_keystrokes("tab shift-tab");
            window
                .update(&mut cx, |harness, window, cx| {
                    assert!(harness.second.read(cx).focus_handle(cx).is_focused(window));
                })
                .unwrap();
        }
    }

    /// A surface behind a dialog, counting what reaches it.
    struct Behind {
        presses: Rc<Cell<usize>>,
        wheels: Rc<Cell<usize>>,
        dismissed: Rc<Cell<usize>>,
    }

    impl Render for Behind {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (presses, wheels, dismissed) = (
                self.presses.clone(),
                self.wheels.clone(),
                self.dismissed.clone(),
            );
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("behind")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, _| {
                            presses.set(presses.get() + 1);
                        })
                        .on_scroll_wheel(move |_, _, _| wheels.set(wheels.get() + 1)),
                )
                .child(modal("blocking", "Dialog", px(200.), div().h(px(40.)), {
                    move |_, _| dismissed.set(dismissed.get() + 1)
                }))
        }
    }

    /// The backdrop keeps the pointer from the window behind it: a press on
    /// it dismisses the dialog and reaches nothing else, and a wheel over it
    /// scrolls nothing.
    #[gpui::test]
    fn the_backdrop_keeps_the_pointer_from_the_window_behind(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let presses = Rc::new(Cell::new(0));
        let wheels = Rc::new(Cell::new(0));
        let dismissed = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let (presses, wheels, dismissed) = (presses.clone(), wheels.clone(), dismissed.clone());
            move |_, _| Behind {
                presses,
                wheels,
                dismissed,
            }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(px(600.), px(400.)));
        cx.run_until_parked();

        // A corner of the window: backdrop, well clear of the panel.
        let corner = gpui::point(px(10.), px(10.));
        cx.simulate_click(corner, gpui::Modifiers::none());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: corner,
            delta: gpui::ScrollDelta::Lines(gpui::point(0., 3.)),
            ..Default::default()
        });
        cx.run_until_parked();

        assert_eq!(
            dismissed.get(),
            1,
            "the backdrop did not dismiss the dialog"
        );
        assert_eq!(
            presses.get(),
            0,
            "a press reached the window behind the dialog"
        );
        assert_eq!(
            wheels.get(),
            0,
            "a wheel reached the window behind the dialog"
        );
    }

    gpui::actions!(modal_test, [HostEscape]);

    /// A dialog with every option set, over a field that holds the focus
    /// before it opens — the editor a confirmation is raised over.
    struct Options {
        behind: Entity<TextInput>,
        field: Entity<TextInput>,
        cancel: FocusHandle,
        open: bool,
        focus_cancel: bool,
        escape: bool,
        backdrop: bool,
        allow: bool,
        confirmed: usize,
        dismissed: usize,
        cancels: usize,
        asked: Vec<DismissReason>,
        host_escapes: usize,
    }

    impl Render for Options {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let owner = cx.entity();
            let body = div().child(self.field.clone()).child(
                Button::new("cancel", "Cancel")
                    .focus_handle(&self.cancel)
                    .on_click({
                        let owner = owner.clone();
                        move |_, _, cx| owner.update(cx, |this, _| this.cancels += 1)
                    }),
            );
            let dialog = modal("options", "Options", px(300.), body, {
                let owner = owner.clone();
                move |_, cx| {
                    owner.update(cx, |this, cx| {
                        this.dismissed += 1;
                        this.open = false;
                        cx.notify();
                    })
                }
            })
            .backdrop_dismisses(self.backdrop)
            .escape_dismisses(self.escape)
            .dismiss_guard({
                let owner = owner.clone();
                move |reason, _, cx| {
                    owner.update(cx, |this, _| {
                        this.asked.push(reason);
                        this.allow
                    })
                }
            })
            .on_confirm({
                let owner = owner.clone();
                move |_, cx| owner.update(cx, |this, _| this.confirmed += 1)
            })
            .map(|dialog| {
                if self.focus_cancel {
                    dialog.initial_focus(&self.cancel)
                } else {
                    dialog.autofocus(true)
                }
            });
            div()
                .relative()
                .size_full()
                .key_context("ModalOptionsHost")
                .on_action(cx.listener(|this, _: &HostEscape, _, _| this.host_escapes += 1))
                .child(self.behind.clone())
                .when(self.open, |root| root.child(dialog))
        }
    }

    fn options(cx: &mut TestAppContext) -> (Entity<Options>, VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.bind_keys([gpui::KeyBinding::new(
                "escape",
                HostEscape,
                Some("ModalOptionsHost"),
            )]);
        });
        let window = cx.add_window(|_, cx| Options {
            behind: cx.new(TextInput::new),
            field: cx.new(TextInput::new),
            cancel: cx.focus_handle(),
            open: false,
            focus_cancel: false,
            escape: true,
            backdrop: true,
            allow: true,
            confirmed: 0,
            dismissed: 0,
            cancels: 0,
            asked: Vec::new(),
            host_escapes: 0,
        });
        let entity = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(px(600.), px(400.)));
        cx.update(|window, cx| {
            let behind = entity.read(cx).behind.read(cx).focus_handle(cx);
            behind.focus(window, cx);
        });
        cx.run_until_parked();
        (entity, cx)
    }

    fn set(entity: &Entity<Options>, cx: &mut VisualTestContext, f: impl FnOnce(&mut Options)) {
        entity.update(cx, |this, cx| {
            f(this);
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn behind_focused(entity: &Entity<Options>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| {
            entity
                .read(cx)
                .behind
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        })
    }

    /// `autofocus` puts the keys in the dialog: `Enter` on the panel is the
    /// default action, and the editor behind hears nothing. Closing gives the
    /// focus back to it.
    #[gpui::test]
    fn autofocus_takes_the_keys_and_gives_the_focus_back(cx: &mut TestAppContext) {
        let (entity, mut cx) = options(cx);
        set(&entity, &mut cx, |this| this.open = true);
        assert!(!behind_focused(&entity, &mut cx));
        activate(&mut cx);
        cx.run_until_parked();
        assert_eq!(entity.read_with(&cx, |this, _| this.confirmed), 1);
        assert_eq!(
            entity.read_with(&cx, |this, cx| this.behind.read(cx).content().len()),
            0
        );

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        entity.read_with(&cx, |this, _| {
            assert_eq!(this.dismissed, 1);
            assert_eq!(this.asked, [DismissReason::Escape]);
            assert_eq!(this.host_escapes, 0, "the dialog kept the key");
        });
        assert!(behind_focused(&entity, &mut cx), "the focus came back");

        // Closed by the host rather than by the dialog: given back once the
        // dialog's state is released, which the next update after the frame
        // that left it out performs.
        set(&entity, &mut cx, |this| this.open = true);
        assert!(!behind_focused(&entity, &mut cx));
        set(&entity, &mut cx, |this| this.open = false);
        cx.update(|_, _| {});
        cx.run_until_parked();
        assert!(
            behind_focused(&entity, &mut cx),
            "the focus came back again"
        );
    }

    /// The focused control keeps `Enter`: on a focused Cancel it is Cancel,
    /// and in a field without a submit of its own it is the dialog's.
    #[gpui::test]
    fn enter_belongs_to_the_focused_button_before_the_dialog(cx: &mut TestAppContext) {
        let (entity, mut cx) = options(cx);
        set(&entity, &mut cx, |this| {
            this.focus_cancel = true;
            this.open = true;
        });
        assert!(cx.update(|window, cx| entity.read(cx).cancel.is_focused(window)));
        activate(&mut cx);
        cx.run_until_parked();
        entity.read_with(&cx, |this, _| {
            assert_eq!(this.cancels, 1);
            assert_eq!(this.confirmed, 0);
        });

        cx.update(|window, cx| {
            let field = entity.read(cx).field.read(cx).focus_handle(cx);
            field.focus(window, cx);
        });
        cx.run_until_parked();
        activate(&mut cx);
        cx.run_until_parked();
        assert_eq!(entity.read_with(&cx, |this, _| this.confirmed), 1);
    }

    /// The guard is asked for both gestures and can refuse either, and a
    /// dialog with neither option set leaves `Escape` to the host and the
    /// backdrop to nothing.
    #[gpui::test]
    fn the_guard_and_the_switches_decide_a_dismissal(cx: &mut TestAppContext) {
        let (entity, mut cx) = options(cx);
        let corner = gpui::point(px(5.), px(5.));
        set(&entity, &mut cx, |this| {
            this.allow = false;
            this.open = true;
        });
        cx.simulate_click(corner, gpui::Modifiers::none());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        entity.read_with(&cx, |this, _| {
            assert_eq!(this.dismissed, 0);
            assert_eq!(this.asked, [DismissReason::Backdrop, DismissReason::Escape]);
        });

        set(&entity, &mut cx, |this| {
            this.allow = true;
            this.backdrop = false;
            this.escape = false;
        });
        cx.simulate_click(corner, gpui::Modifiers::none());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        entity.read_with(&cx, |this, _| {
            assert_eq!(this.dismissed, 0, "neither gesture is the dialog's");
            assert_eq!(this.asked.len(), 2, "and the guard is not asked");
            assert_eq!(this.host_escapes, 1, "Escape reached the host");
        });
    }

    /// The fitted column is the widest label's, between the floor the fixed
    /// column always had and the ceiling.
    #[gpui::test]
    fn a_fitted_label_column_is_held_between_its_bounds(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| FormLayout {
            body: gpui::ScrollHandle::new(),
            controls: [gpui::ScrollHandle::new(), gpui::ScrollHandle::new()],
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, cx| {
            assert_eq!(
                fit_label_width(["Host", "Port"], window, cx),
                px(FORM_LABEL_WIDTH)
            );
            assert_eq!(
                fit_label_width(Vec::<&str>::new(), window, cx),
                px(FORM_LABEL_WIDTH)
            );
            let long = "Bei einer Zeile, die nicht eingefügt werden kann";
            assert_eq!(
                fit_label_width([long], window, cx),
                px(FORM_LABEL_MAX_WIDTH)
            );
            let word = fit_label_width(["Host", "Verbindungszeitlimit"], window, cx);
            assert!(word > px(FORM_LABEL_WIDTH) && word < px(FORM_LABEL_MAX_WIDTH));
            crate::set_ui_scale(1.5, cx);
            assert_eq!(
                fit_label_width(["Host"], window, cx),
                px(FORM_LABEL_WIDTH * 1.5)
            );
        });
    }

    /// A stacked row gives the control the whole width, under its label.
    struct Stacked {
        control: gpui::ScrollHandle,
        body: gpui::ScrollHandle,
    }

    impl Render for Stacked {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("stacked-body")
                    .track_scroll(&self.body)
                    .w(px(300.))
                    .child(
                        form_row(
                            "Label",
                            div()
                                .id("stacked-control")
                                .track_scroll(&self.control)
                                .h(px(20.)),
                        )
                        .stacked(true),
                    ),
            )
        }
    }

    #[gpui::test]
    fn a_stacked_row_puts_the_control_under_its_label(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| Stacked {
            control: gpui::ScrollHandle::new(),
            body: gpui::ScrollHandle::new(),
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(&mut cx, |stacked, _, _| {
                let body = stacked.body.bounds();
                let control = stacked.control.bounds();
                assert_eq!(control.left(), body.left());
                assert_eq!(control.size.width, px(300.));
                assert!(control.top() > body.top());
            })
            .unwrap();
    }

    /// A title too long for the panel wraps inside the band, which grows to
    /// hold it, instead of running past the panel's border.
    #[gpui::test]
    fn a_long_title_wraps_inside_the_panel(cx: &mut TestAppContext) {
        struct Titled(&'static str);
        impl Render for Titled {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .child(modal("titled", self.0, px(240.), div(), |_, _| {}))
            }
        }
        cx.update(crate::init);
        let band = |title: &'static str, cx: &mut TestAppContext| {
            let window = cx.add_window(|_, _| Titled(title));
            let mut cx = VisualTestContext::from_window(window.into(), cx);
            cx.simulate_resize(gpui::size(px(800.), px(600.)));
            cx.run_until_parked();
            cx.debug_bounds("modal-title")
                .expect("the title band is drawn")
        };
        let short = band("Export set", cx);
        let long = band(
            "Benutzerdefinierte Variablen dieser Verbindung in die exportierte Datei aufnehmen",
            cx,
        );
        assert_eq!(
            long.size.width, short.size.width,
            "the band keeps the panel's width"
        );
        assert!(
            long.size.height > short.size.height + px(10.),
            "the long title did not wrap: {short:?} vs {long:?}"
        );
    }
}
