//! Centered dialog rendered on top of a translucent backdrop.

use gpui::{AnyElement, App, ElementId, Pixels, SharedString, Window, div, prelude::*, px};

use crate::overlay;
use crate::theme::theme;

/// Callback fired when the backdrop is clicked.
type DismissHandler = Box<dyn Fn(&mut Window, &mut App)>;

/// Space kept clear above and below the panel, in pixels, top and bottom
/// combined, inside the client-side shadow band if the window has one.
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
        let max_height =
            overlay::max_height(window, px(VIEWPORT_MARGIN / 2.)).max(px(MIN_PANEL_HEIGHT));

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
                            .w_full()
                            .min_h_0()
                            .gap(px(12.))
                            .p(px(16.))
                            .child(self.body),
                    ),
            )
    }
}

/// Builds a labeled form row: a fixed-width label followed by `control`.
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
            // The containing form supplies the width. Avoid measuring the
            // control subtree again just to discover the row's intrinsic width.
            .w_full()
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
}
