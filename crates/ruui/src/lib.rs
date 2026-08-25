//! Reusable gpui widgets, and the theme layer they draw with.
//!
//! The crate is a widget kit and nothing more: it knows nothing of the domain
//! any application built on it works in — no connections, no documents, no
//! sessions — and only about colors ([`theme`], [`editor_theme`]), text entry
//! ([`text_input`]), buttons ([`button`]), tabs ([`tab_bar`]), dropdown menus
//! ([`menu`]), one-of-many dropdowns over plain strings ([`select`]) and over
//! palettes ([`scheme_select`]), hover tooltips ([`tooltip`]), dialogs
//! ([`modal`]), overlay scroll indicators ([`scrollbar`]), lazily filled trees
//! ([`tree`]) and the caption buttons of a self-drawn title bar
//! ([`window_controls`]). A widget that would need to understand the host's
//! data to draw itself belongs in the host, not here — the tree included: it
//! knows about ids the host invents and rows the host draws, and nothing about
//! what they mean.
//!
//! For the same reason no widget here holds a user-facing string of its own:
//! labels arrive from the host, which is what keeps a localized application
//! localized without this crate having heard of a locale.
//!
//! Two palettes live side by side and are chosen independently: [`theme`] is
//! the chrome, the result grid included, and [`editor_theme`] is the code
//! editor alone — a different file, a different directory and a different set
//! of tokens. [`theme_store`] reads both from directories the host names
//! through [`ThemeDirs`].
//!
//! Call [`init`] once during application start-up so the widgets that need key
//! bindings get them.

#![warn(missing_docs)]

pub mod button;
pub mod checkbox;
pub mod editor_theme;
pub mod editor_theme_picker;
pub mod menu;
pub mod modal;
pub mod scheme_select;
pub mod scrollbar;
pub mod segmented;
pub mod select;
pub mod tab_bar;
pub mod text_input;
pub mod theme;
pub mod theme_store;
pub mod tooltip;
pub mod tree;
pub mod window_controls;

pub use button::{Button, ButtonVariant};
pub use checkbox::Checkbox;
pub use editor_theme::{
    CustomEditorTheme, EditorTheme, EditorThemeColors, EditorThemeEntry, EditorThemeFile,
    EditorThemeRegistry, editor_theme, set_editor_theme,
};
pub use editor_theme_picker::{EditorThemePicker, EditorThemeSwatch};
pub use menu::{ContextMenu, MenuButton, MenuEntry};
pub use modal::{form_row, modal};
pub use scheme_select::{SchemePreview, SchemeSelect, SchemeSwatch};
pub use scrollbar::{
    DraggedThumb, Scrollbar, ScrollbarAxis, ScrollbarState, hide_later, hide_now, scroll_to,
    scrolled,
};
pub use segmented::Segmented;
pub use select::Select;
pub use tab_bar::{TabBar, TabItem, TabMark, TabStatus};
pub use text_input::{InputMenuLabels, TextInput};
pub use theme::{
    CustomUiTheme, Theme, ThemeColors, ThemeEntry, ThemeFile, ThemeRegistry, parse_hex, set_theme,
    set_window_tint, theme, to_hex, window_tint, window_translucent,
};
pub use theme_store::ThemeDirs;
pub use tooltip::tooltip_label;
pub use tree::{ChildState, TreeEvent, TreeRow, TreeRowInfo, TreeSource, TreeView};
pub use window_controls::{WindowControlIcons, WindowControls};

use gpui::App;

/// Registers everything the widget layer needs before the first window opens.
///
/// Both registries are installed empty and both palettes are set to their
/// defaults, so that a view rendered before [`theme_store::reload`] has read the
/// user's files still has colors to draw with. The window opacity defaults to
/// fully opaque for the same reason: a view drawn before the shell has said what
/// the window looks like still has an answer, and it is the one that paints.
pub fn init(cx: &mut App) {
    ThemeRegistry::init(cx);
    EditorThemeRegistry::init(cx);
    set_theme(Theme::dark(), cx);
    set_editor_theme(EditorTheme::default(), cx);
    set_window_tint(1.0, cx);
    TextInput::init(cx);
    tree::init(cx);
}
