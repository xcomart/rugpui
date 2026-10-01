//! Centered dialog rendered on top of a translucent backdrop.

use gpui::{AnyElement, App, ElementId, Pixels, SharedString, Window, div, prelude::*, px};

use crate::theme::theme;

/// Callback fired when the backdrop is clicked.
type DismissHandler = Box<dyn Fn(&mut Window, &mut App)>;

/// Space kept clear above and below the panel, in pixels, top and bottom
/// combined.
const VIEWPORT_MARGIN: f32 = 64.;

/// Floor for the panel's height cap, in pixels.
///
/// Without it a window shorter than [`VIEWPORT_MARGIN`] would cap the panel at
/// zero — or a negative value — and hide it entirely.
const MIN_PANEL_HEIGHT: f32 = 160.;

/// Builds a modal dialog.
///
/// The returned element positions itself absolutely, so it must be rendered
/// inside a `relative()` ancestor that spans the window — typically the root
/// element of the view — and it should be the last child so that it paints on
/// top of everything else.
///
/// Tab and Shift+Tab cycle through the topmost dialog's enabled controls.
/// Existing host key bindings keep their behaviour within that boundary.
///
/// Clicks on the panel itself are swallowed; only clicks on the backdrop invoke
/// `on_dismiss`.
///
/// The panel is as tall as its content but never taller than the window, so a
/// `body` that can grow past that should put its own scroll area inside.
///
/// ```ignore
/// modal("connect", "New connection", px(420.), body, cx.listener(..))
/// ```
pub fn modal<E: IntoElement>(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    width: Pixels,
    body: E,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Modal {
        id: id.into(),
        title: title.into(),
        width,
        body: body.into_any_element(),
        on_dismiss: Box::new(on_dismiss),
    }
}

/// Backing element of [`modal`].
#[derive(IntoElement)]
struct Modal {
    id: ElementId,
    title: SharedString,
    width: Pixels,
    body: AnyElement,
    on_dismiss: DismissHandler,
}

impl RenderOnce for Modal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let on_dismiss = self.on_dismiss;
        let viewport_height = f32::from(window.viewport_size().height);
        let max_height = px((viewport_height - VIEWPORT_MARGIN).max(MIN_PANEL_HEIGHT));

        div()
            .id(self.id.clone())
            .absolute()
            .inset_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.overlay)
            .on_click(move |_, window, cx| on_dismiss(window, cx))
            .child(
                div()
                    .id(ElementId::from((self.id, "panel")))
                    .occlude()
                    .tab_group()
                    .focus_trap()
                    .flex()
                    .flex_col()
                    .w(self.width)
                    .max_h(max_height)
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .rounded_lg()
                    .shadow_lg()
                    .text_color(theme.text)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .px(px(16.))
                            .h(px(44.))
                            .border_b_1()
                            .border_color(theme.border)
                            .text_size(px(14.))
                            .child(self.title),
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
                            .min_h_0()
                            .gap(px(12.))
                            .p(px(16.))
                            .child(self.body),
                    ),
            )
    }
}

/// Builds a labelled form row: a fixed-width label followed by `control`.
///
/// Intended for the body of a [`modal`], but usable anywhere a label/control
/// pair is needed.
pub fn form_row<E: IntoElement>(label: impl Into<SharedString>, control: E) -> impl IntoElement {
    FormRow {
        label: label.into(),
        control: control.into_any_element(),
    }
}

/// Backing element of [`form_row`].
#[derive(IntoElement)]
struct FormRow {
    label: SharedString,
    control: AnyElement,
}

impl RenderOnce for FormRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_none()
                    .w(px(96.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(self.label),
            )
            .child(div().flex_grow_1().min_w_0().child(self.control))
    }
}

#[cfg(test)]
mod tests {
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
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let owner = cx.entity();
            div()
                .relative()
                .size_full()
                .track_focus(&self.root)
                .child(self.background.clone())
                .when(self.show_dialog, |root| {
                    root.child(modal(
                        "test-modal",
                        "Dialog",
                        px(400.),
                        div()
                            .child(self.first.clone())
                            .child(self.disabled.clone())
                            .child(self.second.clone())
                            .child(Button::new("disabled-button", "Disabled").disabled(true))
                            .child(Button::new("accept", "Accept").on_click(move |_, _, cx| {
                                owner.update(cx, |harness, _| harness.clicks += 1);
                            })),
                        |_, _| {},
                    ))
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
                assert!(
                    harness
                        .background
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
    }
}
