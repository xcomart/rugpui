//! The two palette pickers, the caption buttons, and the composite that shows
//! a whole `Theme` at once.

use gpui::{AnyView, App, Window, WindowButton, div, prelude::*, px};
use rugpui::{
    Button, ButtonVariant, Checkbox, EditorThemePicker, EditorThemeRegistry, EditorThemeSwatch,
    ProgressBar, SchemePreview, SchemeSelect, SchemeSwatch, Segmented, Select, Slider, Spinner,
    Switch, TabBar, TabItem, TabStatus, Theme, ThemeRegistry, WindowControls, theme,
};

use super::{Motion, Shot, bare, panel, row};
use crate::monospace;

/// Every shot on the two picker pages, the caption-button page and the theming
/// page.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "scheme-select/closed",
        width: 380.,
        height: 66.,
        per_theme: "",
        motion: Motion::Still,
        build: scheme_closed,
    },
    Shot {
        name: "scheme-select/open",
        width: 380.,
        height: 260.,
        per_theme: "",
        motion: Motion::Still,
        build: scheme_open,
    },
    Shot {
        name: "scheme-select/disabled",
        width: 380.,
        height: 66.,
        per_theme: "",
        motion: Motion::Still,
        build: scheme_disabled,
    },
    Shot {
        name: "editor-theme-picker/cards",
        width: 520.,
        height: 280.,
        per_theme: "",
        motion: Motion::Still,
        build: editor_cards,
    },
    Shot {
        name: "window-controls/strip",
        width: 300.,
        height: 60.,
        per_theme: "",
        motion: Motion::Still,
        build: window_controls,
    },
    Shot {
        name: "theme/sample",
        width: 560.,
        height: 284.,
        per_theme: "theme/%s",
        motion: Motion::Still,
        build: sample,
    },
];

// --- scheme select ----------------------------------------------------------

/// The id of the first built-in chrome theme, which every picker shot has
/// picked.
///
/// An *id*, not a name: a scheme is chosen by the string `settings.json` stores,
/// and a picker handed an id nothing answers to shows the id itself — which is
/// correct behaviour and a poor picture.
const FIRST_SCHEME: &str = "one-dark";

/// One entry per registered chrome theme, each carrying the colours that tell
/// it from the others.
fn scheme_swatches(cx: &App) -> Vec<SchemeSwatch> {
    ThemeRegistry::all(cx)
        .into_iter()
        .map(|entry| {
            let palette: Theme = ThemeRegistry::resolve(&entry.id, cx);
            SchemeSwatch::new(entry.id, entry.name).preview(SchemePreview {
                background: palette.background,
                foreground: palette.text,
                accents: vec![palette.accent, palette.success, palette.danger],
            })
        })
        .collect()
}

/// The trigger with a scheme picked and the list away.
fn scheme_closed(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        SchemeSelect::new("scheme")
            .options(scheme_swatches(cx))
            .selected(Some(FIRST_SCHEME))
            .width(px(320.))
            .into_any_element()
    })
}

/// The list showing: one row per scheme, each with its own pill.
fn scheme_open(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        SchemeSelect::new("scheme")
            .options(scheme_swatches(cx))
            .selected(Some(FIRST_SCHEME))
            .open(true)
            .width(px(320.))
            .on_open_change(|_open, _window, _cx| {})
            .into_any_element()
    })
}

/// Something else has already made the choice, and the answer is still worth
/// showing because it moves as that choice does.
fn scheme_disabled(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, cx| {
        SchemeSelect::new("scheme")
            .options(scheme_swatches(cx))
            .selected(Some(FIRST_SCHEME))
            .disabled(true)
            .width(px(320.))
            .into_any_element()
    })
}

// --- editor theme picker ----------------------------------------------------

