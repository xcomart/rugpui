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

use std::borrow::Cow;

use gpui::{
    AnyView, App, AssetSource, Axis, Bounds, Context, Div, DragMoveEvent, Entity, Hsla, Result,
    ScrollHandle, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use rugpui::{
    Button, ButtonVariant, Checkbox, DraggedThumb, EditorTheme, MenuButton, MenuEntry, ProgressBar,
    Scrollbar, ScrollbarAxis, Segmented, Select, Slider, Spinner, Splitter, Switch, TabBar,
    TabItem, TabStatus, TextInput, Theme, Tooltip, TreeView, scroll_to, set_editor_theme,
    set_theme, theme, tooltip_label,
};
use rugpui_editor::{CodeSnippet, EditorView, MarkKind, highlighter_for_extension};
use rugpui_grid::GridView;

mod data;

use data::{Catalog, Orders};

// --- assets -----------------------------------------------------------------

/// A folder, for the open and closed branches of the tree.
const FOLDER: &str = "icons/folder.svg";
/// A document, for the leaves of the tree.
const FILE: &str = "icons/file.svg";
/// The mark on the tab that stands for a file with something wrong in it.
const WARNING: &str = "icons/warning.svg";
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
    (PREVIEW, include_bytes!("../assets/icons/preview.svg")),
];

/// The asset source the application is built with.
///
/// Without one gpui's default answers every path with `None` and every icon
/// paints as nothing at all.
struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| *name)
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

/// Reads `--theme <id>` (or `--theme=<id>`) off the command line.
///
/// An unknown id is worth stopping for: a screenshot taken in the wrong palette
/// looks exactly like one taken in the right one.
fn requested_theme() -> Palettes {
    let mut args = std::env::args().skip(1);
    let mut chosen = None;
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--theme" => args.next(),
            other => other.strip_prefix("--theme=").map(str::to_owned),
        };
        match value {
            Some(value) => chosen = Some(value),
            None => {
                eprintln!("usage: rugpui-gallery [--theme <{THEME_IDS}>]");
                std::process::exit(2);
            }
        }
    }
    match chosen {
        None => palettes("dark").expect("dark is a built-in palette"),
        Some(name) => palettes(&name).unwrap_or_else(|| {
            eprintln!("unknown theme {name:?}; expected one of: {THEME_IDS}");
            std::process::exit(2);
        }),
    }
}

// --- start-up ---------------------------------------------------------------

fn main() {
    let palettes = requested_theme();

    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        // In this order, and before anything renders: `rugpui::init` builds the
        // two registries and installs the default palettes, and the other two
        // register the key bindings their widgets are driven by.
        rugpui::init(cx);
        rugpui_grid::init(cx);
        rugpui_editor::init(cx);
        // Over the defaults `rugpui::init` just set.
        set_theme(palettes.ui.clone(), cx);
        set_editor_theme(palettes.editor.clone(), cx);

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

        cx.activate(true);
    });
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
    /// The dropdown's choice, and whether its list is showing.
    choice: SharedString,
    select_open: bool,
    /// Whether the toolbar button's menu is showing.
    menu_open: bool,
    /// The three text fields: one with a value, one empty, one disabled.
    filled: Entity<TextInput>,
    empty: Entity<TextInput>,
    locked: Entity<TextInput>,
    /// The surface the "Scrollbar" list scrolls on.
    list: ScrollHandle,
    /// The three larger widgets.
    tree: Entity<TreeView<Catalog>>,
    grid: Entity<GridView<Orders>>,
    sql: Entity<EditorView>,
    json: Entity<EditorView>,
    /// The fixed-pitch family both editors draw with.
    mono: SharedString,
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
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
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

        let tree = cx.new(|cx| {
            let mut tree = TreeView::new(Catalog, cx);
            // Two levels open, which is what shows the indent and both
            // disclosure states at once.
            tree.expand(&"warehouse", cx);
            tree.expand(&"warehouse/public", cx);
            tree.set_selected(Some("warehouse/public/orders"), cx);
            tree
        });

        let grid = cx.new(|cx| {
            let mut grid = GridView::new(Orders, cx).insert_table("public.orders");
            grid.select_cell(2, 1, cx);
            grid.extend_selection(4, 3, cx);
            grid
        });

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
            choice: "PostgreSQL".into(),
            select_open: false,
            menu_open: false,
            filled,
            empty,
            locked,
            list: ScrollHandle::new(),
            tree,
            grid,
            sql,
            json,
            mono: monospace(cx),
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
            .child(ProgressBar::new("amount-progress").fraction(amount))
            .child(ProgressBar::new("loading").indeterminate());

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
                .options(
                    ["PostgreSQL", "MySQL", "Oracle", "SQLite", "SQL Server"]
                        .map(SharedString::new_static),
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
            .child(switches)
            .child(slider)
            .child(spinners)
            .child(segmented)
            .child(select)
            .child(fields)
            .child(menu)
            .child(self.list(cx))
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

    /// The right-hand column: the tree, the grid and the two editors, divided
    /// by two [`Splitter`]s.
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

        // Fills whatever height is left in its half, so that the column does
        // not end in a band of nothing.
        let tree = section("Tree", &palette)
            .flex_1()
            .min_h_0()
            .child(framed(&palette).flex_1().child(self.tree.clone()));

        // Fills its half of the vertical split, rather than the fixed 256 px it
        // was pinned at before the divider existed.
        let grid = section("Grid", &palette)
            .flex_1()
            .min_h_0()
            .child(framed(&palette).flex_1().child(self.grid.clone()));

        let sql = section("Editor — SQL", &palette).child(
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
                        .pr(px(7.))
                        .child(tree),
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
