//! Every widget in this repository, in one window.
//!
//! Two jobs at once. It is what the README's screenshots are taken of, and it
//! is a worked example of the three things a host has to do for itself: install
//! an [`AssetSource`] so the icon paths widgets are handed resolve to something,
//! call the three `init` functions in order, and keep the state the stateless
//! widgets do not — which tab is active, whether a dropdown is open, what a
//! checkbox says. Being a workspace member is deliberate: CI compiles it on
//! every platform, so an example that has gone stale is a build failure rather
//! than a surprise for whoever copies it.
//!
//! ```sh
//! cargo run -p rugpui-gallery -- --theme light
//! ```
//!
//! It has a second mode, which the documentation's per-option images are taken
//! in: `--shot <name>` opens one widget in one state in a small window of its
//! own, and `--list-shots` writes the registry of those out. See
//! [`shots`] and `scripts/docshots.sh`.

use std::borrow::Cow;

use gpui::{
    AnyView, App, AssetSource, Axis, Bounds, Context, Div, DragMoveEvent, Entity, Hsla, Result,
    ScrollHandle, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use rugpui::{
    Button, ButtonVariant, Checkbox, Collapsible, DraggedThumb, EditorTheme, ListEvent, ListView,
    MenuButton, MenuEntry, ProgressBar, RangeSlider, Scrollbar, ScrollbarAxis, Segmented, Select,
    SelectOption, Slider, Spinner, Splitter, Switch, TabBar, TabItem, TabStatus, TextInput, Theme,
    Tooltip, TreeView, scroll_to, set_editor_theme, set_theme, theme, tooltip_label,
};
use rugpui_editor::{CodeSnippet, EditorView, MarkKind, highlighter_for_extension};
use rugpui_grid::{GridEvent, GridView};

mod data;
mod shots;

use data::{Catalog, Contacts, Orders};
use shots::Shot;

// --- assets -----------------------------------------------------------------

/// A folder, for the open and closed branches of the tree.
const FOLDER: &str = "icons/folder.svg";
/// A document, for the leaves of the tree.
const FILE: &str = "icons/file.svg";
/// The mark on the tab that stands for a file with something wrong in it, and
/// on the dropdown row whose driver the demo calls out.
const WARNING: &str = "icons/warning.svg";
/// A database, for the leading slot of the driver dropdown's rows.
const DATABASE: &str = "icons/database.svg";
/// A filled triangle pointing right, for the shot that replaces a
/// `Collapsible`'s own disclosure arrow.
///
/// Nothing like `rugpui::CARET_RIGHT`, which is the default those widgets draw
/// without being asked: a shot about handing a widget an icon of your own has
/// to show an icon that is visibly not the one it came with.
const TRIANGLE_RIGHT: &str = "icons/triangle-right.svg";
/// The open disclosure that goes with [`TRIANGLE_RIGHT`], and the one the shot
/// that replaces a `Select`'s chevron hands over.
const TRIANGLE_DOWN: &str = "icons/triangle-down.svg";

/// A thumbnail of a table, for the rich tooltip.
///
/// Drawn by [`img`](gpui::img) rather than by [`svg`](gpui::svg), so unlike the
/// three above its colours are the ones written in the file.
const PREVIEW: &str = "icons/preview.svg";

/// The id of the list's overlay scroll bar.
///
/// Named once because the drag handler has to rebuild the very same bar: the id
/// is what tells one bar's drag from another's.
const LIST_BAR: &str = "list-bar";

/// The gallery's icons, embedded in the binary.
///
/// gpui's [`svg`](gpui::svg) element resolves its `path` through the
/// [`AssetSource`] the application was built with and paints the result as a
/// monochrome sprite: only the alpha channel survives, and the element's
/// `text_color` supplies the colour. So the colours written in these files
/// never reach the screen — only their coverage does.
const ICONS: &[(&str, &[u8])] = &[
    (FOLDER, include_bytes!("../assets/icons/folder.svg")),
    (FILE, include_bytes!("../assets/icons/file.svg")),
    (WARNING, include_bytes!("../assets/icons/warning.svg")),
    (DATABASE, include_bytes!("../assets/icons/database.svg")),
    (PREVIEW, include_bytes!("../assets/icons/preview.svg")),
    (
        TRIANGLE_RIGHT,
        include_bytes!("../assets/icons/triangle-right.svg"),
    ),
    (
        TRIANGLE_DOWN,
        include_bytes!("../assets/icons/triangle-down.svg"),
    ),
];

/// Every table [`Icons`] answers out of, searched in order.
///
/// The widget layer's disclosure marks and menu trigger, and the shell's four
/// caption glyphs, are each a table of their own in their own crate, exactly
/// as [`ICONS`] is one here, so the gallery's asset source is the three
/// slices chained rather than a copy of any of them. `rugpui::ICONS` is not
/// optional: without it every tree, collapsible section and dropdown here
/// would draw its arrow as nothing at all.
const ICON_TABLES: &[&[(&str, &[u8])]] =
    &[rugpui::ICONS, rugpui_shell::WINDOW_CONTROL_ICONS, ICONS];

/// Every `(path, bytes)` pair the gallery can resolve.
fn icon_table() -> impl Iterator<Item = (&'static str, &'static [u8])> {
    ICON_TABLES.iter().copied().flatten().copied()
}

/// The asset source the application is built with.
///
/// Without one gpui's default answers every path with `None` and every icon
/// paints as nothing at all.
struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(icon_table()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(icon_table()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

// --- the command line -------------------------------------------------------

/// The two palettes a `--theme` names, which are chosen together but are
/// separate files and separate registries: one for the chrome, one for the code.
struct Palettes {
    /// The chrome, the result grid included.
    ui: Theme,
    /// The code editor alone.
    editor: EditorTheme,
}

/// The six built-in palette pairs, by the id `--theme` takes.
fn palettes(name: &str) -> Option<Palettes> {
    let pair = match name {
        "dark" => (Theme::dark(), EditorTheme::one_dark()),
        "light" => (Theme::light(), EditorTheme::one_light()),
        "solarized-dark" => (Theme::solarized_dark(), EditorTheme::solarized_dark()),
        "solarized-light" => (Theme::solarized_light(), EditorTheme::solarized_light()),
        "gruvbox-dark" => (Theme::gruvbox_dark(), EditorTheme::gruvbox_dark()),
        "dracula" => (Theme::dracula(), EditorTheme::dracula()),
        _ => return None,
    };
    Some(Palettes {
        ui: pair.0,
        editor: pair.1,
    })
}

/// Every id [`palettes`] answers to, for the usage line.
const THEME_IDS: &str = "dark, light, solarized-dark, solarized-light, gruvbox-dark, dracula";

/// What the whole command line comes to.
struct Args {
    /// The palette pair, already resolved.
    palettes: Palettes,
    /// The id that named it. Carried because a doc shot taken once per palette
    /// is filed under it, and the window itself cannot say which palette it is
    /// wearing.
    theme_id: String,
    /// One named shot in a window of its own, instead of the gallery.
    shot: Option<&'static Shot>,
}

/// The one line printed for a command line that does not parse.
const USAGE: &str =
    "usage: rugpui-gallery [--theme <id>] [--shot <name>] [--list-shots]\n       ids: ";

/// Stops with `code`, after `message`.
fn bail(message: &str, code: i32) -> ! {
    eprintln!("{message}");
    std::process::exit(code)
}

/// Reads the command line: `--theme <id>`, `--shot <name>`, `--list-shots`.
///
/// Every value may also be written `--flag=value`. An unknown theme or an
/// unknown shot is worth stopping for: a screenshot taken in the wrong palette,
/// or of the wrong widget, looks exactly like one taken of the right one.
///
/// `--list-shots` never returns — it writes the registry to standard output and
/// exits, which is what `scripts/docshots.sh` drives the whole run from. Its
/// empty columns are written as `-` rather than left empty, because the shell
/// splitting the line apart treats a run of tabs as one separator and would
/// otherwise read a later column as an earlier one.
fn parse_args() -> Args {
    /// `--flag value` or `--flag=value`, whichever this argument is.
    fn value(flag: &str, arg: &str, rest: &mut impl Iterator<Item = String>) -> Option<String> {
        if arg == flag {
            return rest.next();
        }
        arg.strip_prefix(flag)
            .and_then(|tail| tail.strip_prefix('='))
            .map(str::to_owned)
    }

    let usage = || format!("{USAGE}{THEME_IDS}");
    let mut args = std::env::args().skip(1);
    let mut theme_id = None;
    let mut shot = None;

    while let Some(arg) = args.next() {
        if arg == "--list-shots" {
            for shot in shots::all() {
                /// `-` for a column a shot has nothing to say in.
                fn column(value: &str) -> &str {
                    if value.is_empty() { "-" } else { value }
                }
                let motion = shot.motion.tag();
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    shot.name,
                    shot.width,
                    shot.height,
                    column(shot.per_theme),
                    column(&motion)
                );
            }
            std::process::exit(0);
        }
        if arg.starts_with("--theme") {
            match value("--theme", &arg, &mut args) {
                Some(id) => theme_id = Some(id),
                None => bail(&usage(), 2),
            }
            continue;
        }
        if arg.starts_with("--shot") {
            match value("--shot", &arg, &mut args) {
                Some(name) => shot = Some(name),
                None => bail(&usage(), 2),
            }
            continue;
        }
        bail(&usage(), 2);
    }

    let theme_id = theme_id.unwrap_or_else(|| "dark".to_owned());
    let Some(palettes) = palettes(&theme_id) else {
        bail(
            &format!("unknown theme {theme_id:?}; expected one of: {THEME_IDS}"),
            2,
        );
    };

    let shot = shot.map(|name| {
        shots::find(&name).unwrap_or_else(|| {
            let names: Vec<&str> = shots::all().map(|shot| shot.name).collect();
            bail(
                &format!(
                    "unknown shot {name:?}; expected one of:\n  {}",
                    names.join("\n  ")
                ),
                1,
            )
        })
    });

    Args {
        palettes,
        theme_id,
        shot,
    }
}

// --- start-up ---------------------------------------------------------------

fn main() {
    let args = parse_args();

    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        // In this order, and before anything renders: `rugpui::init` builds the
        // two registries and installs the default palettes, and the other two
        // register the key bindings their widgets are driven by.
        rugpui::init(cx);
        rugpui_grid::init(cx);
        rugpui_editor::init(cx);
        // Over the defaults `rugpui::init` just set.
        set_theme(args.palettes.ui.clone(), cx);
        set_editor_theme(args.palettes.editor.clone(), cx);

        match args.shot {
            Some(shot) => shots::open(shot, &args.theme_id, cx),
            None => open_gallery(cx),
        }

        // A shot is captured with `spectacle -a`, which photographs the window
        // that has the focus, so the new window has to take it.
        cx.activate(true);
    });
}

