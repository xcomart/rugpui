//! The widgets that put something over the rest of the window, and the two
//! that divide it: `Select`, `MenuButton`/`ContextMenu`, `TabBar`, the three
//! tooltips, `modal`, `Scrollbar` and `Splitter`.

use gpui::{AnyElement, AnyView, App, Axis, ScrollHandle, Window, div, point, prelude::*, px};
use rugpui::{
    Button, ButtonVariant, Checkbox, ContextMenu, MenuButton, MenuEntry, Scrollbar, ScrollbarAxis,
    Select, SelectOption, Splitter, TabBar, TabItem, TabStatus, TextInput, Tooltip, form_row,
    modal, theme, tooltip_label,
};
use rugpui_editor::{highlighter_for_extension, tooltip_code};

use super::{Motion, Shot, bare, caption, framed, panel, row};
use crate::{DATABASE, TRIANGLE_DOWN, WARNING, data, monospace};

/// Every shot on the seven pages above.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "select/closed",
        width: 260.,
        height: 66.,
        per_theme: "",
        motion: Motion::Still,
        build: select_closed,
    },
    Shot {
        name: "select/placeholder",
        width: 260.,
        height: 66.,
        per_theme: "",
        motion: Motion::Still,
        build: select_placeholder,
    },
    Shot {
        name: "select/open",
        width: 260.,
        height: 212.,
        per_theme: "",
        motion: Motion::Still,
        build: select_open,
    },
    Shot {
        name: "select/icons",
        width: 260.,
        height: 212.,
        per_theme: "",
        motion: Motion::Still,
        build: select_icons,
    },
    Shot {
        name: "select/chevron-icon",
        width: 260.,
        height: 66.,
        per_theme: "",
        motion: Motion::Still,
        build: select_chevron,
    },
    Shot {
        name: "menu/button",
        width: 130.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: menu_button,
    },
    Shot {
        name: "menu/open",
        width: 330.,
        height: 216.,
        per_theme: "",
        motion: Motion::Still,
        build: menu_open,
    },
    Shot {
        name: "menu/context",
        width: 330.,
        height: 176.,
        per_theme: "",
        motion: Motion::Still,
        build: menu_context,
    },
    Shot {
        name: "tab-bar/tabs",
        width: 620.,
        height: 60.,
        per_theme: "",
        motion: Motion::Still,
        build: tab_bar,
    },
    Shot {
        name: "tab-bar/menu-open",
        width: 620.,
        height: 176.,
        per_theme: "",
        motion: Motion::Still,
        build: tab_bar_menu,
    },
    Shot {
        name: "tooltip/label",
        width: 320.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: tooltip_plain,
    },
    Shot {
        name: "tooltip/rich",
        width: 420.,
        height: 282.,
        per_theme: "",
        motion: Motion::Still,
        build: tooltip_rich,
    },
    Shot {
        name: "tooltip/code",
        width: 440.,
        height: 152.,
        per_theme: "",
        motion: Motion::Still,
        build: tooltip_snippet,
    },
    Shot {
        name: "modal/dialog",
        width: 560.,
        height: 340.,
        per_theme: "",
        motion: Motion::Still,
        build: modal_dialog,
    },
    Shot {
        name: "scrollbar/vertical",
        width: 260.,
        height: 200.,
        per_theme: "",
        motion: Motion::Still,
        build: scrollbar_vertical,
    },
    Shot {
        name: "scrollbar/horizontal",
        width: 320.,
        height: 100.,
        per_theme: "",
        motion: Motion::Still,
        build: scrollbar_horizontal,
    },
    Shot {
        name: "splitter/horizontal",
        width: 480.,
        height: 180.,
        per_theme: "",
        motion: Motion::Still,
        build: splitter_horizontal,
    },
    Shot {
        name: "splitter/vertical",
        width: 480.,
        height: 220.,
        per_theme: "",
        motion: Motion::Still,
        build: splitter_vertical,
    },
    Shot {
        name: "splitter/seamless",
        width: 480.,
        height: 190.,
        per_theme: "",
        motion: Motion::Still,
        build: splitter_seamless,
    },
];

