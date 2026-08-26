//! The small widgets a form is built out of: one picture per option of
//! `Button`, `Checkbox`, `Switch`, `Collapsible`, `Segmented`, `Slider`,
//! `ProgressBar`, `Spinner` and `TextInput`.

use gpui::{AnyView, App, Entity, Focusable, Window, div, prelude::*, px};
use rugpui::{
    Button, ButtonVariant, Checkbox, Collapsible, ProgressBar, Segmented, Slider, Spinner, Switch,
    TextInput, theme,
};

use super::{Motion, Shot, caption, column, panel, row};
use crate::{TRIANGLE_DOWN, TRIANGLE_RIGHT};

/// Every shot on the nine pages above.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "button/variants",
        width: 420.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: variants,
    },
    Shot {
        name: "button/compact",
        width: 200.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: compact,
    },
    Shot {
        name: "button/full-width",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: full_width,
    },
    Shot {
        name: "checkbox/states",
        width: 280.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: checkboxes,
    },
    Shot {
        name: "switch/states",
        width: 380.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: switches,
    },
    Shot {
        name: "collapsible/closed",
        width: 260.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_closed,
    },
    Shot {
        name: "collapsible/open",
        width: 260.,
        height: 120.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_open,
    },
    Shot {
        name: "collapsible/trailing",
        width: 260.,
        height: 120.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_trailing,
    },
    Shot {
        name: "collapsible/arrow-icons",
        width: 260.,
        height: 120.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_arrows,
    },
    Shot {
        name: "collapsible/indent",
        width: 260.,
        height: 208.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_indent,
    },
    Shot {
        name: "collapsible/disabled",
        width: 260.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: collapsible_disabled,
    },
    Shot {
        name: "segmented/selected",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: segmented,
    },
    Shot {
        name: "slider/values",
        width: 320.,
        height: 180.,
        per_theme: "",
        motion: Motion::Still,
        build: sliders,
    },
    Shot {
        name: "progress/values",
        width: 320.,
        height: 152.,
        per_theme: "",
        motion: Motion::Still,
        build: progress_values,
    },
    Shot {
        name: "progress/indeterminate",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Sweep { period_ms: 1200 },
        build: progress_indeterminate,
    },
    Shot {
        name: "spinner/sizes",
        width: 180.,
        height: 72.,
        per_theme: "",
        motion: Motion::Spin { period_ms: 800 },
        build: spinner_sizes,
    },
    Shot {
        name: "spinner/color",
        width: 180.,
        height: 72.,
        per_theme: "",
        motion: Motion::Spin { period_ms: 800 },
        build: spinner_color,
    },
    Shot {
        name: "text-input/placeholder",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_placeholder,
    },
    Shot {
        name: "text-input/value",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_value,
    },
    Shot {
        name: "text-input/focused",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_focused,
    },
    Shot {
        name: "text-input/masked",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_masked,
    },
    Shot {
        name: "text-input/multiline",
        width: 320.,
        height: 106.,
        per_theme: "",
        motion: Motion::Still,
        build: input_multiline,
    },
    Shot {
        name: "text-input/disabled",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_disabled,
    },
    Shot {
        name: "text-input/invalid",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: input_invalid,
    },
];

// --- button -----------------------------------------------------------------

/// The four weights side by side, and the same button disabled.
fn variants(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(Button::new("primary", "Connect"))
            .child(Button::new("secondary", "Cancel").variant(ButtonVariant::Secondary))
            .child(Button::new("ghost", "Reset").variant(ButtonVariant::Ghost))
            .child(Button::new("danger", "Drop").variant(ButtonVariant::Danger))
            .child(Button::new("disabled", "Connect").disabled(true))
            .into_any_element()
    })
}

/// The default height beside `compact()`, which is the only way to see it.
fn compact(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(Button::new("default", "Run"))
            .child(Button::new("compact", "Run").compact())
            .into_any_element()
    })
}

