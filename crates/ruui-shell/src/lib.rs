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
//! them. What follows is the map.
//!
//! # What is here
//!
//! | Module | What it holds |
//! |---|---|
//!
//! # What stays in the application
//!
//! Deliberately, and in every case because the shell cannot know it:
//!
//! * **The workspace.** Nothing here refers to one. Every dialog reports
//!   through an [`EventEmitter`](gpui::EventEmitter) and the application decides
//!   what that means — including the restart after an update, which is
//!   `cx.restart()` for two of the three applications and
//!   `cx.set_restart_path(…)` first for the one whose executable moves.
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

mod inject;

pub use inject::{
    AppIdentity, Strings, UpdatePolicy, identity, ignored_release, init, label,
    set_ignored_release, set_strings, set_update_policy, text,
};