/// The five drivers every `Select` shot offers.
const DRIVERS: [&str; 5] = ["PostgreSQL", "MySQL", "Oracle", "SQLite", "SQL Server"];

// --- select -----------------------------------------------------------------

/// A trigger with something picked and the list away.
fn select_closed(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Select::new("driver")
            .options(DRIVERS)
            .selected(Some("PostgreSQL"))
            .width(px(200.))
            .into_any_element()
    })
}

/// Nothing picked, so the trigger shows the muted placeholder instead.
fn select_placeholder(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Select::new("driver")
            .options(DRIVERS)
            .placeholder("Pick a driver")
            .width(px(200.))
            .into_any_element()
    })
}

/// The list showing, which is a flag the host holds and not a state the widget
/// keeps.
fn select_open(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Select::new("driver")
            .options(DRIVERS)
            .selected(Some("PostgreSQL"))
            .open(true)
            .width(px(200.))
            .into_any_element()
    })
}

/// The same list with an icon in each row's leading slot, and one row wearing a
/// trailing mark as well — a list may mark only some of its rows.
fn select_icons(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Select::new("driver")
            .options(DRIVERS.into_iter().enumerate().map(|(index, name)| {
                let option = SelectOption::new(name).leading(DATABASE);
                match index {
                    0 => option.trailing(WARNING),
                    _ => option,
                }
            }))
            .selected(Some("PostgreSQL"))
            .open(true)
            .width(px(200.))
            .into_any_element()
    })
}

/// The default caret replaced with an svg path of the host's own.
fn select_chevron(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        Select::new("driver")
            .options(DRIVERS)
            .selected(Some("PostgreSQL"))
            .chevron_icon(TRIANGLE_DOWN)
            .width(px(200.))
            .into_any_element()
    })
}

// --- menus ------------------------------------------------------------------

/// The rows every menu shot shows: two commands, a rule, a checked row and a
/// row that is there and is not this moment's.
fn menu_entries() -> Vec<MenuEntry> {
    vec![
        MenuEntry::new("New tab").shortcut("Ctrl+T"),
        MenuEntry::new("Run statement").shortcut("Ctrl+Enter"),
        MenuEntry::separator(),
        MenuEntry::new("Wrap long values").checked(true),
        MenuEntry::new("Export…").disabled(true),
    ]
}

/// The 28 px trigger on its own.
fn menu_button(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        row()
            .child(MenuButton::new("app-menu").entries(menu_entries()))
            .child(
                MenuButton::new("db-menu")
                    .icon(DATABASE)
                    .entries(menu_entries()),
            )
            .into_any_element()
    })
}

/// The same trigger with its panel down.
fn menu_open(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |_window, _cx| {
        MenuButton::new("app-menu")
            .open(true)
            .entries(menu_entries())
            // Setting the handler is what makes the trigger answer at all; the
            // shot needs it only so the panel is drawn the way a live one is.
            .on_open_change(|_open, _window, _cx| {})
            .into_any_element()
    })
}

/// The same rows with no trigger at all, placed where a right click landed.
fn menu_context(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, _cx| {
        ContextMenu::new("context")
            .position(point(px(24.), px(24.)))
            .entries(menu_entries())
            .into_any_element()
    })
}

// --- tab bar ----------------------------------------------------------------

/// The four tabs every tab-bar shot shows: one per status, and a mark on the
/// second.
fn tabs() -> Vec<TabItem> {
    vec![
        TabItem::new("t1", "warehouse").status(TabStatus::Connected),
        TabItem::new("t2", "orders.sql")
            .status(TabStatus::Connecting)
            .mark(WARNING, "One statement did not parse"),
        TabItem::new("t3", "report.json").status(TabStatus::Disconnected),
        TabItem::new("t4", "staging").status(TabStatus::Error),
    ]
}