/// One button stretched across its parent.
fn full_width(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        div()
            .w_full()
            .child(Button::new("full", "Connect").full_width(true))
            .into_any_element()
    })
}

// --- checkbox and switch ----------------------------------------------------

/// Both states of the box, which are the only two it has.
fn checkboxes(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(Checkbox::new("off", "Show nulls"))
            .child(Checkbox::new("on", "Wrap long values").checked(true))
            .into_any_element()
    })
}

/// Both states of the switch. It has no disabled form; see the page.
fn switches(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(Switch::new("off", "Send telemetry"))
            .child(Switch::new("on", "Auto-reconnect").checked(true))
            .into_any_element()
    })
}

// --- collapsible ------------------------------------------------------------

/// Folded away — and the body is not rendered at all, which is the point.
fn collapsible_closed(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Collapsible::new("advanced", "Advanced options")
            .child(Checkbox::new("nulls", "Show nulls"))
            .into_any_element()
    })
}

/// The same section open.
fn collapsible_open(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Collapsible::new("advanced", "Advanced options")
            .open(true)
            .child(Checkbox::new("nulls", "Show nulls"))
            .child(Checkbox::new("locked", "Read only").checked(true))
            .into_any_element()
    })
}

/// A switch at the far end of the header: arming the block and folding it away
/// are two gestures, so the switch sits beside the disclosure rather than in it.
fn collapsible_trailing(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Collapsible::new("advanced", "Advanced options")
            .open(true)
            .trailing(Switch::new("advanced-on", "").checked(true))
            .child(Checkbox::new("nulls", "Show nulls"))
            .child(Checkbox::new("locked", "Read only").checked(true))
            .into_any_element()
    })
}

/// The default caret pair replaced with the host's own svg paths.
fn collapsible_arrows(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Collapsible::new("advanced", "Advanced options")
            .open(true)
            .arrow_icons(TRIANGLE_RIGHT, TRIANGLE_DOWN)
            .child(Checkbox::new("nulls", "Show nulls"))
            .child(Checkbox::new("locked", "Read only").checked(true))
            .into_any_element()
    })
}

/// `indent(true)`, the default, above `indent(false)`.
fn collapsible_indent(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        column()
            .child(caption("indent(true) — the default", cx))
            .child(
                Collapsible::new("indented", "Advanced options")
                    .open(true)
                    .child(Checkbox::new("indented-nulls", "Show nulls")),
            )
            .child(caption("indent(false)", cx))
            .child(
                Collapsible::new("flush", "Advanced options")
                    .open(true)
                    .indent(false)
                    .child(Checkbox::new("flush-nulls", "Show nulls")),
            )
            .into_any_element()
    })
}

/// A header that is drawn and does not answer.
fn collapsible_disabled(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Collapsible::new("advanced", "Advanced options")
            .disabled(true)
            .child(Checkbox::new("nulls", "Show nulls"))
            .into_any_element()
    })
}

// --- segmented --------------------------------------------------------------

/// Three segments with the middle one picked.
fn segmented(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Segmented::new("format")
            .options(vec![("csv", "CSV"), ("json", "JSON"), ("insert", "INSERT")])
            .selected(1)
            .into_any_element()
    })
}

// --- slider and progress ----------------------------------------------------

/// Empty, part way and full, since a slider's whole state is where the knob is.
fn sliders(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        column()
            .child(caption("value(0.0)", cx))
            .child(Slider::new("empty").value(0.))
            .child(caption("value(0.4)", cx))
            .child(Slider::new("part").value(0.4))
            .child(caption("value(1.0)", cx))
            .child(Slider::new("full").value(1.))
            .into_any_element()
    })
}

/// The same three amounts on the bar.
fn progress_values(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        column()
            .child(caption("fraction(0.0)", cx))
            .child(ProgressBar::new("empty").fraction(0.))
            .child(caption("fraction(0.35)", cx))
            .child(ProgressBar::new("part").fraction(0.35))
            .child(caption("fraction(1.0)", cx))
            .child(ProgressBar::new("full").fraction(1.))
            .into_any_element()
    })
}