/// Opens the gallery window: every widget in the repository at once.
fn open_gallery(cx: &mut App) {
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1180.), px(1020.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("rugpui gallery".into()),
                ..Default::default()
            }),
            app_id: Some("rugpui-gallery".into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Gallery::new(window, cx)),
    )
    .expect("failed to open the gallery window");
}

// --- the view ---------------------------------------------------------------

/// The whole gallery: one view holding the state its widgets do not.
struct Gallery {
    /// Which tab of the strip is highlighted.
    tab: usize,
    /// Whether the checkbox in the first section is ticked.
    checked: bool,
    /// Which segment of the segmented control is picked.
    segment: usize,
    /// Whether the first switch is on.
    switch_on: bool,
    /// The slider's value, also what the progress bar beside it shows.
    amount: f32,
    /// The two ends of the range slider's interval.
    range: (f32, f32),
    /// The dropdown's choice, and whether its list is showing.
    choice: SharedString,
    select_open: bool,
    /// Whether the toolbar button's menu is showing.
    menu_open: bool,
    /// Whether each of the two fold-away sections is open.
    advanced_open: bool,
    ssh_open: bool,
    /// The switch in the "Advanced options" header — its own value, not the
    /// one the "Switches" section drives, so toggling one leaves the other
    /// alone.
    advanced_on: bool,
    /// The three text fields: one with a value, one empty, one disabled.
    filled: Entity<TextInput>,
    empty: Entity<TextInput>,
    locked: Entity<TextInput>,
    /// The field inside the closed "SSH tunnel" section, which is built here
    /// and yet not rendered until the section is opened.
    tunnel_host: Entity<TextInput>,
    /// The surface the "Scrollbar" list scrolls on.
    list: ScrollHandle,
    /// The three larger widgets, plus the flat list beside the tree.
    tree: Entity<TreeView<Catalog>>,
    contacts: Entity<ListView<Contacts>>,
    grid: Entity<GridView<Orders>>,
    sql: Entity<EditorView>,
    json: Entity<EditorView>,
    /// The fixed-pitch family both editors draw with.
    mono: SharedString,
    /// Whether the SQL editor breaks its long lines.
    editor_wrap: bool,
    /// Where the two dividers sit, as the first half's share of its box. Two
    /// `f32`s are the whole of what a `Splitter` does not keep for itself:
    /// `split_x` is the tree against the results beside it, `split_y` the grid
    /// against the editors below it.
    split_x: f32,
    split_y: f32,
}

