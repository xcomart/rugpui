//! The layer above the widget kit: the self-drawn title bar, the two dialogs,
//! the theme editor, the catalogue management row and the settings pieces.
//!
//! Everything here needs two things injected before it draws anything at all —
//! an [`AppIdentity`] and a table of words — because `rugpui-shell` is written
//! against an application it deliberately knows nothing about. The gallery is
//! not that application, so [`init`] hands it a stand-in: an identity naming
//! this crate, and a [`Words`] table that answers a key with the English the
//! key is named after. That is what a screenshot needs and it is *not* what a
//! host should copy — a real application looks these up in its own locale
//! files, which is the whole point of the trait.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyView, App, SharedString, Window, div, prelude::*, px};
use rugpui::{MenuButton, TextInput, Theme, ThemeDirs, ThemeFile, WindowControls, theme};
use rugpui_shell::{
    AboutDialog, AppIdentity, CatalogActions, CatalogFile, Strings, ThemeEditor, UiThemeCatalog,
    UpdateDialog, form,
    menu_rows::{self, MenuRow},
    update::Release,
};

use super::{Motion, Shot, bare, panel};

/// Every shot on the shell page.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "shell/title-bar",
        width: 620.,
        height: 56.,
        per_theme: "",
        motion: Motion::Still,
        build: title_bar,
    },
    Shot {
        name: "shell/menu-rows",
        width: 340.,
        height: 240.,
        per_theme: "",
        motion: Motion::Still,
        build: menu,
    },
    Shot {
        name: "shell/about",
        width: 560.,
        height: 400.,
        per_theme: "",
        motion: Motion::Still,
        build: about,
    },
    Shot {
        name: "shell/update",
        width: 560.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: update,
    },
    Shot {
        name: "shell/theme-editor",
        width: 660.,
        height: 608.,
        per_theme: "",
        motion: Motion::Still,
        build: theme_editor,
    },
    Shot {
        name: "shell/catalog-actions",
        width: 480.,
        height: 62.,
        per_theme: "",
        motion: Motion::Still,
        build: catalog_actions,
    },
    Shot {
        name: "shell/settings-form",
        width: 480.,
        height: 210.,
        per_theme: "",
        motion: Motion::Still,
        build: settings_form,
    },
];

// --- the stand-in host ------------------------------------------------------

/// The identity the shell reads its name, version and links out of.
const IDENTITY: AppIdentity = AppIdentity {
    name: "rugpui",
    version: env!("CARGO_PKG_VERSION"),
    repository_url: "https://github.com/xcomart/rugpui",
    repository_label: "github.com/xcomart/rugpui",
    latest_release_api: "https://api.github.com/repos/xcomart/rugpui/releases/latest",
    releases_page: "https://github.com/xcomart/rugpui/releases",
    fallback_archive: "rugpui.tar.gz",
    payload: &["rugpui-gallery"],
    bundle_executable: "Contents/MacOS/rugpui",
    windows_arp_key: "",
    must_defer: || false,
};

/// A table that answers a key with the English it is named after.
///
/// A real host looks the key up in its locale files. This one turns
/// `settings.manage.duplicate` into "Duplicate" and
/// `settings.editor.slot.text_muted` into "Text muted", which is enough for a
/// picture and is never enough for an application: nothing here is translated,
/// and half of it reads like a variable name because it *is* one.
struct Words;

/// The handful of keys the derivation above gets wrong, or that carry a marker
/// the shell fills in.
const PHRASES: &[(&str, &str)] = &[
    ("about.title", "About"),
    ("about.version", "Version %{version}"),
    (
        "about.tagline",
        "A gpui widget kit, and the two larger widgets built on it.",
    ),
    ("about.license", "Licensed under %{license}."),
    ("about.credits", "Built on gpui, from Zed."),
    ("update.title", "Update"),
    (
        "update.available",
        "%{app} %{version} is out. Update now, or carry on and be told again next time.",
    ),
    ("update.checking", "Looking for a newer release…"),
    ("update.up_to_date", "You are on the latest release."),
    ("update.downloading", "Downloading…"),
    ("update.installing", "Installing…"),
    ("update.installed", "You are running %{app} %{version}."),
    ("update.failed", "The update failed."),
    ("update.ignore", "Skip this version"),
    ("update.update", "Update"),
    ("update.open_release", "Open the release page"),
    ("menu.check_updates", "Check for updates"),
    ("settings.editor.theme_title", "Theme"),
    ("settings.editor.editor_theme_title", "Editor theme"),
    ("settings.editor.dark", "Dark palette"),
    ("settings.editor.automatic", "Automatic"),
    ("settings.editor.automatic_slot", "Back to automatic"),
    ("settings.editor.invalid", "Not a colour"),
    ("settings.editor.grid_group", "Grid"),
    ("settings.manage.copy_name", "%{name} copy"),
    ("settings.manage.delete_theme_confirm", "Delete %{name}?"),
    (
        "settings.manage.delete_editor_theme_confirm",
        "Delete %{name}?",
    ),
];