/// A strip with all four statuses, a mark, close buttons and a "+".
///
/// The last three are opt-in by handler rather than by a flag, which is why the
/// shot sets three handlers that do nothing.
fn tab_bar(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, _cx| {
        TabBar::new("tabs")
            .tabs(tabs())
            .active(1)
            .tooltips("All tabs", "New tab", "Close")
            .on_select(|_index, _window, _cx| {})
            .on_close(|_index, _window, _cx| {})
            .on_new(|_window, _cx| {})
            .on_menu_open_change(|_open, _window, _cx| {})
            .into_any_element()
    })
}

/// The dropdown that lists every tab, down.
fn tab_bar_menu(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, _cx| {
        TabBar::new("tabs")
            .tabs(tabs())
            .active(1)
            .menu_open(true)
            .tooltips("All tabs", "New tab", "Close")
            .on_select(|_index, _window, _cx| {})
            .on_close(|_index, _window, _cx| {})
            .on_new(|_window, _cx| {})
            .on_menu_open_change(|_open, _window, _cx| {})
            .into_any_element()
    })
}

// --- tooltips ---------------------------------------------------------------

/// One line of text in the standard box.
///
/// The box is rendered straight into the window rather than hovered into view:
/// a screenshot cannot hold a pointer still, and what the page has to show is
/// the box and not the gesture.
fn tooltip_plain(_window: &mut Window, cx: &mut App) -> AnyView {
    panel(cx, |window, cx| {
        // A flex *row*, so the box keeps its natural width: the panel is a
        // column, and a column stretches its children across itself.
        div()
            .flex()
            .flex_row()
            .child(tooltip_label("Copy the selected cells")(window, cx))
            .into_any_element()
    })
}

/// A thumbnail, a caption and a snippet in one column.
fn tooltip_rich(_window: &mut Window, cx: &mut App) -> AnyView {
    let mono = monospace(cx);
    panel(cx, move |window, cx| {
        let sql = highlighter_for_extension("sql").expect("sql ships with rugpui-editor");
        let mono = mono.clone();
        let build = Tooltip::new()
            .image(crate::PREVIEW, px(96.))
            .note("public.orders — 12 rows")
            .element(move |_window, _cx| {
                rugpui_editor::CodeSnippet::new(data::SQL, sql.clone())
                    .font_family(mono.clone())
                    .max_lines(4)
                    .into_any_element()
            })
            .build();
        div()
            .flex()
            .flex_row()
            .child(build(window, cx))
            .into_any_element()
    })
}

/// Nothing but code, in the editor's own palette.
fn tooltip_snippet(_window: &mut Window, cx: &mut App) -> AnyView {
    let mono = monospace(cx);
    panel(cx, move |window, cx| {
        let sql = highlighter_for_extension("sql").expect("sql ships with rugpui-editor");
        let build = tooltip_code(SNIPPET, sql, Some(mono.clone()));
        div()
            .flex()
            .flex_row()
            .child(build(window, cx))
            .into_any_element()
    })
}

/// What the code tooltip shows.
///
/// Its own listing rather than the gallery's: a tooltip is a thing that appears
/// beside a pointer, and thirteen lines of it would be a second window.
const SNIPPET: &str = "\
SELECT o.order_id, c.name
  FROM public.orders AS o
 WHERE o.shipped_at IS NULL
 LIMIT 100;";

// --- modal ------------------------------------------------------------------