impl Gallery {
    /// Builds every widget in its interesting state, so that a screenshot shows
    /// something rather than a column of empty controls.
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filled = cx.new(|cx| {
            let mut input = TextInput::new(cx).placeholder("host:port");
            input.set_content("db.internal:5432", cx);
            input
        });
        let empty = cx.new(|cx| TextInput::new(cx).placeholder("Search tables…"));
        let locked = cx.new(|cx| {
            let mut input = TextInput::new(cx).disabled(true);
            input.set_content("read-only", cx);
            input
        });
        let tunnel_host = cx.new(|cx| TextInput::new(cx).placeholder("bastion:22"));

        let tree = cx.new(|cx| {
            let mut tree = TreeView::new(Catalog, cx);
            // Two levels open, which is what shows the indent and both
            // disclosure states at once.
            tree.expand(&"warehouse", cx);
            tree.expand(&"warehouse/public", cx);
            tree.set_selected(Some("warehouse/public/orders"), cx);
            tree
        });

        // Two lines to a row, so the list is asked for a taller row than its
        // 24 px default — one height for every row is the condition
        // `uniform_list` virtualises under.
        let contacts = cx.new(|cx| {
            let mut list = ListView::new(Contacts, cx).row_height(px(44.));
            list.set_selected(Some("grace"), cx);
            list
        });
        // The host's half of a list: what activating a row *means* is the only
        // thing the widget cannot know.
        cx.subscribe(
            &contacts,
            |_gallery, _list, event: &ListEvent<&str>, _cx| {
                if let ListEvent::Activated(id) = event {
                    eprintln!("contact activated: {id}");
                }
            },
        )
        .detach();

