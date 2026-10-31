//! Dropdown menus: the toolbar application menu, and the menu a right-click
//! opens at the pointer.
//!
//! Windows and Linux get no native menu bar from gpui — [`gpui::App::set_menus`]
//! only builds one on macOS — so the shell draws its own. [`MenuButton`] is that
//! drawing: a compact glyph button which, while open, paints a list of
//! [`MenuEntry`] rows over the rest of the window. [`ContextMenu`] paints the
//! same list, without a trigger, wherever the caller says.
//!
//! Like every other widget here both are stateless: the parent view owns the
//! open flag — and, for a context menu, the position that goes with it — passes
//! it in on every render, and closes the menu from
//! [`MenuButton::on_open_change`] or [`ContextMenu::on_dismiss`].
//!
//! An open menu holds the keyboard, wherever the focus is: `Up`, `Down`,
//! `Home` and `End` move a highlight over the rows that can be run, `Enter` and
//! `Space` run it, a typed letter jumps to the next row starting with it, and
//! `Escape` or `Tab` closes the menu. Neither menu takes the focus from the
//! surface it was opened over — a field's edit is not committed by a blur, and
//! a command acts on the surface it was opened for — so the keys are caught
//! ahead of that surface's own bindings rather than at the menu, and closing
//! the menu leaves the focus exactly where it was.
//!
//! [`OpenContextMenu`] (Shift+F10, the Menu key) and [`OpenAppMenu`] (F10) are
//! the two keys that open a menu in the first place; the surface or the view
//! that owns the menu handles them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, App, Div, ElementId, Hsla, KeyBinding, Keystroke, Pixels, Point, ScrollHandle,
    SharedString, Size, Subscription, Window, actions, anchored, deferred, div, point, prelude::*,
    px, svg,
};

use crate::overlay;
use crate::scale::{TextSize, scaled};
use crate::select_keyboard::Typeahead;
use crate::theme::{Theme, theme};
use crate::tooltip::tooltip_label;

actions!(
    rugpui_menu,
    [
        /// Open the context menu of whatever has the focus, from the keyboard.
        ///
        /// Bound to `Shift+F10` and the Menu key by [`init`], with no key
        /// context, and handled by nobody here: a surface that has a context
        /// menu handles it with `.on_action(..)` and opens the menu where its
        /// selection is, with [`ContextMenu::highlight_first`]. Unhandled, the
        /// key goes on as if the binding were not there.
        OpenContextMenu,
        /// Open the application menu, from the keyboard.
        ///
        /// Bound to `F10` by [`init`], with no key context. The view that draws
        /// the application's [`MenuButton`] handles it by opening the menu with
        /// [`MenuButton::highlight_first`].
        OpenAppMenu,
    ]
);

/// Registers the keys that open a menu from the keyboard; see
/// [`OpenContextMenu`] and [`OpenAppMenu`]. Called by [`crate::init`].
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("shift-f10", OpenContextMenu, None),
        KeyBinding::new("menu", OpenContextMenu, None),
        KeyBinding::new("f10", OpenAppMenu, None),
    ]);
}

/// Which corner of a floating panel is pinned to the point it was opened at,
/// and so which way the panel grows from there.
///
/// Re-exported from gpui: a view choosing how its [`ContextMenu`] opens names
/// one of these, and it should not have to reach past this crate for the name.
pub use gpui::Anchor;

/// Edge length of the trigger button.
const TRIGGER_SIZE: f32 = 28.;

/// Edge length of the icon inside the trigger, when it carries one.
///
/// Matches the toolbar's other icon buttons rather than the glyph it may stand
/// in for: a vector drawn at its own size sits in the row at the same weight as
/// the panel toggle beside it, which a font glyph scaled to the same box would
/// not.
const TRIGGER_ICON: f32 = 16.;

/// Style group of a trigger, so hovering the button recolors the icon in it.
///
/// Shared by every [`MenuButton`] rather than made unique per button: a
/// `group_hover` resolves against the nearest ancestor carrying the name, so
/// two triggers side by side each answer to their own.
const TRIGGER_GROUP: &str = "menu-trigger";

/// Gap between the bottom of the trigger and the top of the dropdown, so
/// that the panel clears the button it hangs from. Not scaled: the trigger's
/// own height is, and the gap is the frame around it.
const DROP_GAP: f32 = 4.;

/// Width of a [`MenuButton`] dropdown panel.
///
/// Wide enough for the longest row that menu has — the pane commands name the
/// thing they act on ("Split right of current tab") and carry a shortcut hint —
/// with room for a translation of it, since a row neither wraps nor ellipsises.
///
/// Fixed rather than content-sized because the button's menu is the same menu
/// every time it is opened: a width that followed the entries would make the
/// panel breathe as commands come and go, under a trigger that never moves.
const PANEL_WIDTH: f32 = 280.;

/// Narrowest a content-sized panel draws.
///
/// A context menu is built for one right-click and can be three short words
/// long; without a floor it would come out as a sliver that reads as a glitch
/// rather than a menu.
const PANEL_MIN_WIDTH: f32 = 180.;

/// Widest a content-sized panel draws.
///
/// Past this a row is truncated instead: the label is a command, not the text
/// of the object it acts on, and a panel wide enough for a fully qualified name
/// would cover the surface the menu was opened over.
const PANEL_MAX_WIDTH: f32 = 360.;

/// Height of one command row, at the default UI scale.
const ROW_HEIGHT: f32 = 28.;

/// Width of the column a check mark sits in.
const CHECK_WIDTH: f32 = 16.;

/// Edge length of the color square a row can lead with; see
/// [`MenuEntry::swatch`].
pub(crate) const SWATCH_SIZE: f32 = 12.;

/// Space between a row's color square and its label.
const SWATCH_GAP: f32 = 8.;

/// A square of `color`, framed so that a color close to the panel's own still
/// reads as a square: what [`MenuEntry::swatch`] and
/// [`SelectOption::swatch`](crate::select::SelectOption::swatch) draw.
///
/// `size` is the square's edge; a menu row and a dropdown option pass
/// [`SWATCH_SIZE`] at the UI scale.
pub(crate) fn color_swatch(color: Hsla, theme: &Theme, size: Pixels) -> Div {
    div()
        .flex_none()
        .size(size)
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .bg(color)
}

/// What a checked row is marked with.
///
/// A glyph rather than an asset: the mark has to line up with a row of text and
/// take the row's color, which is what a character does for free.
const CHECK_MARK: &str = "\u{2713}";

/// Key a test looks a drawn panel's bounds up by.
///
/// gpui compiles the call that records it away outside a test build, so this
/// costs the application nothing; it is here because the width of a panel is
/// now a decision rather than a constant, and a decision wants a test.
pub(crate) const PANEL_SELECTOR: &str = "rugpui-menu-panel";

/// Distance the dropdown keeps from the window edges when it would overflow.
const WINDOW_MARGIN: f32 = 6.;

/// Draw order of the click-catching backdrop, relative to other deferred draws.
const BACKDROP_PRIORITY: usize = 1;

/// Draw order of the dropdown panel; above [`BACKDROP_PRIORITY`] so that the
/// backdrop never eats clicks meant for a menu row.
const PANEL_PRIORITY: usize = 2;

/// Callback fired when a menu row is activated.
type ActivateHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Callback fired when an open menu wants to close itself.
type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Callback fired when the menu wants to open or close itself.
type OpenChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// One row of a [`MenuButton`] or [`ContextMenu`] dropdown.
///
/// A row is either a command — a label, an optional shortcut hint and a
/// callback — or a horizontal rule built with [`MenuEntry::separator`].
pub struct MenuEntry {
    /// Text shown on the left of the row.
    label: SharedString,
    /// Shortcut hint shown right-aligned and muted.
    shortcut: Option<SharedString>,
    /// Invoked when the row is clicked.
    on_activate: Option<ActivateHandler>,
    /// Whether the row is a rule rather than a command.
    separator: bool,
    /// Whether the row is shown but cannot be run.
    disabled: bool,
    /// Whether the row carries a check mark.
    checked: bool,
    /// The color square drawn before the label, if any.
    swatch: Option<Hsla>,
}