impl Strings for Words {
    fn text(&self, key: &str) -> SharedString {
        if let Some((_, phrase)) = PHRASES.iter().find(|(name, _)| *name == key) {
            return SharedString::new_static(phrase);
        }
        let tail = key.rsplit('.').next().unwrap_or(key).replace('_', " ");
        let mut chars = tail.chars();
        let sentence = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => tail,
        };
        SharedString::from(sentence)
    }
}

/// Installs the identity and the words, once per process.
///
/// Every shell shot calls it, because a shot is opened on its own and there is
/// no start-up path here that would have run first.
fn init(cx: &mut App) {
    rugpui_shell::init(IDENTITY, cx);
    rugpui_shell::set_strings(Box::new(Words), cx);
}

/// A catalogue over no directory at all.
///
/// Nothing in these shots saves, imports or deletes, and every one of those
/// refuses politely when the host named no directory — which is exactly the
/// state a user who has added no palette of their own is in.
fn catalog() -> Arc<UiThemeCatalog> {
    Arc::new(UiThemeCatalog::new(
        ThemeDirs {
            ui_themes: PathBuf::new(),
            editor_themes: None,
        },
        "dark",
    ))
}

// --- the title bar ----------------------------------------------------------

/// How tall the band a self-drawn caption is drawn in stands.
///
/// One number for the row and for the caption strip inside it: `WindowControls`
/// is `h_full`, so the band's height *is* the strip's, and the hairline each of
/// them draws lands in the same row only while that is true.
const TITLEBAR_HEIGHT: f32 = 38.;

/// A window that draws its own caption: a menu trigger, the title, and the
/// three caption buttons at the far end.
///
/// Assembled by hand rather than through `window_control_strips`, which answers
/// `None` unless the window really did take its decorations over — a screenshot
/// window has not, and the picture is about what the pieces look like together.
fn title_bar(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    bare(cx, |_window, cx| {
        let palette = theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("titlebar")
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .h(px(TITLEBAR_HEIGHT))
                    .pl(px(8.))
                    .bg(palette.surface)
                    // The strip is `h_full` and paints a hairline along the
                    // bottom of its own box, so the band's hairline has to sit
                    // in that same row or the two read as a two-pixel step
                    // under the caption buttons. An absolutely positioned rule
                    // at `bottom_0` is exactly that row, which is why the band
                    // does not simply carry a `border_b_1`: a border is laid
                    // out *inside* the box and would push the strip a pixel up.
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(1.))
                            .bg(palette.border),
                    )
                    .child(MenuButton::new("app-menu").entries(menu_rows::entries(rows())))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(palette.text_muted)
                            .child("warehouse — orders.sql"),
                    )
                    .child(WindowControls::new(
                        "window-controls",
                        rugpui_shell::window_control_icons(),
                        vec![
                            gpui::WindowButton::Minimize,
                            gpui::WindowButton::Maximize,
                            gpui::WindowButton::Close,
                        ],
                    )),
            )
            .into_any_element()
    })
}

// --- menus as rows ----------------------------------------------------------

/// The rows both menu shots draw, in the shape `menu_rows` takes.
fn rows() -> Vec<MenuRow> {
    vec![
        MenuRow::new("New tab").shortcut(format!("{}+T", menu_rows::SHORTCUT_MODIFIER)),
        MenuRow::new("Run statement").shortcut(format!("{}+Enter", menu_rows::SHORTCUT_MODIFIER)),
        MenuRow::separator(),
        MenuRow::new("Wrap long values").checked(true),
        MenuRow::new("Export…").enabled(false),
        MenuRow::separator(),
        MenuRow::new("Check for updates"),
    ]
}