        let grid = cx.new(|cx| {
            let mut grid = GridView::new(Orders, cx).insert_table("public.orders");
            grid.select_cell(2, 1, cx);
            grid.extend_selection(4, 3, cx);
            grid
        });
        // The two halves of editing a cell that a host has to write itself: a
        // double click (or `Enter`) says *which* cell, and the grid opens
        // whatever `Orders::cell_editor` asked for over it — a dropdown on
        // `channel`, a field on `note`.
        cx.subscribe_in(
            &grid,
            window,
            |_gallery, grid, event: &GridEvent, window, cx| match event {
                GridEvent::CellActivated { row, column } => {
                    grid.update(cx, |grid, cx| grid.begin_edit(*row, *column, window, cx));
                }
                // A real host would stage this against its result and answer
                // `cell_dirty` with it afterwards. `Orders` is a unit struct
                // over a `const` table, so there is nowhere to put a staged
                // value and the gallery only reports what it was handed: the
                // grid itself changes nothing either way.
                GridEvent::EditCommitted { row, column, value } => {
                    eprintln!("edit committed on row {row}, column {column}: {value:?}");
                }
                _ => {}
            },
        )
        .detach();

        let sql = cx.new(|cx| {
            let mut editor =
                EditorView::new(cx).highlighter(highlighter_for_extension("sql").expect("sql"));
            editor.set_text(data::SQL, cx);
            // A gutter mark is the whole-document verdict a host's parser
            // reached, handed to an editor that has never heard of one. Line 9,
            // counting from zero.
            editor.set_marks(vec![(8, MarkKind::Warning)], cx);
            editor
        });

        let json = cx.new(|cx| {
            let mut editor =
                EditorView::new(cx).highlighter(highlighter_for_extension("json").expect("json"));
            editor.set_text(data::JSON, cx);
            editor
        });