/// A dialog over its backdrop: three form rows and the two buttons that end it.
fn modal_dialog(_window: &mut Window, cx: &mut App) -> AnyView {
    let host = cx.new(|cx| {
        let mut input = TextInput::new(cx).placeholder("host:port");
        input.set_content("db.internal:5432", cx);
        input
    });
    let user = cx.new(|cx| {
        let mut input = TextInput::new(cx);
        input.set_content("reporting", cx);
        input
    });
    let password = cx.new(|cx| {
        let mut input = TextInput::new(cx).masked(true);
        input.set_content("hunter2", cx);
        input
    });

    bare(cx, move |_window, _cx| {
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(form_row("Host", host.clone()))
            .child(form_row("User", user.clone()))
            .child(form_row("Password", password.clone()))
            .child(form_row(
                "",
                Checkbox::new("save", "Remember me").checked(true),
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(Button::new("cancel", "Cancel").variant(ButtonVariant::Secondary))
                    .child(Button::new("connect", "Connect")),
            );
        modal(
            "connect",
            "New connection",
            px(420.),
            body,
            |_window, _cx| {},
        )
        .into_any_element()
    })
}

// --- scrollbar --------------------------------------------------------------

/// A list taller than its box, with the overlay bar down the right of it.
fn scrollbar_vertical(_window: &mut Window, cx: &mut App) -> AnyView {
    let handle = ScrollHandle::new();
    panel(cx, move |_window, cx| {
        let palette = theme(cx);
        let bar = Scrollbar::for_handle("vertical", ScrollbarAxis::Vertical, &handle);
        framed(cx)
            .relative()
            .size_full()
            .child(
                div()
                    .id("list")
                    .track_scroll(&handle)
                    .size_full()
                    .overflow_y_scroll()
                    .py(px(4.))
                    .children(data::COLUMN_NAMES.iter().map(|name| {
                        div()
                            .px(px(10.))
                            .py(px(3.))
                            .text_color(palette.text_muted)
                            .child(*name)
                    })),
            )
            .children(bar.render(&palette))
            .into_any_element()
    })
}

/// The same bar turned on its side, under a row wider than its box.
fn scrollbar_horizontal(_window: &mut Window, cx: &mut App) -> AnyView {
    let handle = ScrollHandle::new();
    panel(cx, move |_window, cx| {
        let palette = theme(cx);
        let bar = Scrollbar::for_handle("horizontal", ScrollbarAxis::Horizontal, &handle);
        framed(cx)
            .relative()
            .size_full()
            .child(
                div()
                    .id("strip")
                    .track_scroll(&handle)
                    .size_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .overflow_x_scroll()
                    .px(px(10.))
                    .children(data::COLUMN_NAMES.iter().map(|name| {
                        div()
                            .flex_none()
                            .text_color(palette.text_muted)
                            .child(*name)
                    })),
            )
            .children(bar.render(&palette))
            .into_any_element()
    })
}

// --- splitter ---------------------------------------------------------------

/// One half of a split, filled so the divider has something to divide.
fn half(label: &'static str, cx: &App) -> AnyElement {
    let palette = theme(cx);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(palette.surface)
        .text_color(palette.text_muted)
        .child(label)
        .into_any_element()
}

/// Side by side, with the divider moved off the middle.
fn splitter_horizontal(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, cx| {
        div()
            .size_full()
            .p(px(super::PADDING))
            .child(
                Splitter::new("split", Axis::Horizontal)
                    .ratio(0.35)
                    .first(half("first", cx))
                    .second(half("second", cx)),
            )
            .into_any_element()
    })
}

/// One above the other.
fn splitter_vertical(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, cx| {
        div()
            .size_full()
            .p(px(super::PADDING))
            .child(
                Splitter::new("split", Axis::Vertical)
                    .ratio(0.4)
                    .first(half("first", cx))
                    .second(half("second", cx)),
            )
            .into_any_element()
    })
}

/// The seam's hairline dropped, so the band is invisible until the pointer
/// finds it — beside the default, which draws the line.
fn splitter_seamless(_window: &mut Window, cx: &mut App) -> AnyView {
    bare(cx, |_window, cx| {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(super::PADDING))
            .child(caption("the default — the seam is drawn", cx))
            .child(
                div().h(px(56.)).child(
                    Splitter::new("drawn", Axis::Horizontal)
                        .first(half("first", cx))
                        .second(half("second", cx)),
                ),
            )
            .child(caption("seamless()", cx))
            .child(
                div().h(px(56.)).child(
                    Splitter::new("seamless", Axis::Horizontal)
                        .seamless()
                        .first(half("first", cx))
                        .second(half("second", cx)),
                ),
            )
            .into_any_element()
    })
}