/// One list of rows, drawn by `MenuButton` — which is what `menu_rows::entries`
/// converts them for.
fn menu(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    panel(cx, |_window, _cx| {
        MenuButton::new("app-menu")
            .open(true)
            .entries(menu_rows::entries(rows()))
            .on_open_change(|_open, _window, _cx| {})
            .into_any_element()
    })
}

// --- the two dialogs --------------------------------------------------------

/// The about card: the wordmark, the version, one line of prose, the repository
/// button and the footnotes.
fn about(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    let dialog = cx.new(AboutDialog::new);
    dialog.update(cx, |dialog, cx| dialog.open(cx));
    bare(cx, move |_window, _cx| {
        div().size_full().child(dialog.clone()).into_any_element()
    })
}

/// The update dialog in its announcing state, which is where the start-up
/// check leaves it.
fn update(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    let dialog = cx.new(UpdateDialog::new);
    dialog.update(cx, |dialog, cx| {
        dialog.open(
            Release {
                tag: "v0.3.0".to_owned(),
                version: "0.3.0".to_owned(),
                url: "https://github.com/xcomart/rugpui/releases/tag/v0.3.0".to_owned(),
                asset: None,
            },
            cx,
        );
    });
    bare(cx, move |_window, _cx| {
        div().size_full().child(dialog.clone()).into_any_element()
    })
}

// --- the catalogue ----------------------------------------------------------

/// One palette edited colour by colour, with the live preview above the name.
///
/// The window is the editor's own full height and not a pixel more: the field
/// list caps itself at `BODY_MAX_HEIGHT` and scrolls past that — the cap is
/// what keeps a settings modal the same size whichever body it is showing — so
/// a taller window buys empty background rather than more slots.
fn theme_editor(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    let file = CatalogFile::UiTheme(Box::new(ThemeFile::from_theme("Midnight", &Theme::dark())));
    let editor = cx.new(|cx| ThemeEditor::new(catalog(), "midnight", &file, cx));
    bare(cx, move |_window, cx| {
        let palette = theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(super::PADDING))
            .bg(palette.background)
            .child(editor.clone())
            .into_any_element()
    })
}

/// Duplicate / edit / delete / import / export, under the picker they act on.
fn catalog_actions(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    let actions = cx.new(|_cx| CatalogActions::new(catalog(), 100));
    actions.update(cx, |actions, cx| actions.set_selection("one-dark", cx));
    panel(cx, move |_window, _cx| {
        div().w_full().child(actions.clone()).into_any_element()
    })
}

// --- the settings pieces ----------------------------------------------------

/// The three pieces every settings form was being written out of identically: a
/// titled card, a muted paragraph, and a unit hint beside a narrow control.
fn settings_form(_window: &mut Window, cx: &mut App) -> AnyView {
    init(cx);
    let size = cx.new(|cx| {
        let mut input = TextInput::new(cx);
        input.set_content("13", cx);
        input
    });
    let family = cx.new(|cx| {
        let mut input = TextInput::new(cx).placeholder("the platform's default");
        input.set_content("DejaVu Sans Mono", cx);
        input
    });

    panel(cx, move |_window, cx| {
        let field_row = || div().flex().flex_row().items_center().gap(px(10.));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                field_row()
                    .child(div().flex_none().w(px(90.)).child("Font size"))
                    .child(div().flex_1().min_w_0().child(form::suffixed(
                        size.clone(),
                        "px".into(),
                        cx,
                    ))),
            )
            .child(
                field_row()
                    .child(div().flex_none().w(px(90.)).child("Family"))
                    .child(div().flex_1().min_w_0().child(family.clone())),
            )
            .child(form::hint(
                "An empty family falls back to whatever the platform calls its \
                 fixed-pitch face."
                    .into(),
                cx,
            ));
        div()
            .w_full()
            .child(form::section("Editor".into(), cx, body))
            .into_any_element()
    })
}