impl MenuEntry {
    /// Creates a command row with no shortcut hint and no callback.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            on_activate: None,
            separator: false,
            disabled: false,
            checked: false,
            swatch: None,
        }
    }

    /// The text this row shows, empty for a [`MenuEntry::separator`].
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// Whether this row is a rule rather than a command.
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// Creates a horizontal rule between two groups of commands.
    pub fn separator() -> Self {
        Self {
            label: SharedString::default(),
            shortcut: None,
            on_activate: None,
            separator: true,
            disabled: false,
            checked: false,
            swatch: None,
        }
    }

    /// Sets the shortcut hint shown at the right edge of the row.
    ///
    /// The hint is decoration only: the key binding itself is registered by the
    /// application, and the menu dispatches the same action the binding does.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Sets the callback run when the row is clicked.
    ///
    /// The menu closes itself afterwards, so the callback does not have to.
    pub fn on_activate(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Some(Rc::new(handler));
        self
    }

    /// Shows the row without letting it be run.
    ///
    /// A disabled row is drawn muted, takes no hover and no pointer cursor, and
    /// carries no click handler at all — clicking it runs nothing *and* leaves
    /// the menu open, since the panel occludes the backdrop that a press
    /// otherwise dismisses the menu from. That is the point of showing the row
    /// rather than dropping it: a command that is missing tells the reader
    /// nothing, while one that is grayed out says the surface has it and this
    /// is not the moment.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks the row as the one that is currently in effect.
    ///
    /// For the menus that show a choice rather than an action — which tab is
    /// showing, which sort a column is under. The mark sits in a column of its
    /// own before the label, and that column is laid out on *every* row of a
    /// menu that has a checked one, so the labels stay in a line; a menu with
    /// nothing checked never gets the column, and is drawn exactly as it was
    /// before the slot existed.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Leads the label with a square of `color`.
    ///
    /// For the menus whose rows *are* colors, where the name alone says less
    /// than the color it stands for. Like the check mark, the square has a
    /// column of its own that is laid out on every row of a menu that has one,
    /// so a row without a color — "none", say — keeps its label in line with
    /// the rest.
    pub fn swatch(mut self, color: Hsla) -> Self {
        self.swatch = Some(color);
        self
    }
}

/// How wide a dropdown panel draws itself.
enum PanelWidth {
    /// One width whatever is in it: what a menu hanging off a fixed trigger
    /// takes, and what a caller who named a width with [`ContextMenu::width`]
    /// asked for.
    Fixed(Pixels),
    /// As wide as the widest row, held between two bounds, for a menu built for
    /// a single right-click on a surface whose commands vary.
    Content {
        /// Floor, so a two-word menu is still a menu.
        min: f32,
        /// Ceiling, past which a label is truncated with an ellipsis.
        max: f32,
    },
}

/// The tallest a panel may draw in `window`.
///
/// What is left of the window once the margin `anchored` snaps back to is taken
/// off both edges: the most a panel can be and still be reachable wherever it
/// ends up, inside the client-side shadow band if the window has one.
fn panel_max_height(window: &Window) -> Pixels {
    overlay::max_height(window, px(WINDOW_MARGIN))
}

/// One row as the keyboard sees it: what it is called, whether it can be run,
/// and what running it does.
struct KeyRow {
    /// The label, matched against what is typed. Empty for a separator.
    label: SharedString,
    /// Whether the row is a command that is not disabled — the only kind the
    /// highlight lands on.
    enabled: bool,
    /// The row's callback.
    on_activate: Option<ActivateHandler>,
}

impl KeyRow {
    /// The keyboard's view of `entry`.
    fn of(entry: &MenuEntry) -> Self {
        Self {
            label: entry.label.clone(),
            enabled: !entry.separator && !entry.disabled,
            on_activate: entry.on_activate.clone(),
        }
    }
}

/// What the interceptor of one open menu acts on, as of the last frame.
struct KeyTarget {
    /// The rows, rebuilt with the menu on every frame.
    rows: Vec<KeyRow>,
    /// The handler that closes the menu; taken when a key closes it, so that a
    /// second key before the host has redrawn reaches the surface underneath.
    on_dismiss: Option<DismissHandler>,
}

/// The keyboard of one open menu.
///
/// Held in the menu's element state, so it lives exactly as long as the menu is
/// drawn in consecutive frames: a menu the host stops rendering drops the
/// interceptor with it, and nothing has to be unregistered by hand. Opening the
/// menu again starts from a fresh one, with nothing highlighted and nothing
/// typed.
struct MenuKeys {
    /// The rows and the dismiss handler of the menu as last drawn, the
    /// highlighted row — an index into the entries, drawn by the panel and
    /// moved by the keys and by the pointer — and the panel's scroll, so that
    /// a row the keys move to is brought into view.
    handles: KeyHandles,
    _interceptor: Subscription,
}

/// What a drawn panel needs of its keyboard.
#[derive(Clone)]
struct PanelKeys {
    highlight: Rc<Cell<Option<usize>>>,
    scroll: ScrollHandle,
}

/// Whether `keystroke` carries a command modifier. Shift is allowed, the way a
/// menu row's click allows it; a chord with any other modifier is somebody
/// else's shortcut and passes through the open menu untouched.
fn has_command_modifier(keystroke: &Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.control || modifiers.alt || modifiers.platform || modifiers.function
}

/// The first enabled row at or after `from` going `forward`, wrapping round.
fn next_enabled(rows: &[KeyRow], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = rows.len();
    if count == 0 {
        return None;
    }
    (1..=count)
        .map(|step| match (from, forward) {
            (Some(at), true) => (at + step) % count,
            (Some(at), false) => (at + count - step) % count,
            (None, true) => step - 1,
            (None, false) => count - step,
        })
        .find(|&index| rows[index].enabled)
}

/// The parts of [`MenuKeys`] the interceptor answers a key with.
#[derive(Clone)]
struct KeyHandles {
    target: Rc<RefCell<KeyTarget>>,
    highlight: Rc<Cell<Option<usize>>>,
    scroll: ScrollHandle,
}

impl KeyHandles {
    /// Answers one keystroke, or returns `false` to let it through.
    fn answer(
        &self,
        keystroke: &Keystroke,
        search: &mut Typeahead,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if has_command_modifier(keystroke) {
            return false;
        }
        let key = keystroke.key.as_str();
        match key {
            "escape" | "tab" => {
                // Out of the cell before the call: the handler may well redraw.
                let Some(dismiss) = self.target.borrow_mut().on_dismiss.take() else {
                    return false;
                };
                dismiss(window, cx);
            }
            "up" | "down" | "home" | "end" => {
                search.clear();
                let next = {
                    let target = self.target.borrow();
                    let rows = &target.rows;
                    match key {
                        "up" => next_enabled(rows, self.highlight.get(), false),
                        "down" => next_enabled(rows, self.highlight.get(), true),
                        "home" => next_enabled(rows, None, true),
                        _ => next_enabled(rows, None, false),
                    }
                };
                if next.is_some() {
                    self.move_to(next, window);
                }
            }
            "enter" | "space" => {
                search.clear();
                let row = self.highlight.get().and_then(|index| {
                    let target = self.target.borrow();
                    let row = target.rows.get(index)?;
                    row.enabled.then(|| row.on_activate.clone())
                });
                // Nothing highlighted is still the menu's key: an `Enter`
                // that reached the terminal under an open menu would run
                // something the user cannot see.
                if let Some(activate) = row {
                    let dismiss = self.target.borrow_mut().on_dismiss.take();
                    if let Some(activate) = activate {
                        activate(window, cx);
                    }
                    if let Some(dismiss) = dismiss {
                        dismiss(window, cx);
                    }
                }
            }
            _ => {
                let Some(text) = keystroke
                    .key_char
                    .as_deref()
                    .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
                else {
                    return false;
                };
                let found = {
                    let target = self.target.borrow();
                    let rows = &target.rows;
                    search.find(
                        text,
                        self.highlight.get(),
                        rows.len(),
                        |index| {
                            let row = &rows[index];
                            if row.enabled { row.label.as_ref() } else { "" }
                        },
                        Instant::now(),
                    )
                };
                if found.is_some() {
                    self.move_to(found, window);
                }
            }
        }
        true
    }

    /// Highlights `index`, scrolled into view.
    fn move_to(&self, index: Option<usize>, window: &mut Window) {
        self.highlight.set(index);
        if let Some(index) = index {
            self.scroll.scroll_to_item(index);
        }
        window.refresh();
    }
}