        Self {
            tab: 1,
            checked: true,
            segment: 1,
            switch_on: true,
            amount: 0.4,
            range: (0.25, 0.75),
            choice: "PostgreSQL".into(),
            select_open: false,
            menu_open: false,
            advanced_open: true,
            advanced_on: true,
            ssh_open: false,
            filled,
            empty,
            locked,
            tunnel_host,
            list: ScrollHandle::new(),
            tree,
            contacts,
            grid,
            sql,
            json,
            mono: monospace(cx),
            editor_wrap: false,
            // Roughly the 230 px the tree column used to be pinned at, out of
            // the 802 the data side gets in the window the screenshots are
            // taken in — plus the half of the 14 px gutter it now pays for
            // itself, since the flex `gap` that used to separate the columns is
            // gone and each half keeps its own breathing room beside the seam.
            split_x: 0.3,
            split_y: 0.4,
        }
    }

    /// The tab strip across the top: statuses, a mark, close buttons and a "+".
    fn tabs(&self, cx: &mut Context<Self>) -> TabBar {
        let this = cx.entity();
        TabBar::new("tabs")
            .tabs(vec![
                TabItem::new("t1", "warehouse").status(TabStatus::Connected),
                TabItem::new("t2", "orders.sql")
                    .status(TabStatus::Connecting)
                    .mark(WARNING, "One statement did not parse"),
                TabItem::new("t3", "report.json").status(TabStatus::Disconnected),
                TabItem::new("t4", "staging").status(TabStatus::Error),
            ])
            .active(self.tab)
            .tooltips("All tabs", "New tab", "Close")
            .on_select(move |index, _window, cx| {
                this.update(cx, |gallery, cx| {
                    gallery.tab = index;
                    cx.notify();
                });
            })
            .on_close(|_index, _window, _cx| {})
            .on_new(|_window, _cx| {})
    }

    /// The left-hand column: the small widgets a form is built out of.
    ///
    /// The four stateless indicators and pickers — spinner, segmented control,
    /// select and scrollbar — live in the middle column instead, under the
    /// tree, so that this column is short enough to fit beside it.
    fn controls(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme(cx);
        let this = cx.entity();

        let buttons = section("Buttons", &palette).child(
            row()
                .child(Button::new("primary", "Connect"))
                .child(Button::new("secondary", "Cancel").variant(ButtonVariant::Secondary))
                .child(Button::new("ghost", "Reset").variant(ButtonVariant::Ghost))
                .child(Button::new("danger", "Drop").variant(ButtonVariant::Danger))
                .child(Button::new("disabled", "Connect").disabled(true)),
        );

        let checked = self.checked;
        let checkboxes = section("Checkboxes", &palette).child(
            row()
                .child(
                    Checkbox::new("wrap", "Wrap long values")
                        .checked(checked)
                        .on_toggle({
                            let this = this.clone();
                            move |value, _window, cx| {
                                this.update(cx, |gallery, cx| {
                                    gallery.checked = value;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(Checkbox::new("nulls", "Show nulls"))
                .child(Checkbox::new("locked-check", "Read only").checked(true)),
        );

        let collapsibles = section("Collapsible", &palette).child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    Collapsible::new("advanced", "Advanced options")
                        .open(self.advanced_open)
                        .on_toggle({
                            let this = this.clone();
                            move |open, _window, cx| {
                                this.update(cx, |gallery, cx| {
                                    gallery.advanced_open = open;
                                    cx.notify();
                                });
                            }
                        })
                        // Beside the disclosure rather than inside it: arming
                        // the block and folding it away are two gestures, and
                        // a switch nested in the header would perform both.
                        .trailing(
                            Switch::new("advanced-on", "")
                                .checked(self.advanced_on)
                                .on_toggle({
                                    let this = this.clone();
                                    move |value, _window, cx| {
                                        this.update(cx, |gallery, cx| {
                                            gallery.advanced_on = value;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(Checkbox::new("advanced-nulls", "Show nulls"))
                        .child(Checkbox::new("advanced-locked", "Read only").checked(true)),
                )
                .child(
                    // Closed, so `tunnel_host` is built and yet never drawn —
                    // which is the point: a folded section costs no elements
                    // and holds no focus.
                    Collapsible::new("ssh", "SSH tunnel")
                        .open(self.ssh_open)
                        .on_toggle({
                            let this = this.clone();
                            move |open, _window, cx| {
                                this.update(cx, |gallery, cx| {
                                    gallery.ssh_open = open;
                                    cx.notify();
                                });
                            }
                        })
                        .child(self.tunnel_host.clone()),
                ),
        );

        let switches = section("Switches", &palette).child(
            row()
                .child(
                    Switch::new("wifi", "Auto-reconnect")
                        .checked(self.switch_on)
                        .on_toggle({
                            let this = this.clone();
                            move |value, _window, cx| {
                                this.update(cx, |gallery, cx| {
                                    gallery.switch_on = value;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(Switch::new("telemetry", "Send telemetry")),
        );

        let amount = self.amount;
        let (low, high) = self.range;
        let slider = section("Slider and progress", &palette)
            .child(Slider::new("amount").value(amount).step(0.05).on_change({
                let this = this.clone();
                move |value, _window, cx| {
                    this.update(cx, |gallery, cx| {
                        gallery.amount = value;
                        cx.notify();
                    });
                }
            }))
            .child(
                RangeSlider::new("band")
                    .low(low)
                    .high(high)
                    .step(0.05)
                    .on_change({
                        let this = this.clone();
                        move |low, high, _window, cx| {
                            this.update(cx, |gallery, cx| {
                                gallery.range = (low, high);
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(ProgressBar::new("amount-progress").fraction(amount))
            .child(ProgressBar::new("loading").indeterminate());

        let fields = section("Text fields", &palette)
            .child(self.filled.clone())
            .child(self.empty.clone())
            .child(self.locked.clone());

        let menu = section("Menu and tooltip", &palette).child(
            row()
                .child(
                    MenuButton::new("app-menu")
                        .tooltip("Everything this window can do")
                        .open(self.menu_open)
                        .entries(vec![
                            MenuEntry::new("New tab").shortcut("Ctrl+T"),
                            MenuEntry::new("Run statement").shortcut("Ctrl+Enter"),
                            MenuEntry::separator(),
                            MenuEntry::new("Wrap long values").checked(self.checked),
                            MenuEntry::new("Export…").disabled(true),
                        ])
                        .on_open_change({
                            let this = this.clone();
                            move |open, _window, cx| {
                                this.update(cx, |gallery, cx| {
                                    gallery.menu_open = open;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .id("tooltip-target")
                        .px(px(8.))
                        .py(px(4.))
                        .rounded_md()
                        .border_1()
                        .border_color(palette.border)
                        .text_color(palette.text_muted)
                        .tooltip(tooltip_label("Rests here to show a tooltip"))
                        .child("Hover me"),
                )
                .child(
                    div()
                        .id("tooltip-rich")
                        .px(px(8.))
                        .py(px(4.))
                        .rounded_md()
                        .border_1()
                        .border_color(palette.border)
                        .text_color(palette.text_muted)
                        .tooltip(self.preview_tooltip())
                        .child("Hover for a preview"),
                ),
        );

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(330.))
            .gap(px(16.))
            .child(buttons)
            .child(checkboxes)
            .child(collapsibles)
            .child(switches)
            .child(slider)
            .child(fields)
            .child(menu)
    }

    /// The rich tooltip: a thumbnail, a caption and four lines of highlighted
    /// SQL, in one [`Tooltip`].
    ///
    /// Three parts, three kinds. The image is an `img` and therefore keeps the
    /// colours written in its file, unlike the monochrome `svg` icons the tree
    /// and the tabs use. The note is the caption slot. The snippet is a
    /// [`CodeSnippet`] handed in through `.element(..)`, which is how anything
    /// `rugpui` has never heard of gets into the column — it reads the *editor*
    /// palette, so it matches the two editors on the right rather than the
    /// tooltip's own surface.
    ///
    /// Everything the closure needs is cloned into it: gpui calls it afresh on
    /// every hover, so it cannot borrow from the gallery.
    fn preview_tooltip(&self) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let sql = highlighter_for_extension("sql").expect("sql ships with rugpui-editor");
        let mono = self.mono.clone();
        Tooltip::new()
            .image(PREVIEW, px(96.))
            .note("public.orders — 12 rows")
            .element(move |_window, _cx| {
                CodeSnippet::new(data::SQL, sql.clone())
                    .font_family(mono.clone())
                    .max_lines(4)
                    .into_any_element()
            })
            .build()
    }

    /// A scrolling list with an overlay bar over it.
    ///
    /// The bar is built afresh on every render from the handle the list is
    /// tracked by, and dragging its thumb is the owner's to handle — the same
    /// id that built the bar is what tells that drag from any other. Left at
    /// the default [`Fade::Shown`](rugpui::scrollbar::Fade), so it is simply
    /// always there; an application that would rather the bar came and went
    /// with the scrolling keeps a [`ScrollbarState`] beside the handle and
    /// passes `.fade(state.fade())`, which is what the grid, the tree and the
    /// editors below do for their own.
    fn list(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme(cx);
        let bar = Scrollbar::for_handle(LIST_BAR, ScrollbarAxis::Vertical, &self.list);

        section("Scrollbar", &palette).child(
            div()
                .relative()
                .h(px(150.))
                .rounded_md()
                .border_1()
                .border_color(palette.border)
                .on_drag_move(cx.listener(
                    |gallery, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                        let bar =
                            Scrollbar::for_handle(LIST_BAR, ScrollbarAxis::Vertical, &gallery.list);
                        if let Some(progress) = bar.dragged(event, cx) {
                            scroll_to(&gallery.list, ScrollbarAxis::Vertical, progress);
                            cx.notify();
                        }
                    },
                ))
                .child(
                    div()
                        .id("list")
                        .track_scroll(&self.list)
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
                .children(bar.render(&palette)),
        )
    }

    /// The middle column: the tree, the flat list under it, and under those
    /// the small stateless indicators and pickers — spinner, segmented
    /// control, select and scrollbar — for which a form field's two-column
    /// layout would waste width. The right-hand column holds the grid and the two editors.
    /// Divided from the left by one [`Splitter`], and the grid from the
    /// editors by a second.
    ///
    /// The dividers are the point of the arrangement. The tree used to be
    /// pinned at 230 px and the grid at 256, and both numbers are now a share
    /// of whatever box the window happens to give them — which is what a
    /// splitter buys, and the reason the halves are `min_w_0`/`min_h_0` and
    /// grow into their share rather than asking for a height of their own.
    ///
    /// The padding beside each seam is the gap the flex `gap` used to leave:
    /// a splitter's halves meet on the divider, so the breathing room has to be
    /// paid for from inside them.
    fn data(&self, cx: &mut Context<Self>) -> Div {
        let palette = theme(cx);
        let this = cx.entity();

        // Fills whatever height is left in its column, so that the column
        // does not end in a band of nothing — but never below 200 px, so the
        // four sections beneath it cannot squeeze the tree away entirely.
        let tree = section("Tree", &palette)
            .flex_1()
            .min_h_0()
            .min_h(px(200.))
            .child(framed(&palette).flex_1().child(self.tree.clone()));

        // Fixed rather than flexible: the tree above already takes whatever is
        // left in the column, and two greedy sections would fight over it.
        let contacts = section("List", &palette).child(
            framed(&palette)
                .flex_none()
                .h(px(180.))
                .child(self.contacts.clone()),
        );

        let spinners = section("Spinner", &palette).child(
            row()
                .child(Spinner::new("spinner-small"))
                .child(Spinner::new("spinner-large").size(px(24.))),
        );

        let segmented = section("Segmented", &palette).child(
            Segmented::new("format")
                .options(vec![("csv", "CSV"), ("json", "JSON"), ("insert", "INSERT")])
                .selected(self.segment)
                .on_select({
                    let this = this.clone();
                    move |index, _window, cx| {
                        this.update(cx, |gallery, cx| {
                            gallery.segment = index;
                            cx.notify();
                        });
                    }
                }),
        );

        let select = section("Select", &palette).child(
            Select::new("driver")
                // Every driver gets the same leading mark; the first also
                // gets a trailing one, so both slots are on show and the
                // gallery says out loud that a list may mark only some rows.
                .options(
                    ["PostgreSQL", "MySQL", "Oracle", "SQLite", "SQL Server"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, name)| {
                            let option = SelectOption::new(name).leading(DATABASE);
                            match index {
                                0 => option.trailing(WARNING),
                                _ => option,
                            }
                        }),
                )
                .selected(Some(self.choice.clone()))
                .placeholder("Pick a driver")
                .open(self.select_open)
                .width(px(180.))
                .on_select({
                    let this = this.clone();
                    move |_index, text, _window, cx| {
                        let text = SharedString::from(text.to_owned());
                        this.update(cx, |gallery, cx| {
                            gallery.choice = text;
                            cx.notify();
                        });
                    }
                })
                .on_open_change({
                    let this = this.clone();
                    move |open, _window, cx| {
                        this.update(cx, |gallery, cx| {
                            gallery.select_open = open;
                            cx.notify();
                        });
                    }
                }),
        );

        let list = self.list(cx);

        // Fills its half of the vertical split, rather than the fixed 256 px it
        // was pinned at before the divider existed.
        let grid = section("Grid", &palette)
            .flex_1()
            .min_h_0()
            .child(framed(&palette).flex_1().child(self.grid.clone()));

        let sql = section("Editor — SQL", &palette)
            .child(
                Switch::new("editor-wrap", "Wrap long lines")
                    .checked(self.editor_wrap)
                    .on_toggle({
                        let this = this.clone();
                        let editor = self.sql.clone();
                        move |value, _window, cx| {
                            editor.update(cx, |editor, cx| editor.set_word_wrap(value, cx));
                            this.update(cx, |gallery, cx| {
                                gallery.editor_wrap = value;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                framed(&palette)
                    .h(px(228.))
                    .font_family(self.mono.clone())
                    .text_size(px(12.5))
                    .child(self.sql.clone()),
            );

        // Like the tree, this one takes whatever height is left in its column,
        // so neither column ends in a band of nothing.
        let json = section("Editor — JSON", &palette).flex_1().min_h_0().child(
            framed(&palette)
                .flex_1()
                .min_h(px(172.))
                .font_family(self.mono.clone())
                .text_size(px(12.5))
                .child(self.json.clone()),
        );

        let editors = div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .pt(px(7.))
            .gap(px(14.))
            .child(sql)
            .child(json);

        let results = Splitter::new("results-split", Axis::Vertical)
            .ratio(self.split_y)
            .first(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_h_0()
                    .pb(px(7.))
                    .child(grid),
            )
            .second(editors)
            .on_change({
                let this = this.clone();
                move |ratio, _window, cx| {
                    this.update(cx, |gallery, cx| {
                        gallery.split_y = ratio;
                        cx.notify();
                    });
                }
            });

        div().flex().flex_1().min_w_0().child(
            Splitter::new("data-split", Axis::Horizontal)
                .ratio(self.split_x)
                .min_ratio(0.15)
                .first(
                    div()
                        .flex()
                        .flex_col()
                        .size_full()
                        .min_h_0()
                        .gap(px(16.))
                        .pr(px(7.))
                        .child(tree)
                        .child(contacts)
                        .child(spinners)
                        .child(segmented)
                        .child(select)
                        .child(list),
                )
                .second(div().flex().size_full().min_w_0().pl(px(7.)).child(results))
                .on_change(move |ratio, _window, cx| {
                    this.update(cx, |gallery, cx| {
                        gallery.split_x = ratio;
                        cx.notify();
                    });
                }),
        )
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme(cx);
        let controls = self.controls(cx);
        let data = self.data(cx);
        let tabs = self.tabs(cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(px(13.))
            .child(tabs)
            .child(
                div()
                    .id("body")
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .gap(px(16.))
                    .p(px(16.))
                    .overflow_y_scroll()
                    .child(controls)
                    .child(data),
            )
    }
}

// --- small helpers ----------------------------------------------------------

/// A captioned block. The caller adds the widgets themselves.
fn section(title: &'static str, palette: &Theme) -> Div {
    div().flex().flex_col().gap(px(6.)).child(
        div()
            .text_size(px(10.5))
            .text_color(palette.text_muted)
            .child(title),
    )
}

/// A row of small widgets that wraps rather than overflowing.
fn row() -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
}

/// The bordered box the three larger widgets are dropped into.
fn framed(palette: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_h_0()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(palette.border)
}

/// The fixed-pitch family the editors draw with.
///
/// The literal `"monospace"` is a *fontconfig* alias: it resolves to a real
/// face on Linux and nowhere else, so on the other two platforms a family that
/// actually exists has to be named, and the only way to know which ones exist
/// is to ask.
fn monospace(cx: &App) -> SharedString {
    const CANDIDATES: &[&str] = &[
        "SF Mono",
        "Menlo",
        "Cascadia Mono",
        "Consolas",
        "DejaVu Sans Mono",
    ];
    let installed = cx.text_system().all_font_names();
    CANDIDATES
        .iter()
        .find_map(|candidate| {
            installed
                .iter()
                .find(|name| name.eq_ignore_ascii_case(candidate))
                .cloned()
        })
        .map_or_else(|| SharedString::new_static("monospace"), SharedString::from)
}

/// The colour a tree row's icon takes, which is the theme's icon tint until the
/// row is the selected one.
fn icon_tint(selected: bool, palette: &Theme) -> Hsla {
    if selected { palette.text } else { palette.icon }
}