/// The grid of cards, with the leading "follow the app theme" placeholder the
/// picker draws for an entry with no preview.
fn editor_cards(_window: &mut Window, cx: &mut App) -> AnyView {
    let mono = monospace(cx);
    panel(cx, move |_window, cx| {
        let follow = EditorThemeSwatch::new("", "Follow the app theme")
            .placeholder_label("follows the app theme");
        let options =
            std::iter::once(follow).chain(EditorThemeRegistry::all(cx).into_iter().take(3).map(
                |entry| {
                    let preview = EditorThemeRegistry::resolve(&entry.id, cx);
                    EditorThemeSwatch::new(entry.id, entry.name).preview(preview)
                },
            ));
        EditorThemePicker::new("editor-theme")
            .options(options)
            .selected(Some("one-dark"))
            .font_family(mono.clone())
            .columns(2)
            .into_any_element()
    })
}

// --- window controls --------------------------------------------------------

/// The three caption buttons of a window that draws its own title bar.
///
/// On the window's background rather than on a title bar's surface: the strip
/// paints its own surface and its own hairline, and a band of that same surface
/// behind it would hide the whole of what the widget draws.
fn window_controls(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, _cx| {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .h(px(36.))
                    .child(WindowControls::new(
                        "window-controls",
                        rugpui_shell::window_control_icons(),
                        vec![
                            WindowButton::Minimize,
                            WindowButton::Maximize,
                            WindowButton::Close,
                        ],
                    )),
            )
            .into_any_element()
    })
}

// --- the theme sample -------------------------------------------------------

/// A handful of widgets that between them wear most of a [`Theme`]'s slots, so
/// that one picture per palette says what the palette actually looks like.
///
/// Taken once for each of the six built-ins; the script drives that loop and
/// files the answers under `docs/screenshots/theme/<id>.png`.
fn sample(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, cx| {
        let palette = theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                TabBar::new("tabs")
                    .tabs(vec![
                        TabItem::new("t1", "warehouse").status(TabStatus::Connected),
                        TabItem::new("t2", "orders.sql").status(TabStatus::Connecting),
                        TabItem::new("t3", "staging").status(TabStatus::Error),
                    ])
                    .active(0)
                    .on_select(|_index, _window, _cx| {})
                    .on_close(|_index, _window, _cx| {}),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(super::PADDING))
                    .child(
                        row()
                            .child(Button::new("primary", "Connect"))
                            .child(
                                Button::new("secondary", "Cancel")
                                    .variant(ButtonVariant::Secondary),
                            )
                            .child(Button::new("ghost", "Reset").variant(ButtonVariant::Ghost))
                            .child(Button::new("danger", "Drop").variant(ButtonVariant::Danger))
                            .child(Button::new("off", "Connect").disabled(true)),
                    )
                    .child(
                        row()
                            .child(Checkbox::new("wrap", "Wrap long values").checked(true))
                            .child(Switch::new("auto", "Auto-reconnect").checked(true))
                            .child(Spinner::new("busy")),
                    )
                    .child(
                        row()
                            .child(
                                div().w(px(240.)).child(
                                    Segmented::new("format")
                                        .options(vec![
                                            ("csv", "CSV"),
                                            ("json", "JSON"),
                                            ("insert", "INSERT"),
                                        ])
                                        .selected(1),
                                ),
                            )
                            .child(
                                div().w(px(200.)).child(
                                    Select::new("driver")
                                        .options(["PostgreSQL", "MySQL", "SQLite"])
                                        .selected(Some("PostgreSQL"))
                                        .width(px(200.)),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(Slider::new("amount").value(0.4))
                            .child(ProgressBar::new("amount-progress").fraction(0.4)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(11.))
                            .child(div().text_color(palette.text).child("text"))
                            .child(div().text_color(palette.text_muted).child("text_muted"))
                            .child(div().text_color(palette.accent).child("accent"))
                            .child(div().text_color(palette.success).child("success"))
                            .child(div().text_color(palette.danger).child("danger"))
                            .child(
                                div()
                                    .px(px(6.))
                                    .rounded_md()
                                    .bg(palette.surface_active)
                                    .child("surface_active"),
                            ),
                    ),
            )
            .into_any_element()
    })
}