/// Gives the open menu `id` its keyboard for as long as it is drawn.
///
/// A keystroke interceptor rather than key bindings, and the focus is left
/// where it was: the editor, grid or terminal the menu was opened over keeps it
/// — so a field's edit is not committed by a blur, and a command the menu runs
/// acts on the surface it was opened for — and the menu takes the keys ahead of
/// that surface's own bindings instead. `Up`, `Down`, `Home` and `End` move the
/// highlight over the rows that can be run, `Enter` and `Space` run the
/// highlighted one, a typed letter moves to the next row starting with it, and
/// `Escape` and `Tab` close the menu. Any chord with a command modifier passes
/// through, so the application's shortcuts still work with a menu open. Only
/// keys pressed in the menu's own window count.
///
/// `highlight_first` starts the menu on its first row that can be run, which is
/// what a menu opened from the keyboard wants; one opened by the pointer starts
/// with nothing highlighted until a key or the pointer moves.
fn menu_keys(
    id: ElementId,
    entries: &[MenuEntry],
    on_dismiss: Option<DismissHandler>,
    highlight_first: bool,
    window: &mut Window,
    cx: &mut App,
) -> PanelKeys {
    let rows: Vec<KeyRow> = entries.iter().map(KeyRow::of).collect();
    let first = highlight_first
        .then(|| next_enabled(&rows, None, true))
        .flatten();
    let state = window.use_keyed_state((id, "keys"), cx, |window, cx| {
        let handles = KeyHandles {
            target: Rc::new(RefCell::new(KeyTarget {
                rows: Vec::new(),
                on_dismiss: None,
            })),
            highlight: Rc::new(Cell::new(first)),
            scroll: ScrollHandle::new(),
        };
        let owner = window.window_handle();
        let mut search = Typeahead::default();
        let interceptor = {
            let handles = handles.clone();
            cx.intercept_keystrokes(move |event, window, cx| {
                if window.window_handle() == owner
                    && handles.answer(&event.keystroke, &mut search, window, cx)
                {
                    cx.stop_propagation();
                }
            })
        };
        MenuKeys {
            handles,
            _interceptor: interceptor,
        }
    });
    let handles = &state.read(cx).handles;
    *handles.target.borrow_mut() = KeyTarget { rows, on_dismiss };
    PanelKeys {
        highlight: handles.highlight.clone(),
        scroll: handles.scroll.clone(),
    }
}

/// Builds the full-window sheet that sits under an open menu.
///
/// A pointer press anywhere it can see dismisses the menu — either mouse button,
/// so that a right-click outside is not swallowed without effect. The panel is
/// drawn above it and occludes it, so presses on a row never reach here.
///
/// Callers wrap this in `anchored`, whose positions are window-relative by
/// default, so the sheet covers the window rather than the caller's own box.
fn menu_backdrop(
    id: ElementId,
    viewport: Size<Pixels>,
    on_dismiss: Option<DismissHandler>,
) -> AnyElement {
    div()
        .id(id)
        .w(viewport.width)
        .h(viewport.height)
        .occlude()
        .when_some(on_dismiss, |this, dismiss| {
            this.on_any_mouse_down(move |_, window, cx| dismiss(window, cx))
        })
        .into_any_element()
}

/// Builds the floating panel listing `entries`.
///
/// Opaque on purpose: a translucent window allows only one tinted fill per
/// pixel, and the terminal surface underneath already owns it.
///
/// `max_height` caps how tall the panel may grow. A list longer than that — a
/// menu of every syntax the application knows, which is as long as the user's
/// own grammars make it — scrolls under the wheel instead of running off the
/// bottom of the window, where snapping the panel back inside would only trade
/// unreachable rows at the end for unreachable rows at the start.
fn menu_panel(
    id: ElementId,
    entries: Vec<MenuEntry>,
    width: PanelWidth,
    max_height: Pixels,
    on_dismiss: Option<DismissHandler>,
    keys: PanelKeys,
    cx: &App,
) -> AnyElement {
    let theme = &theme(cx);
    // The check column is decided for the menu, not for the row: a mark that
    // only indented its own row would leave the labels stepping sideways down
    // the list.
    let checkable = entries.iter().any(|entry| entry.checked);
    // The same, for the color column.
    let swatched = entries.iter().any(|entry| entry.swatch.is_some());
    // Only a panel that may run out of room truncates. A fixed-width one was
    // measured for its own rows, and an ellipsis there would be a regression
    // rather than a fallback.
    let truncates = matches!(width, PanelWidth::Content { .. });

    let row_theme = theme.clone();
    let highlighted = keys.highlight.get();
    let row_height = scaled(ROW_HEIGHT, cx);
    let check_width = scaled(CHECK_WIDTH, cx);
    let swatch_size = scaled(SWATCH_SIZE, cx);
    let row_text = TextSize::Body.size(cx);
    let shortcut_text = TextSize::Xs.size(cx);
    let highlight = keys.highlight.clone();
    let rows = entries.into_iter().enumerate().map(move |(index, entry)| {
        let theme = &row_theme;
        if entry.separator {
            return div()
                .id(ElementId::from(("menu-separator", index)))
                .flex_none()
                .h(px(1.))
                .my(px(4.))
                .mx(px(6.))
                .bg(theme.border);
        }

        let on_dismiss = on_dismiss.clone();
        let MenuEntry {
            label,
            shortcut,
            on_activate,
            disabled,
            checked,
            swatch,
            ..
        } = entry;
        let swatch = swatch.map(|color| color_swatch(color, theme, swatch_size));

        let highlight = highlight.clone();
        div()
            .id(ElementId::from(("menu-entry", index)))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .h(row_height)
            .px(px(10.))
            .mx(px(4.))
            .rounded_sm()
            .text_size(row_text)
            .text_color(if disabled {
                theme.text_muted
            } else {
                theme.text
            })
            // The keyboard's row wears the pointer's hover, so the two never
            // disagree about which row `Enter` would run.
            .when(!disabled && highlighted == Some(index), |this| {
                this.bg(theme.surface_hover)
            })
            // Everything that makes a row look and behave like a control hangs
            // off this one condition, so a disabled row is inert by having no
            // handler rather than by having one that thinks better of it.
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(theme.surface_hover))
                    // The pointer moves the highlight too, so the keys carry on
                    // from the row it rests on.
                    .on_mouse_move(move |_, window, _| {
                        if highlight.get() != Some(index) {
                            highlight.set(Some(index));
                            window.refresh();
                        }
                    })
                    .on_click(move |_, window, cx| {
                        if let Some(activate) = on_activate.clone() {
                            activate(window, cx);
                        }
                        if let Some(dismiss) = on_dismiss.clone() {
                            dismiss(window, cx);
                        }
                    })
            })
            .when(checkable, |this| {
                this.child(
                    div()
                        .flex_none()
                        .w(check_width)
                        .whitespace_nowrap()
                        .child(if checked { CHECK_MARK } else { "" }),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .gap(px(SWATCH_GAP))
                    // Inside the label's box rather than a column of the row:
                    // the row's gap is sized for the space before a shortcut,
                    // and a square that far from its name would read as
                    // belonging to neither.
                    .when(swatched, |this| {
                        this.child(div().flex_none().w(swatch_size).children(swatch))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .when(truncates, |this| this.text_ellipsis())
                            .child(label),
                    ),
            )
            .children(shortcut.map(|shortcut| {
                div()
                    .flex_none()
                    .text_size(shortcut_text)
                    .text_color(theme.text_muted)
                    .whitespace_nowrap()
                    .child(shortcut)
            }))
    });

    let panel = div()
        .id(id)
        .occlude()
        .flex()
        .flex_col()
        .flex_none()
        .max_h(max_height)
        .py(px(4.))
        // The panel carries an id, which is what gpui needs to keep a scroll
        // offset for it between frames; the rows stay `flex_none` so that they
        // scroll past rather than being squeezed to fit. The lock to one axis
        // keeps a horizontal wheel — or a trackpad's stray sideways component —
        // from sliding a panel that has nothing to show sideways.
        .overflow_y_scroll()
        .track_scroll(&keys.scroll)
        .restrict_scroll_to_axis()
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .rounded_lg()
        .shadow_lg()
        .text_color(theme.text)
        .debug_selector(|| PANEL_SELECTOR.to_string());

    match width {
        PanelWidth::Fixed(width) => panel.w(width),
        // Wide enough for the same words at any UI scale.
        PanelWidth::Content { min, max } => panel.min_w(scaled(min, cx)).max_w(scaled(max, cx)),
    }
    .children(rows)
    .into_any_element()
}

