//! The application-level shell three `ruui` applications were each carrying a
//! copy of.
//!
//! `ruui` is the widget kit: buttons, inputs, trees, grids, the theme layer they
//! draw with. This crate is the layer above it — the parts of an *application*
//! that turned out not to be about the application at all. A window that draws
//! its own title bar, an updater that replaces the installed copy with the one
//! GitHub published, a dialog that says which version is running, an editor for
//! a palette, a tree of split panes. Every one of them was written once and then
//! copied twice, and every fix to one of them was three fixes or, more often,
//! one.
//!
//! ```ignore
//! ruui_shell::init(IDENTITY, cx);
//! ruui_shell::set_strings(Box::new(Words), cx);
//! ruui_shell::set_update_policy(Box::new(Settings), cx);
//! ```
//!
//! Those three calls are the whole of the wiring, and the README walks through
//! them. A host that has to apply a staged update before it can build an
//! [`App`](gpui::App) calls [`init_process_identity`] ahead of all three; it is
//! the half of [`init`] that needs no `App`, and [`init`] performs it itself.
//! What follows is the map.
//!
//! # What is here
//!
//! | Module | What it holds |
//! |---|---|
//! | [`chrome`] | [`TitlebarStyle`] and everything a window has to put back when the platform stops drawing its caption: the drag, the resize grips, the caption buttons, the background appearance. |
//! | [`caption`] | Keeping the *platform's* caption in step with the application's theme, where there still is one. |
//! | [`pane`] | [`PaneTree`], the split layout, and [`Pane`], one pane's tab strip. |
//! | [`menu_rows`] | [`MenuRow`], a context menu as a list a test can read. |
//! | [`icons`] | [`IconSet`], and the four caption glyphs every self-drawn title bar needs. |
//! | [`about`] | [`AboutDialog`]. |
//! | [`update`] | The release check and the self-update: download, verify, unpack, swap, roll back, and stage when a rename cannot happen yet. |
//! | [`update_dialog`] | [`UpdateDialog`], the state machine around all of that. |
//! | [`catalog`] | [`ThemeCatalog`], and the two implementations over `ruui`'s own palette formats. |
//! | [`theme_editor`] | [`ThemeEditor`], one entry of a catalogue edited colour by colour. |
//! | [`catalog_ui`] | [`CatalogActions`], the duplicate / edit / delete / import / export row under a picker. |
//! | [`form`] | The pieces a settings form is built out of, minus the form. |
//! | [`settings`] | [`WindowGeometry`], the monospace fallback, the window tint. |
//! | [`locale`] | Which language to render in, and one check for the files that hold the translations. |
//!
//! # What stays in the application
//!
//! Deliberately, and in every case because the shell cannot know it:
//!
//! * **The workspace.** Nothing here refers to one. Every dialog reports
//!   through an [`EventEmitter`](gpui::EventEmitter) and the application decides
//!   what that means — including the restart after an update, which is
//!   `if let Some(path) = ruui_shell::restart_path() { cx.set_restart_path(path); }`
//!   and then `cx.restart()`. The *path* is the shell's, because only the
//!   shell knows that the install has just renamed the running image aside on
//!   Linux, and that on macOS `cx.restart()` needs the `.app` bundle rather
//!   than the executable inside it; what to do with the path is not.
//! * **What a tab is.** [`Pane`] is generic over its item; the variants, and the
//!   lookups over them, are the application's.
//! * **The body of the settings form.** Which rows there are, what they mean,
//!   and how they turn into a settings struct. [`form`] holds the parts that
//!   were identical; the form was never one of them.
//! * **The settings type itself**, and both of its globals. The three
//!   applications spell theirs three different ways — one of them nested — and
//!   the pieces here take the two or three values they need instead.
//! * **Domain icons.** [`WINDOW_CONTROL_ICONS`] is here because a caption button
//!   is a caption button; a table glyph, a driver badge and a log-level mark are
//!   not.
//! * **The translations.** `rust-i18n` compiles a crate's locale files into
//!   *that* crate and keeps the active locale in a process global, so the table
//!   cannot move — which is exactly why [`Strings`] exists. The `i18n!`
//!   invocation, the `ts!` macro and `available_locales!()` all stay put;
//!   [`locale`] takes the arithmetic around them.
//! * **Packaging.** The GUID in [`AppIdentity::windows_arp_key`] is one corner
//!   of a triangle with an Inno Setup `AppId` and a winget manifest, and all
//!   three belong to the application.

#![warn(missing_docs)]

pub mod about;
pub mod caption;
pub mod catalog;
pub mod catalog_ui;
pub mod chrome;
pub mod form;
pub mod icons;
mod inject;
pub mod locale;
pub mod menu_rows;
pub mod pane;
pub mod settings;
pub mod theme_editor;
pub mod update;
pub mod update_dialog;

pub use about::{AboutDialog, AboutDialogEvent};
pub use caption::apply_caption_theme;
pub use catalog::{
    CatalogEntry, CatalogFile, EditorThemeCatalog, ImportError, Slot, ThemeCatalog, UiThemeCatalog,
    derived_slot, slot, valid_hex,
};
pub use catalog_ui::{CatalogActionEvent, CatalogActions, export_directory};
pub use chrome::{
    TitlebarStyle, client_tiling, draws_own_titlebar, render_resize_edges, titlebar_gestures,
    window_appearance, window_control_strips,
};
pub use icons::{
    IconSet, WINDOW_CLOSE, WINDOW_CONTROL_ICONS, WINDOW_MAXIMIZE, WINDOW_MINIMIZE, WINDOW_RESTORE,
    icon, window_control_icons,
};
pub use inject::{
    AppIdentity, Strings, UpdatePolicy, identity, ignored_release, init, init_process_identity,
    input_menu_labels, label, set_ignored_release, set_strings, set_update_policy, text,
};
pub use menu_rows::{MenuRow, SHORTCUT_MODIFIER, entries, greyed, labels, row};
pub use pane::{Axis, Pane, PaneId, PaneNode, PaneTree, SplitId};
pub use settings::{WindowGeometry, monospace_family, window_bounds, window_geometry, window_tint};
pub use theme_editor::{ThemeEditor, ThemeEditorEvent};
pub use update::{Asset, Check, Installed, Progress, Release, restart_path};
pub use update_dialog::{UpdateDialog, UpdateDialogEvent};