/// The sweeping segment, caught mid-sweep — it is an animation, so the file is
/// one frame of it.
fn progress_indeterminate(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        div()
            .w_full()
            .child(ProgressBar::new("loading").indeterminate())
            .into_any_element()
    })
}

// --- spinner ----------------------------------------------------------------

/// The default 16 px beside a smaller and two larger ones.
fn spinner_sizes(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(Spinner::new("s12").size(px(12.)))
            .child(Spinner::new("s16"))
            .child(Spinner::new("s24").size(px(24.)))
            .child(Spinner::new("s32").size(px(32.)))
            .into_any_element()
    })
}

/// The theme's accent, which is the default, beside two colours of the host's.
fn spinner_color(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        let palette = theme(cx);
        row()
            .child(Spinner::new("accent").size(px(24.)))
            .child(Spinner::new("success").size(px(24.)).color(palette.success))
            .child(Spinner::new("danger").size(px(24.)).color(palette.danger))
            .into_any_element()
    })
}

// --- text input -------------------------------------------------------------

/// A field with `content`, built once and cloned into the body.
fn field(cx: &mut App, build: impl FnOnce(&mut gpui::Context<TextInput>) -> TextInput) -> AnyView {
    let input = cx.new(build);
    panel(cx, move |_window, _cx| {
        div().w_full().child(input.clone()).into_any_element()
    })
}

/// Empty, showing the muted placeholder.
fn input_placeholder(_window: &mut Window, cx: &mut App) -> AnyView {
    field(cx, |cx| TextInput::new(cx).placeholder("Search tables…"))
}

/// The same field with something in it.
fn input_value(_window: &mut Window, cx: &mut App) -> AnyView {
    field(cx, |cx| {
        let mut input = TextInput::new(cx).placeholder("host:port");
        input.set_content("db.internal:5432", cx);
        input
    })
}

/// Focused, which is the accent border and the caret — the one state a host can
/// put a field into without a pointer.
fn input_focused(_window: &mut Window, cx: &mut App) -> AnyView {
    let input: Entity<TextInput> = cx.new(|cx| {
        let mut input = TextInput::new(cx).placeholder("host:port");
        input.set_content("db.internal:5432", cx);
        input
    });
    panel(cx, move |window, cx| {
        // Every frame rather than once: the window takes the focus as it opens,
        // and asking again costs nothing once the field already has it.
        let handle = input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        div().w_full().child(input.clone()).into_any_element()
    })
}

/// Every grapheme drawn as a bullet.
fn input_masked(_window: &mut Window, cx: &mut App) -> AnyView {
    field(cx, |cx| {
        let mut input = TextInput::new(cx).masked(true).placeholder("Password");
        input.set_content("hunter2", cx);
        input
    })
}

/// Three rows tall, and `Enter` breaks the line instead of submitting.
fn input_multiline(_window: &mut Window, cx: &mut App) -> AnyView {
    field(cx, |cx| {
        let mut input = TextInput::new(cx).multiline(3).placeholder("Notes");
        input.set_content(
            "customs paperwork attached\nsplit shipment\ncall before delivery",
            cx,
        );
        input
    })
}

/// Read-only, muted, with no menu and no actions wired at all.
fn input_disabled(_window: &mut Window, cx: &mut App) -> AnyView {
    field(cx, |cx| {
        let mut input = TextInput::new(cx).disabled(true);
        input.set_content("read-only", cx);
        input
    })
}

/// Outlined in `danger`, which is `set_invalid` rather than a builder option.
fn input_invalid(_window: &mut Window, cx: &mut App) -> AnyView {
    let input: Entity<TextInput> = cx.new(|cx| {
        let mut input = TextInput::new(cx).placeholder("host:port");
        input.set_content("db.internal:port", cx);
        input.set_invalid(true, cx);
        input
    });
    panel(cx, move |_window, _cx| {
        div().w_full().child(input.clone()).into_any_element()
    })
}