/// A menu opened at a point of the caller's choosing, with no trigger of its
/// own.
///
/// Rendered by the view that owns the pointer position — typically from an
/// `on_mouse_down(MouseButton::Right, …)` handler that stored the event's
/// window-space position. The element takes no space in its parent's layout, so
/// it can be dropped in anywhere the view already renders:
///
/// ```ignore
/// ContextMenu::new("tab-context")
///     .position(position)
///     .entries(vec![
///         MenuEntry::new("Close tab"),
///         MenuEntry::new("Close others").disabled(only_tab),
///     ])
///     .on_dismiss(|_window, cx| { /* clear the stored position */ })
/// ```
///
/// The panel is as wide as its widest row, held between a floor and a ceiling,
/// because the commands a right-click offers depend on what was under the
/// pointer: two surfaces of the same window can want very differently sized
/// menus, and neither of them is a fixed trigger's own. A caller who already
/// knows the width it wants says so with [`ContextMenu::width`].
#[derive(IntoElement)]
pub struct ContextMenu {
    id: ElementId,
    position: Point<Pixels>,
    anchor: Anchor,
    width: Option<Pixels>,
    entries: Vec<MenuEntry>,
    on_dismiss: Option<DismissHandler>,
    highlight_first: bool,
}

impl ContextMenu {
    /// Creates an empty menu anchored at the window's top-left corner.
    ///
    /// `id` must be unique among the siblings of the menu.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            position: point(px(0.), px(0.)),
            anchor: Anchor::TopLeft,
            width: None,
            entries: Vec::new(),
            on_dismiss: None,
            highlight_first: false,
        }
    }

    /// Opens with the first row that can be run already highlighted.
    ///
    /// For a menu opened from the keyboard — [`OpenContextMenu`], which
    /// Shift+F10 and the Menu key send — where the next key is `Down` or
    /// `Enter` and a menu with nothing highlighted would make the first of
    /// them do nothing visible. A menu opened by the pointer leaves it off.
    pub fn highlight_first(mut self, highlight_first: bool) -> Self {
        self.highlight_first = highlight_first;
        self
    }

    /// Puts the panel's [`ContextMenu::anchor`] corner at `position`, in window
    /// coordinates.
    ///
    /// A panel that would hang off an edge is pulled back inside the window
    /// instead.
    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }

    /// Chooses which corner of the panel sits at the position, and so which way
    /// the menu grows from it.
    ///
    /// [`Anchor::TopLeft`] by default, which is what a right-click wants: the
    /// list hangs down and to the right of the pointer, away from it. A trigger
    /// along the bottom of the window — a status bar's file-format or charset
    /// picker — wants [`Anchor::BottomLeft`] instead, so that the list stands
    /// *on* the trigger and opens upward into the window rather than being
    /// snapped back over the thing it was opened from.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Draws the panel at `width` instead of measuring it.
    ///
    /// Without this the panel follows its widest row, between a floor and a
    /// ceiling, which is right for a menu assembled for one right-click. It is
    /// wrong for a menu the same trigger opens again and again over rows that
    /// come and go — a list of encodings, of syntaxes — where a width that
    /// followed the content would make the panel breathe under a trigger that
    /// never moves. Such a caller measured its own menu once and says so here.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the rows of the menu, in display order.
    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
        self
    }

    /// Called when the menu should go away: after a row is activated, when the
    /// pointer goes down outside the panel, or on `Escape`.
    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let viewport = window.viewport_size();
        let keys = menu_keys(
            self.id.clone(),
            &self.entries,
            self.on_dismiss.clone(),
            self.highlight_first,
            window,
            cx,
        );
        let backdrop = menu_backdrop(
            ElementId::from((self.id.clone(), "backdrop")),
            viewport,
            self.on_dismiss.clone(),
        );
        let width = match self.width {
            Some(width) => PanelWidth::Fixed(width),
            None => PanelWidth::Content {
                min: PANEL_MIN_WIDTH,
                max: PANEL_MAX_WIDTH,
            },
        };
        let panel = menu_panel(
            ElementId::from((self.id.clone(), "panel")),
            self.entries,
            width,
            panel_max_height(window),
            self.on_dismiss,
            keys,
            cx,
        );

        // Absolutely positioned and zero-sized: both children are `anchored` in
        // window coordinates, so this box only has to stay out of the way of the
        // layout it is dropped into.
        div()
            .id(self.id)
            .absolute()
            .w(px(0.))
            .h(px(0.))
            .child(
                deferred(anchored().position(point(px(0.), px(0.))).child(backdrop))
                    .with_priority(BACKDROP_PRIORITY),
            )
            .child(
                deferred(
                    anchored()
                        .anchor(self.anchor)
                        .position(self.position)
                        .snap_to_window_with_margin(px(WINDOW_MARGIN))
                        .child(panel),
                )
                .with_priority(PANEL_PRIORITY),
            )
    }
}

/// A toolbar button with a dropdown menu.
///
/// ```ignore
/// MenuButton::new("app-menu")
///     .open(self.menu_open)
///     .entries(vec![MenuEntry::new("New session").shortcut("Ctrl+T")])
///     .on_open_change(|open, _window, cx| { /* store `open` */ })
/// ```
#[derive(IntoElement)]
pub struct MenuButton {
    id: ElementId,
    glyph: Option<SharedString>,
    icon: Option<SharedString>,
    tooltip: Option<SharedString>,
    open: bool,
    entries: Vec<MenuEntry>,
    on_open_change: Option<OpenChangeHandler>,
    highlight_first: bool,
    tab_index: Option<isize>,
}

impl MenuButton {
    /// Creates a closed menu button showing the default hamburger icon.
    ///
    /// `id` must be unique among the siblings of the button.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            glyph: None,
            icon: None,
            tooltip: None,
            open: false,
            entries: Vec::new(),
            on_open_change: None,
            highlight_first: false,
            tab_index: None,
        }
    }

    /// Opens with the first row that can be run already highlighted, for a
    /// menu the host opened from the keyboard — [`OpenAppMenu`], which F10
    /// sends. A trigger opened with `Enter`, `Space` or `Down` does this by
    /// itself.
    pub fn highlight_first(mut self, highlight_first: bool) -> Self {
        self.highlight_first = highlight_first;
        self
    }

    /// Places the trigger at `index` in the window's tab order.
    ///
    /// Off by default, which keeps a toolbar's tab order what it was. A focused
    /// trigger draws an accent outline and opens its menu with `Enter`, `Space`
    /// or `Down`, on the first row.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    /// Draws `glyph` as text on the trigger instead of the default hamburger.
    ///
    /// For a caller that already owns a character suited to the button — the
    /// tab strip's overflow menu wears the same `▾` its dropdowns do — rather
    /// than reaching for an asset path. Overridden by
    /// [`MenuButton::icon`] if that is also called.
    pub fn glyph(mut self, glyph: impl Into<SharedString>) -> Self {
        self.glyph = Some(glyph.into());
        self
    }

    /// Draws the asset at `path` on the trigger instead of the glyph or the
    /// default hamburger.
    ///
    /// A second way to dress the same button rather than a replacement for
    /// [`MenuButton::glyph`], because the two triggers in the application want
    /// different things: a chevron drawn as text lands at whatever size and
    /// baseline the font feels like, which is exactly the wobble an icon
    /// exists to avoid. Callers hand over the path rather than an element, so
    /// this module keeps knowing nothing about the icon set beyond its own
    /// default.
    pub fn icon(mut self, path: impl Into<SharedString>) -> Self {
        self.icon = Some(path.into());
        self
    }

    /// Sets the label shown when the pointer rests on the trigger.
    ///
    /// Taken as text rather than looked up here: this layer holds no strings of
    /// its own, so the localized sentence comes from the view that builds the
    /// button.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Sets whether the dropdown is currently shown.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sets the rows of the dropdown, in display order.
    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
        self
    }

    /// Called with the open state the menu would like to be in.
    ///
    /// Fires with `true` when the trigger is clicked while closed, and with
    /// `false` when the trigger is clicked again, when a row is activated, or
    /// when the pointer goes down anywhere outside the panel.
    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MenuButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let viewport = window.viewport_size();
        let open = self.open;
        let on_open_change = self.on_open_change;

        let on_dismiss: Option<DismissHandler> = on_open_change.clone().map(|handler| {
            Rc::new(move |window: &mut Window, cx: &mut App| handler(false, window, cx))
                as DismissHandler
        });
        // Whether the trigger opened the menu from the keyboard, which starts
        // it on its first row. Kept for as long as the trigger is drawn, and
        // forgotten as the menu closes.
        let by_keyboard = window
            .use_keyed_state((self.id.clone(), "trigger"), cx, |_, _| {
                Rc::new(Cell::new(false))
            })
            .read(cx)
            .clone();
        if !open {
            by_keyboard.set(false);
        }
        // Only while the dropdown is out: a closed button leaves the keys alone.
        let keys = open.then(|| {
            menu_keys(
                self.id.clone(),
                &self.entries,
                on_dismiss.clone(),
                self.highlight_first || by_keyboard.get(),
                window,
                cx,
            )
        });

        // A trigger only ever wears a mark — an icon, or a glyph standing in
        // for one — never a word, so its resting color is the theme's icon
        // tint rather than the muted text a label would take.
        let tint = if open { theme.text } else { theme.icon };
        let hover_tint = theme.text;
        // An SVG takes its color from its own `text_color`, which — unlike a
        // glyph's — does not inherit from the button, so the open and hover
        // shades have to be handed to it directly. `icon` wins over `glyph` if
        // a caller sets both, and a caller who sets neither gets the default
        // hamburger icon rather than the bare glyph [`MenuButton::new`] used
        // to draw: see [`crate::icons::MENU`] for why that default is an
        // asset and not the `☰` character it used to be.
        let svg_face = |path: SharedString| {
            svg()
                .size(scaled(TRIGGER_ICON, cx))
                .debug_selector(|| "menu-trigger-icon".to_string())
                .flex_none()
                .path(path)
                .text_color(tint)
                .group_hover(TRIGGER_GROUP, move |style| style.text_color(hover_tint))
                .into_any_element()
        };
        let face = match (self.icon.clone(), self.glyph.clone()) {
            (Some(path), _) => svg_face(path),
            (None, Some(glyph)) => glyph.into_any_element(),
            (None, None) => svg_face(SharedString::new_static(crate::icons::MENU)),
        };

        let trigger = div()
            .id(ElementId::from((self.id.clone(), "trigger")))
            // The trigger may sit inside a window drag area — the toolbar
            // doubles as the title bar in the custom style — and occluding is
            // what keeps a click on it from being read as "move the window".
            .occlude()
            .group(TRIGGER_GROUP)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(scaled(TRIGGER_SIZE, cx))
            .debug_selector(|| "menu-trigger".to_string())
            .rounded_md()
            .text_size(TextSize::Title.size(cx))
            .text_color(tint)
            .when_some(self.tab_index, |this, index| {
                let accent = theme.accent;
                this.tab_index(index)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus(move |style| style.border_color(accent))
            })
            .bg(if open {
                theme.surface_active
            } else {
                gpui::transparent_black()
            })
            .cursor_pointer()
            .hover(|style| style.bg(theme.surface_hover).text_color(theme.text))
            .when_some(self.tooltip.clone(), |this, tooltip| {
                this.tooltip(tooltip_label(tooltip))
            })
            .when_some(on_open_change.clone(), |this, handler| {
                let by_click = by_keyboard.clone();
                let down = handler.clone();
                let by_down = by_keyboard.clone();
                this.on_click(move |event, window, cx| {
                    by_click.set(!open && event.is_keyboard());
                    handler(!open, window, cx)
                })
                // `Down` on a focused, closed trigger opens it too, as it
                // does a native menu button; `Enter` and `Space` are the click.
                .on_key_down(move |event, window, cx| {
                    let stroke = &event.keystroke;
                    if !open && stroke.key == "down" && !stroke.modifiers.modified() {
                        by_down.set(true);
                        down(true, window, cx);
                        cx.stop_propagation();
                    }
                })
            })
            .child(face);

        // A full-window sheet under the panel, deferred so that it covers the
        // whole window rather than just the toolbar row this button sits in.
        let backdrop = menu_backdrop(
            ElementId::from((self.id.clone(), "backdrop")),
            viewport,
            on_dismiss.clone(),
        );

        let panel = menu_panel(
            ElementId::from((self.id.clone(), "panel")),
            self.entries,
            PanelWidth::Fixed(scaled(PANEL_WIDTH, cx)),
            panel_max_height(window),
            on_dismiss,
            keys.unwrap_or_else(|| PanelKeys {
                highlight: Rc::default(),
                scroll: ScrollHandle::new(),
            }),
            cx,
        );

        // The dropdown hangs off a zero-width box laid out *before* the
        // trigger, not off the trigger itself. An `anchored` element is
        // absolutely positioned, and an absolutely positioned box is aligned by
        // its parent's `align-items`; hanging it directly in the `items_center`
        // row would center the whole panel on the 28px button instead of
        // starting it at the button's top-left corner. This box neither centers
        // its children nor takes up space, so the panel starts exactly one
        // trigger's height and a [`DROP_GAP`] below its top-left corner.
        let overlays = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(0.))
            .h(scaled(TRIGGER_SIZE, cx))
            .child(
                deferred(anchored().position(point(px(0.), px(0.))).child(backdrop))
                    .with_priority(BACKDROP_PRIORITY),
            )
            .child(
                deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .offset(point(px(0.), scaled(TRIGGER_SIZE, cx) + px(DROP_GAP)))
                        .snap_to_window_with_margin(px(WINDOW_MARGIN))
                        .child(panel),
                )
                .with_priority(PANEL_PRIORITY),
            );

        div()
            .id(self.id)
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .children(open.then_some(overlays))
            .child(trigger)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ops::Deref;

    use gpui::{
        Context, FocusHandle, KeyBinding, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
        Render, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext,
        actions,
    };

    use super::*;

    /// Where the harness anchors the context menu: far enough from every edge
    /// that nothing is snapped back inside, so the row arithmetic below holds.
    const MENU_X: f32 = 100.;

    /// Top of the context menu panel, for the same reason.
    const MENU_Y: f32 = 50.;

    /// Height of one command row, as `menu_panel` lays it out.
    const ROW_HEIGHT: f32 = 28.;

    /// What the panel puts above its first row: its border and its padding.
    const PANEL_TOP: f32 = 5.;

    /// Gap between the cells of a row, as `menu_panel` sets it.
    const ROW_GAP: f32 = 16.;

    /// A column inside every panel these tests draw, past the check column.
    const INSIDE_THE_PANEL: f32 = MENU_X + 60.;

    /// A point no panel covers.
    const OUTSIDE: f32 = 10.;

    /// A label short enough that the panel would rather be at its floor.
    const SHORT: &str = "Cut";

    /// A label long enough to push a panel past its floor and leave room for a
    /// check column before it reaches the ceiling.
    const MEDIUM: &str = "Copy as INSERT statement";

    /// A label no panel can hold, so the ceiling decides instead.
    const LONG: &str = "Copy the fully qualified name of every selected column";

    /// More rows than any test display is tall, so that the panel's own cap is
    /// what decides its height rather than the length of the list.
    const MANY_ROWS: usize = 200;

    /// One row of the menu a test asks for.
    ///
    /// [`MenuEntry`] owns callbacks and cannot be cloned, so the harness keeps
    /// the description of a row and builds the entry again on every draw, the
    /// way a real view rebuilds its menu from its own state.
    #[derive(Clone)]
    struct Row {
        label: SharedString,
        disabled: bool,
        checked: bool,
        swatched: bool,
    }

    impl Row {
        /// An ordinary command row.
        fn new(label: &'static str) -> Self {
            Self {
                label: SharedString::new_static(label),
                disabled: false,
                checked: false,
                swatched: false,
            }
        }

        /// The same row, led by a color square.
        fn swatched(mut self) -> Self {
            self.swatched = true;
            self
        }

        /// The same row, grayed out.
        fn disabled(mut self) -> Self {
            self.disabled = true;
            self
        }

        /// The same row, marked as the one in effect.
        fn checked(mut self) -> Self {
            self.checked = true;
            self
        }
    }

    /// Which of the two menus a test is looking at. Both draw the same panel.
    #[derive(Clone, Copy, PartialEq)]
    enum Surface {
        /// A [`ContextMenu`] at a fixed point, always open.
        Context,
        /// A [`MenuButton`] whose dropdown is held open.
        Button,
    }

    /// A view holding one open menu, as the surface that owns a right-click
    /// would.
    struct Harness {
        surface: Surface,
        rows: Vec<Row>,
        position: Point<Pixels>,
        anchor: Anchor,
        width: Option<Pixels>,
        activated: Rc<RefCell<Vec<usize>>>,
        dismissed: Rc<Cell<usize>>,
    }

    impl Harness {
        /// The entries as the menu wants them, wired to the tallies.
        fn entries(&self) -> Vec<MenuEntry> {
            self.rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let activated = self.activated.clone();
                    let entry = MenuEntry::new(row.label.clone())
                        .disabled(row.disabled)
                        .checked(row.checked)
                        .on_activate(move |_, _| activated.borrow_mut().push(index));
                    if row.swatched {
                        entry.swatch(gpui::red())
                    } else {
                        entry
                    }
                })
                .collect()
        }
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let dismissed = self.dismissed.clone();
            let entries = self.entries();

            div().size_full().child(match self.surface {
                Surface::Context => ContextMenu::new("menu")
                    .position(self.position)
                    .anchor(self.anchor)
                    .when_some(self.width, ContextMenu::width)
                    .entries(entries)
                    .on_dismiss(move |_, _| dismissed.set(dismissed.get() + 1))
                    .into_any_element(),
                Surface::Button => MenuButton::new("menu")
                    .open(true)
                    .entries(entries)
                    .on_open_change(move |open, _, _| {
                        if !open {
                            dismissed.set(dismissed.get() + 1);
                        }
                    })
                    .into_any_element(),
            })
        }
    }

    /// What a test reads back out of a running harness.
    struct Handles {
        activated: Rc<RefCell<Vec<usize>>>,
        dismissed: Rc<Cell<usize>>,
    }

    impl Handles {
        /// The rows run since the last look.
        fn drain(&self) -> Vec<usize> {
            self.activated.borrow_mut().drain(..).collect()
        }

        /// How many times the menu has asked to close.
        fn dismissals(&self) -> usize {
            self.dismissed.get()
        }
    }

    /// Opens a window on `surface` showing `rows`, hanging down and to the
    /// right of the usual corner at its own measured width.
    fn open(
        surface: Surface,
        rows: Vec<Row>,
        cx: &mut TestAppContext,
    ) -> (Handles, VisualTestContext) {
        open_placed(
            surface,
            rows,
            point(px(MENU_X), px(MENU_Y)),
            Anchor::TopLeft,
            None,
            cx,
        )
    }

    /// The same, with the panel's `anchor` corner at `position` and, when the
    /// test names one, a `width` of its own.
    fn open_placed(
        surface: Surface,
        rows: Vec<Row>,
        position: Point<Pixels>,
        anchor: Anchor,
        width: Option<Pixels>,
        cx: &mut TestAppContext,
    ) -> (Handles, VisualTestContext) {
        cx.update(crate::init);

        let handles = Handles {
            activated: Rc::new(RefCell::new(Vec::new())),
            dismissed: Rc::new(Cell::new(0)),
        };
        let window = cx.add_window({
            let activated = handles.activated.clone();
            let dismissed = handles.dismissed.clone();
            move |_, _| Harness {
                surface,
                rows,
                position,
                anchor,
                width,
                activated,
                dismissed,
            }
        });
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();

        (handles, cx)
    }

    /// The middle of row `index` of the context menu, on the label.
    fn row_middle(index: usize) -> Point<Pixels> {
        point(
            px(INSIDE_THE_PANEL),
            px(MENU_Y + PANEL_TOP + ROW_HEIGHT * index as f32 + ROW_HEIGHT / 2.),
        )
    }

    /// Presses and releases the left button over `position`.
    fn click(cx: &mut VisualTestContext, position: Point<Pixels>) {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        cx.run_until_parked();
    }

    /// Rolls the wheel over `position` by `delta` pixels, negative being the
    /// direction that brings later rows into view.
    fn scroll(cx: &mut VisualTestContext, position: Point<Pixels>, delta: f32) {
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        cx.run_until_parked();
    }

    /// How wide the panel came out.
    fn panel_width(cx: &mut VisualTestContext) -> f32 {
        cx.debug_bounds(PANEL_SELECTOR)
            .expect("the panel is drawn")
            .size
            .width
            .into()
    }

    /// How tall the panel came out.
    fn panel_height(cx: &mut VisualTestContext) -> f32 {
        cx.debug_bounds(PANEL_SELECTOR)
            .expect("the panel is drawn")
            .size
            .height
            .into()
    }

    /// The panel's bottom edge, in window coordinates.
    fn panel_bottom(cx: &mut VisualTestContext) -> f32 {
        cx.debug_bounds(PANEL_SELECTOR)
            .expect("the panel is drawn")
            .bottom()
            .into()
    }

    /// Gives the window the transparent shadow band a client-decorated one
    /// carries, and redraws it.
    fn set_client_inset(cx: &mut VisualTestContext, inset: f32) {
        cx.update(|window, _| {
            window.set_client_inset(px(inset));
            window.refresh();
        });
        cx.run_until_parked();
    }

    /// `Escape` closes either menu without a click, and without the focus
    /// having to be on it — the harness never focuses anything.
    #[gpui::test]
    fn escape_dismisses_either_menu(cx: &mut TestAppContext) {
        for surface in [Surface::Context, Surface::Button] {
            let (menu, mut cx) = open(surface, vec![Row::new(SHORT)], cx);
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            assert_eq!(menu.drain(), Vec::<usize>::new(), "no row runs");
            assert_eq!(menu.dismissals(), 1);

            // A chord is not `Escape`.
            cx.simulate_keystrokes("ctrl-escape");
            cx.run_until_parked();
            assert_eq!(menu.dismissals(), 1);
        }
    }

    actions!(menu_escape_test, [HostEscape]);

    /// A focused surface with an `Escape` binding of its own, and a context
    /// menu over it that closes for real.
    struct Host {
        focus: FocusHandle,
        menu_open: bool,
        host_escapes: usize,
    }

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            div()
                .size_full()
                .key_context("MenuEscapeHost")
                .track_focus(&self.focus)
                .on_action(cx.listener(|host, _: &HostEscape, _, _| host.host_escapes += 1))
                .when(self.menu_open, |this_div| {
                    this_div.child(
                        ContextMenu::new("menu")
                            .position(point(px(MENU_X), px(MENU_Y)))
                            .entries(vec![MenuEntry::new(SHORT)])
                            .on_dismiss(move |_, cx| {
                                this.update(cx, |host, cx| {
                                    host.menu_open = false;
                                    cx.notify();
                                });
                            }),
                    )
                })
        }
    }

    /// The menu is on top, so it takes the key before the surface under it —
    /// and once it has gone, the surface's own `Escape` works again.
    #[gpui::test]
    fn escape_closes_the_menu_before_the_surface_under_it(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.bind_keys([KeyBinding::new(
                "escape",
                HostEscape,
                Some("MenuEscapeHost"),
            )]);
        });
        let window = cx.add_window(|_, cx| Host {
            focus: cx.focus_handle(),
            menu_open: true,
            host_escapes: 0,
        });
        let host = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.update(|window, cx| {
            let focus = host.read(cx).focus.clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(!host.read(cx).menu_open, "the menu closed");
            assert_eq!(host.read(cx).host_escapes, 0, "and kept the key");
        });

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(host.read(cx).host_escapes, 1));
    }

    /// The two halves of what disabled means: the callback never runs, and the
    /// menu is still there afterwards — a row that did nothing *and* closed the
    /// menu would read as a command that silently failed.
    #[gpui::test]
    fn a_disabled_row_runs_nothing_and_leaves_the_menu_open(cx: &mut TestAppContext) {
        let (menu, mut cx) = open(
            Surface::Context,
            vec![Row::new(MEDIUM), Row::new(MEDIUM).disabled()],
            cx,
        );

        click(&mut cx, row_middle(1));
        assert_eq!(menu.drain(), Vec::<usize>::new());
        assert_eq!(menu.dismissals(), 0, "the menu stays where it is");

        // The row above it, which is the same in every way but enabled, still
        // runs and still closes the menu.
        click(&mut cx, row_middle(0));
        assert_eq!(menu.drain(), vec![0]);
        assert_eq!(menu.dismissals(), 1);

        // And the backdrop under the panel still dismisses, as it did before
        // any of this: only the panel swallows presses.
        click(&mut cx, point(px(OUTSIDE), px(OUTSIDE)));
        assert_eq!(menu.drain(), Vec::<usize>::new());
        assert_eq!(menu.dismissals(), 2);
    }

    /// A checked row is still a command: the mark says which one is in effect,
    /// it does not stand in for the click.
    #[gpui::test]
    fn a_checked_row_still_runs(cx: &mut TestAppContext) {
        let (menu, mut cx) = open(
            Surface::Context,
            vec![Row::new(MEDIUM).checked(), Row::new(MEDIUM)],
            cx,
        );

        click(&mut cx, row_middle(0));
        assert_eq!(menu.drain(), vec![0]);
        assert_eq!(menu.dismissals(), 1);
    }

    /// The check column is the menu's, not the row's: one checked row widens
    /// every row by the column and the gap after it, and a menu with none is
    /// laid out exactly as it was before the slot existed.
    #[gpui::test]
    fn the_check_column_is_laid_out_for_the_whole_menu(cx: &mut TestAppContext) {
        let rows = vec![Row::new(MEDIUM), Row::new(MEDIUM)];
        let (_, mut bare_cx) = open(Surface::Context, rows.clone(), cx);
        let bare = panel_width(&mut bare_cx);

        let checked = vec![rows[0].clone(), rows[1].clone().checked()];
        let (_, mut checked_cx) = open(Surface::Context, checked, cx);

        assert_eq!(
            panel_width(&mut checked_cx) - bare,
            CHECK_WIDTH + ROW_GAP,
            "the column is laid out once for the menu, on the checked row and \
             the unchecked one alike"
        );
    }

    /// The color column is the menu's too, and sits closer to its label than
    /// the row's gap: one square widens every row by the square and its own
    /// gap.
    #[gpui::test]
    fn the_swatch_column_is_laid_out_for_the_whole_menu(cx: &mut TestAppContext) {
        let rows = vec![Row::new(MEDIUM), Row::new(MEDIUM)];
        let (_, mut bare_cx) = open(Surface::Context, rows.clone(), cx);
        let bare = panel_width(&mut bare_cx);

        let swatched = vec![rows[0].clone(), rows[1].clone().swatched()];
        let (_, mut swatched_cx) = open(Surface::Context, swatched, cx);

        assert_eq!(
            panel_width(&mut swatched_cx) - bare,
            SWATCH_SIZE + SWATCH_GAP,
            "the column is laid out once for the menu, on the colored row and \
             the plain one alike"
        );
    }

    /// A context menu is measured by what is in it, because what is in it
    /// depends on what was right-clicked — with a floor and a ceiling, so that
    /// neither a two-word menu nor a sentence decides the whole layout.
    #[gpui::test]
    fn a_context_menu_follows_its_content_between_its_bounds(cx: &mut TestAppContext) {
        let (_, mut short_cx) = open(Surface::Context, vec![Row::new(SHORT)], cx);
        assert_eq!(panel_width(&mut short_cx), PANEL_MIN_WIDTH);

        let (_, mut medium_cx) = open(Surface::Context, vec![Row::new(MEDIUM)], cx);
        let medium = panel_width(&mut medium_cx);
        assert!(
            medium > PANEL_MIN_WIDTH && medium < PANEL_MAX_WIDTH,
            "a menu that fits is drawn at its own width, not at a bound: {medium}"
        );

        // The longest row decides, not the last one.
        let (_, mut mixed_cx) = open(
            Surface::Context,
            vec![Row::new(SHORT), Row::new(MEDIUM), Row::new(SHORT)],
            cx,
        );
        assert_eq!(panel_width(&mut mixed_cx), medium);

        let (_, mut long_cx) = open(Surface::Context, vec![Row::new(LONG)], cx);
        assert_eq!(panel_width(&mut long_cx), PANEL_MAX_WIDTH);
    }

    /// The button's dropdown is the same panel and takes none of that: it hangs
    /// under a trigger that never moves, and was measured for its own rows.
    #[gpui::test]
    fn a_menu_button_panel_keeps_its_fixed_width(cx: &mut TestAppContext) {
        let (_, mut short_cx) = open(Surface::Button, vec![Row::new(SHORT)], cx);
        assert_eq!(panel_width(&mut short_cx), PANEL_WIDTH);

        let (_, mut long_cx) = open(
            Surface::Button,
            vec![Row::new(LONG), Row::new(MEDIUM).checked()],
            cx,
        );
        assert_eq!(panel_width(&mut long_cx), PANEL_WIDTH);
    }

    /// A named width wins over the measurement, in both directions: narrower
    /// than the floor a measured panel would have taken, and wider than the
    /// ceiling it would have stopped at.
    #[gpui::test]
    fn a_named_width_is_the_width_the_panel_takes(cx: &mut TestAppContext) {
        let narrow = px(PANEL_MIN_WIDTH - 60.);
        let (_, mut narrow_cx) = open_placed(
            Surface::Context,
            vec![Row::new(MEDIUM)],
            point(px(MENU_X), px(MENU_Y)),
            Anchor::TopLeft,
            Some(narrow),
            cx,
        );
        assert_eq!(panel_width(&mut narrow_cx), f32::from(narrow));

        let wide = px(PANEL_MAX_WIDTH + 60.);
        let (_, mut wide_cx) = open_placed(
            Surface::Context,
            vec![Row::new(SHORT)],
            point(px(MENU_X), px(MENU_Y)),
            Anchor::TopLeft,
            Some(wide),
            cx,
        );
        assert_eq!(panel_width(&mut wide_cx), f32::from(wide));

        // And a menu that names nothing still follows its rows, as it did
        // before the setting existed.
        let (_, mut measured_cx) = open(Surface::Context, vec![Row::new(SHORT)], cx);
        assert_eq!(panel_width(&mut measured_cx), PANEL_MIN_WIDTH);
    }

    /// What a status bar's picker needs: a menu that *stands on* its position
    /// instead of hanging from it, because the position is in the last two
    /// dozen pixels of the window and a list hanging down from there would be
    /// snapped back over the bar it was opened from.
    #[gpui::test]
    fn a_bottom_anchored_menu_opens_upward_from_its_position(cx: &mut TestAppContext) {
        // Far enough down the window that a two-row panel standing on it still
        // clears the top edge, so nothing is snapped and the arithmetic holds.
        let foot = 300.;
        let (menu, mut cx) = open_placed(
            Surface::Context,
            vec![Row::new(SHORT), Row::new(MEDIUM)],
            point(px(MENU_X), px(foot)),
            Anchor::BottomLeft,
            None,
            cx,
        );

        // The last row sits directly above the position, under the panel's own
        // padding and border; the first is a row further up again.
        let last = foot - PANEL_TOP - ROW_HEIGHT / 2.;
        let first = last - ROW_HEIGHT;
        click(&mut cx, point(px(INSIDE_THE_PANEL), px(last)));
        assert_eq!(menu.drain(), vec![1]);

        click(&mut cx, point(px(INSIDE_THE_PANEL), px(first)));
        assert_eq!(menu.drain(), vec![0]);

        // And nothing of the panel hangs below the position: a press just under
        // it reaches the backdrop, which is what the bar the menu was opened
        // from would otherwise be covered by.
        click(&mut cx, point(px(INSIDE_THE_PANEL), px(foot + 4.)));
        assert_eq!(menu.drain(), Vec::<usize>::new());
        assert!(menu.dismissals() > 0);
    }

    /// What a menu of every syntax the application knows needs: a list of more
    /// rows than the window is tall stops at the window's height and scrolls,
    /// rather than running off the bottom edge where its last rows would be
    /// drawn outside the window and snapping the panel back inside would only
    /// move the problem to the top.
    #[gpui::test]
    fn a_menu_taller_than_the_window_is_capped_and_scrolls(cx: &mut TestAppContext) {
        // More rows than any test display is tall, so the cap is what decides.
        let rows: Vec<Row> = (0..MANY_ROWS).map(|_| Row::new(MEDIUM)).collect();
        let (menu, mut cx) = open(Surface::Context, rows, cx);

        let viewport = cx.update(|window, _| window.viewport_size());
        let cap = f32::from(viewport.height) - 2. * WINDOW_MARGIN;
        let height = panel_height(&mut cx);
        assert!(
            height <= cap,
            "the panel is capped at the window less both margins: {height} > {cap}"
        );
        assert!(
            height > cap - ROW_HEIGHT,
            "and it grew to the cap rather than stopping short of it: {height}"
        );

        // A capped panel, snapped back inside, starts one margin below the top
        // edge — and the wheel is what reaches the rows past the bottom of it.
        let top_of_list = point(
            px(INSIDE_THE_PANEL),
            px(WINDOW_MARGIN + PANEL_TOP + ROW_HEIGHT / 2.),
        );
        click(&mut cx, top_of_list);
        assert_eq!(
            menu.drain(),
            vec![0],
            "unscrolled, the list starts at row 0"
        );

        // Far enough to be clamped to the end of the list rather than landing
        // on a row this test would have to predict.
        scroll(&mut cx, top_of_list, -100_000.);
        click(&mut cx, top_of_list);
        let activated = menu.drain();
        assert_eq!(activated.len(), 1, "the same click still runs one row");
        assert!(
            activated[0] > 0,
            "the wheel brought a later row under the pointer, got {activated:?}"
        );
    }

    /// The band a client-decorated window paints its shadow into is part of the
    /// viewport but not of the window: the content is clipped at its inner
    /// edge. A panel whose foot lands in that band is lifted clear of it, the
    /// same way one running off the surface is, instead of being cut off.
    #[gpui::test]
    fn a_panel_ending_in_the_shadow_band_is_lifted_above_it(cx: &mut TestAppContext) {
        const INSET: f32 = 24.;
        let rows = || vec![Row::new(SHORT), Row::new(MEDIUM)];

        let (_, mut probe) = open(Surface::Context, rows(), cx);
        let viewport: f32 = probe
            .update(|window, _| window.viewport_size().height)
            .into();
        let height = panel_height(&mut probe);

        // Inside the surface, ten pixels short of its bottom edge: in the band.
        let foot = viewport - 10.;
        let (_, mut cx) = open_placed(
            Surface::Context,
            rows(),
            point(px(MENU_X), px(foot - height)),
            Anchor::TopLeft,
            None,
            cx,
        );
        assert_eq!(
            panel_bottom(&mut cx),
            foot,
            "without a band the panel fits and stays where it was put"
        );

        set_client_inset(&mut cx, INSET);
        assert_eq!(
            panel_bottom(&mut cx),
            viewport - INSET - WINDOW_MARGIN,
            "with one it stands a margin above the band"
        );
    }

    /// The cap leaves out the shadow band on both edges as well as the margins,
    /// so the tallest panel still fits inside what the window shows.
    #[gpui::test]
    fn the_height_cap_leaves_out_the_shadow_band(cx: &mut TestAppContext) {
        const INSET: f32 = 24.;
        let rows: Vec<Row> = (0..MANY_ROWS).map(|_| Row::new(MEDIUM)).collect();
        let (_, mut cx) = open(Surface::Context, rows, cx);
        set_client_inset(&mut cx, INSET);

        let viewport: f32 = cx.update(|window, _| window.viewport_size().height).into();
        let cap = viewport - 2. * (INSET + WINDOW_MARGIN);
        let height = panel_height(&mut cx);
        assert!(height <= cap, "capped inside the band: {height} > {cap}");
        assert!(height > cap - ROW_HEIGHT, "and grown to the cap: {height}");
        assert!(
            panel_bottom(&mut cx) <= viewport - INSET - WINDOW_MARGIN,
            "and its foot clears the band"
        );
    }

    /// Presses and releases `keys` one after another.
    fn press(cx: &mut VisualTestContext, keys: &str) {
        for key in keys.split_whitespace() {
            cx.simulate_keystrokes(key);
            cx.simulate_event(gpui::KeyUpEvent {
                keystroke: Keystroke::parse(key).unwrap(),
            });
            cx.run_until_parked();
        }
    }

    /// The arrows walk the rows that can be run — past a separator and a
    /// disabled row, round from the last to the first — and `Enter` runs the
    /// highlighted one and closes the menu, in both menus.
    #[gpui::test]
    fn the_arrows_walk_the_runnable_rows_and_enter_runs_one(cx: &mut TestAppContext) {
        for surface in [Surface::Context, Surface::Button] {
            let rows = vec![
                Row::new("Cut"),
                Row::new("Copy").disabled(),
                Row::new("Paste"),
                Row::new("Select all"),
            ];
            let (menu, mut cx) = open(surface, rows, cx);
            // Nothing is highlighted yet, and `Enter` runs nothing — but it is
            // still the menu's key.
            press(&mut cx, "enter");
            assert_eq!(menu.drain(), Vec::<usize>::new());
            assert_eq!(menu.dismissals(), 0);

            press(&mut cx, "down down");
            press(&mut cx, "enter");
            assert_eq!(menu.drain(), vec![2], "the disabled row was skipped");
            assert_eq!(menu.dismissals(), 1);
        }
    }

    #[gpui::test]
    fn home_end_wrap_and_letters_move_the_highlight(cx: &mut TestAppContext) {
        let rows = vec![
            Row::new("Cut"),
            Row::new("Copy"),
            Row::new("Paste"),
            Row::new("Close").disabled(),
        ];
        for (keys, expected, why) in [
            ("end enter", 2, "End lands on the last runnable row"),
            ("end down enter", 0, "Down from the last wraps to the first"),
            ("up enter", 2, "Up from nothing is the last"),
            ("p space", 2, "a letter jumps, Space runs"),
        ] {
            let (menu, mut window) = open(Surface::Context, rows.clone(), cx);
            press(&mut window, keys);
            assert_eq!(menu.drain(), vec![expected], "{why}");
        }
    }

    actions!(menu_keys_test, [HostDown, HostSave]);

    /// A focused surface with keys of its own under an open menu: the menu
    /// takes the arrows and `Enter`, a chord with a command modifier still
    /// reaches the surface, and the focus never moves.
    struct Surface2 {
        focus: FocusHandle,
        open: bool,
        downs: usize,
        saves: usize,
        ran: usize,
    }

    impl Render for Surface2 {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let ran = cx.entity();
            div()
                .size_full()
                .key_context("MenuKeysHost")
                .track_focus(&self.focus)
                .on_action(cx.listener(|host, _: &HostDown, _, _| host.downs += 1))
                .on_action(cx.listener(|host, _: &HostSave, _, _| host.saves += 1))
                .when(self.open, |this_div| {
                    this_div.child(
                        ContextMenu::new("menu")
                            .position(point(px(MENU_X), px(MENU_Y)))
                            .highlight_first(true)
                            .entries(vec![MenuEntry::new(SHORT).on_activate(move |_, cx| {
                                ran.update(cx, |host, _| host.ran += 1)
                            })])
                            .on_dismiss(move |_, cx| {
                                this.update(cx, |host, cx| {
                                    host.open = false;
                                    cx.notify();
                                });
                            }),
                    )
                })
        }
    }

    #[gpui::test]
    fn an_open_menu_holds_the_keys_but_not_the_focus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.bind_keys([
                KeyBinding::new("down", HostDown, Some("MenuKeysHost")),
                KeyBinding::new("ctrl-s", HostSave, Some("MenuKeysHost")),
            ]);
        });
        let window = cx.add_window(|_, cx| Surface2 {
            focus: cx.focus_handle(),
            open: true,
            downs: 0,
            saves: 0,
            ran: 0,
        });
        let host = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.update(|window, cx| {
            let focus = host.read(cx).focus.clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();

        press(&mut cx, "down ctrl-s");
        cx.update(|window, cx| {
            let host = host.read(cx);
            assert_eq!(host.downs, 0, "the menu kept Down");
            assert_eq!(host.saves, 1, "a shortcut went through");
            assert!(host.focus.is_focused(window), "the focus never moved");
        });
        // Highlighted from the start, so `Enter` runs the one row.
        press(&mut cx, "enter");
        cx.update(|window, cx| {
            let host = host.read(cx);
            assert_eq!(host.ran, 1);
            assert!(!host.open);
            assert!(host.focus.is_focused(window));
        });
        // Closed, the surface has its keys back.
        press(&mut cx, "down");
        cx.update(|_, cx| assert_eq!(host.read(cx).downs, 1));
    }

    /// A trigger that is a tab stop opens from the keyboard, on its first
    /// row.
    struct Trigger {
        open: bool,
        ran: Rc<RefCell<Vec<usize>>>,
        opened_by_action: usize,
    }

    impl Render for Trigger {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let entries = (0..2)
                .map(|index| {
                    let ran = self.ran.clone();
                    MenuEntry::new(MEDIUM).on_activate(move |_, _| ran.borrow_mut().push(index))
                })
                .collect();
            div()
                .size_full()
                .on_action(cx.listener(|this, _: &OpenAppMenu, _, cx| {
                    this.opened_by_action += 1;
                    this.open = true;
                    cx.notify();
                }))
                .child(
                    MenuButton::new("app-menu")
                        .tab_index(0)
                        .open(self.open)
                        .entries(entries)
                        .on_open_change(move |open, _, cx| {
                            this.update(cx, |this, cx| {
                                this.open = open;
                                cx.notify();
                            })
                        }),
                )
        }
    }

    #[gpui::test]
    fn a_focused_trigger_opens_from_the_keyboard_on_its_first_row(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let ran = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let ran = ran.clone();
            move |_, _| Trigger {
                open: false,
                ran,
                opened_by_action: 0,
            }
        });
        let trigger = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();

        for opener in ["enter", "space", "down"] {
            press(&mut cx, opener);
            assert!(
                cx.update(|_, cx| trigger.read(cx).open),
                "{opener} opens it"
            );
            press(&mut cx, "enter");
            assert_eq!(ran.borrow_mut().drain(..).collect::<Vec<_>>(), vec![0]);
            assert!(!cx.update(|_, cx| trigger.read(cx).open));
        }

        // F10 is the application's to answer, and here it opens this menu.
        press(&mut cx, "f10");
        assert_eq!(cx.update(|_, cx| trigger.read(cx).opened_by_action), 1);
    }
}
