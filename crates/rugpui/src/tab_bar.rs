//! Horizontal tab strip used to switch between SSH sessions.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Context, DragMoveEvent, ElementId, FocusHandle, Hsla, IsZero, KeyBinding, MouseButton,
    Pixels, Point, Render, ScrollHandle, SharedString, Window, actions, div, prelude::*, px, svg,
    transparent_black,
};

use crate::menu::{MenuButton, MenuEntry};
use crate::scrollbar::Scrollbar;
use crate::theme::{Theme, theme};
use crate::tooltip::tooltip_label;

/// Glyph of the button opening the tab list.
const TAB_MENU_GLYPH: &str = "\u{25be}";

/// Hover group tying the "+" button to the icon it may carry.
const NEW_GROUP: &str = "tab-bar-new";

/// Width of the insertion bar drawn while a tab is dragged over the strip.
const INDICATOR_WIDTH: f32 = 2.;

actions!(
    rugpui_tab_bar,
    [
        /// Select the tab left of the active one.
        SelectPrevTab,
        /// Select the tab right of the active one.
        SelectNextTab,
        /// Select the first tab.
        SelectFirstTab,
        /// Select the last tab.
        SelectLastTab,
        /// Close the active tab, as its close button would.
        CloseActiveTab,
        /// Move the active tab one place to the left.
        MoveTabLeft,
        /// Move the active tab one place to the right.
        MoveTabRight,
    ]
);

/// Key context that [`init`] binds the keys above to.
const KEY_CONTEXT: &str = "TabBar";

/// Registers the key bindings a focusable [`TabBar`] relies on.
///
/// Called by [`crate::init`]; scoped to the `TabBar` key context, which only a
/// bar handed a [`TabBar::focus_handle`] carries, so the arrows keep meaning
/// what they meant everywhere else in the app. The reorder chords follow the
/// platform's command key — `cmd` on macOS, `ctrl` elsewhere — the way the
/// text input's clipboard chords do. On macOS the key labelled Delete sends
/// `backspace`, so it closes the tab there too.
pub fn init(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };

    let mut bindings = vec![
        KeyBinding::new("left", SelectPrevTab, Some(KEY_CONTEXT)),
        KeyBinding::new("right", SelectNextTab, Some(KEY_CONTEXT)),
        KeyBinding::new("home", SelectFirstTab, Some(KEY_CONTEXT)),
        KeyBinding::new("end", SelectLastTab, Some(KEY_CONTEXT)),
        KeyBinding::new("delete", CloseActiveTab, Some(KEY_CONTEXT)),
        KeyBinding::new(
            &format!("{modifier}-shift-pageup"),
            MoveTabLeft,
            Some(KEY_CONTEXT),
        ),
        KeyBinding::new(
            &format!("{modifier}-shift-pagedown"),
            MoveTabRight,
            Some(KEY_CONTEXT),
        ),
    ];
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new(
            "backspace",
            CloseActiveTab,
            Some(KEY_CONTEXT),
        ));
    }

    cx.bind_keys(bindings);
}

/// Connection state rendered as a colored dot in front of a tab title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStatus {
    /// A connection attempt is in flight.
    Connecting,
    /// The session is live.
    Connected,
    /// The session ended cleanly or was never started.
    Disconnected,
    /// The session failed.
    Error,
}

impl TabStatus {
    /// The dot color for this status under `theme`.
    fn color(self, theme: &Theme) -> Hsla {
        match self {
            TabStatus::Connecting => theme.accent,
            TabStatus::Connected => theme.success,
            TabStatus::Disconnected => theme.text_muted,
            TabStatus::Error => theme.danger,
        }
    }
}

/// Size of the mark drawn after a tab's title, in pixels.
///
/// Level with the 13 px title beside it rather than with the 16 px of a
/// toolbar button: the mark is read as part of the tab's line, and one drawn
/// larger than the words would announce itself as a control to press.
const MARK_SIZE: f32 = 13.;

/// An icon drawn after a tab's title, saying something the dot cannot.
#[derive(Debug, Clone)]
pub struct TabMark {
    /// Asset path of the icon to draw.
    pub icon: SharedString,
    /// Hover label saying what the icon means.
    pub tooltip: SharedString,
}

/// One entry of a [`TabBar`].
#[derive(Debug, Clone)]
pub struct TabItem {
    /// Element id of the tab; must be unique within the bar.
    pub id: ElementId,
    /// Label shown to the user.
    pub title: SharedString,
    /// Whether this tab offers a close button when the bar has a close handler.
    pub closable: bool,
    /// Connection state dot. `None` renders no dot at all.
    pub status: Option<TabStatus>,
    /// Dot colour chosen by the owner, overriding the one [`TabItem::status`]
    /// would take from the theme.
    ///
    /// For the strips whose dot means something the four states cannot say — a
    /// pane's tabs mark which connection each of them belongs to, in the colour
    /// that connection's profile carries. `None` leaves the status in charge.
    pub dot: Option<Hsla>,
    /// Mark drawn after the title. `None` renders no mark at all, and a tab may
    /// carry one alongside a dot: they answer different questions.
    pub mark: Option<TabMark>,
}

impl TabItem {
    /// Creates a tab without a dot.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            closable: true,
            status: None,
            dot: None,
            mark: None,
        }
    }

    /// Hides the close button for a permanent tab. Tabs are closable by default.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// Attaches a status dot to the tab.
    pub fn status(mut self, status: TabStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Attaches a dot in a colour of the caller's choosing.
    ///
    /// Wins over [`TabItem::status`] when both are set, because a colour named
    /// outright is the more specific of the two.
    pub fn dot(mut self, color: Hsla) -> Self {
        self.dot = Some(color);
        self
    }

    /// Draws the asset at `icon` after the title, labelled `tooltip` on hover.
    ///
    /// Both come from the caller, icon path included, for the reason given on
    /// [`TabBar::tooltips`]: this layer carries neither text nor assets of its
    /// own. Passing them together is what makes an unlabelled mark — the one
    /// thing a mark must never be, since a symbol nobody can name is a symbol
    /// nobody can act on — impossible to ask for.
    ///
    /// Independent of [`TabItem::dot`] and [`TabItem::status`]: a tab may wear
    /// a dot in front of its title and a mark behind it at the same time.
    pub fn mark(mut self, icon: impl Into<SharedString>, tooltip: impl Into<SharedString>) -> Self {
        self.mark = Some(TabMark {
            icon: icon.into(),
            tooltip: tooltip.into(),
        });
        self
    }
}

/// Callback receiving a drag hover: what is dragged, and where it would land.
///
/// The index is the insertion point the pointer is over — the gap before the
/// tab under its left half, after it under its right half, or the strip's
/// length over the empty stretch — or `None` once the pointer has left the
/// strip. The owner keeps the latest one as its drop indicator.
type DragOverHandler = Rc<dyn Fn(DraggedTab, Option<usize>, &mut Window, &mut App)>;

/// Callback receiving the index of the tab that was acted upon.
type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Callback receiving the index of the tab that was right-clicked, along with
/// the window-space position of the pointer.
type ContextHandler = Rc<dyn Fn(usize, Point<Pixels>, &mut Window, &mut App)>;

/// Callback for the "new tab" button.
type PlainHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Callback fired when the tab dropdown wants to open or close itself.
type OpenChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// Callback receiving a tab drop: what was dragged, and where it landed.
///
/// The index is the insertion point the drop lands in, counted the way
/// [`DragOverHandler`] counts it — the gap before the tab under the pointer's
/// left half, after it under its right half, or the strip's length past the
/// last tab, which appends. It is the very gap the last drag hover reported, so
/// the bar a [`TabBar::drop_indicator`] drew is where the tab goes. The owner
/// turns the pair into a move, telling a same-strip drop (the payload's source
/// is this bar's own tag) from a foreign one by comparing tags.
type ReorderHandler = Rc<dyn Fn(DraggedTab, usize, &mut Window, &mut App)>;

/// A tab mid-drag, whether over its own strip or another one.
///
/// The bar knows nothing about what a tab *is* — and in particular nothing
/// about panes — so the source is the opaque tag the owner handed
/// [`TabBar::drag_source`]: whatever identifies the strip that built the bar.
/// Two bars whose tags compare equal are the same strip; anything else is a
/// foreign tab being moved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraggedTab {
    /// Tag of the strip the drag started on.
    pub source: u64,
    /// Index of the dragged tab in that strip, as drawn when the drag began.
    pub index: usize,
}

/// The drop bar of every strip a window draws, kept by the window's owner.
///
/// The bookkeeping each app used to write out for itself around
/// [`TabBar::on_drag_over`], [`TabBar::drop_indicator`] and
/// [`TabBar::on_reorder`]: at most one strip shows a bar at a time — a pointer
/// is over one strip or none — so the state is that strip's tag and the gap
/// its bar stands in. It holds no context and notifies nothing: every method
/// that can change it says whether it did, and the owner notifies on `true`.
///
/// ```ignore
/// TabBar::new("tabs")
///     .drag_source(TAG)
///     .drop_indicator(self.tab_drop.indicator(TAG))
///     .on_drag_over(move |dragged, index, _, cx| {
///         this.update(cx, |view, cx| {
///             if view.tab_drop.drag_over(TAG, index, dragged.source == TAG) {
///                 cx.notify();
///             }
///         })
///     })
///     .on_reorder(move |dragged, to, _, cx| {
///         this.update(cx, |view, cx| {
///             view.tab_drop.clear();
///             view.move_tab(dragged.index, to, cx);
///             cx.notify();
///         })
///     })
/// ```
///
/// A drag cancelled with a release outside every drop target reaches no drop
/// handler, so the owner's root also calls [`TabDropState::clear`] on its own
/// mouse-up — a no-op after a drop, which already took the bar.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TabDropState {
    /// Tag of the strip showing the bar, and the gap the bar stands in.
    at: Option<(u64, usize)>,
}

impl TabDropState {
    /// No bar on any strip.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one report of [`TabBar::on_drag_over`] from the strip tagged
    /// `strip`, and says whether the bar changed.
    ///
    /// `accepted` is the owner's verdict on the dragged tab — whether a drop
    /// here would be taken at all — so a foreign tab this strip refuses never
    /// raises the bar. A `None` takes the bar down only when it is this
    /// strip's own, so the strips a drag is not over, which all report `None`
    /// on every move, never wipe the one it is.
    pub fn drag_over(&mut self, strip: u64, index: Option<usize>, accepted: bool) -> bool {
        let next = match index {
            None if self.indicator(strip).is_some() => None,
            None => return false,
            Some(_) if !accepted => return false,
            Some(index) => Some((strip, index)),
        };
        let changed = self.at != next;
        self.at = next;
        changed
    }

    /// The gap the strip tagged `strip` should draw its bar in, for
    /// [`TabBar::drop_indicator`].
    pub fn indicator(&self, strip: u64) -> Option<usize> {
        self.at
            .and_then(|(tag, index)| (tag == strip).then_some(index))
    }

    /// The strip showing a bar and its gap, if any strip is.
    pub fn current(&self) -> Option<(u64, usize)> {
        self.at
    }

    /// Takes the bar down and answers where a drop on `strip` lands: the gap
    /// its bar stood in, or `fallback` when the bar belongs to another strip
    /// or to none.
    ///
    /// For drop targets the bar does not cover — a pane's body, say, whose
    /// static answer is "after the last tab" — since [`TabBar::on_reorder`]
    /// already reports the hovered gap itself.
    pub fn resolve(&mut self, strip: u64, fallback: usize) -> usize {
        match self.at.take() {
            Some((tag, index)) if tag == strip => index,
            _ => fallback,
        }
    }

    /// Takes the bar down, saying whether one was up.
    pub fn clear(&mut self) -> bool {
        self.at.take().is_some()
    }
}

/// Where a drag over a tab spanning `left..right` lands with the pointer at
/// `x`: the gap before the tab at `index` under its left half, after it under
/// its right half.
fn gap_over_tab(index: usize, left: Pixels, right: Pixels, x: Pixels) -> usize {
    if x < left + (right - left) / 2. {
        index
    } else {
        index + 1
    }
}

/// The 2 px accent standing in a gap of the strip, still to be placed along
/// it by the caller.
///
/// Absolute, so it lays over whatever it sits on and takes no room in the row.
fn insertion_bar(color: Hsla) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(INDICATOR_WIDTH))
        .bg(color)
}

/// What follows the pointer while a tab is dragged.
///
/// The tab's own title on the active fill with an accent edge, so the ghost
/// reads as the tab it came from rather than as a bare rectangle.
struct DragGhost {
    /// Title of the tab being dragged.
    title: SharedString,
    /// Palette the ghost is drawn with.
    theme: Theme,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(4.))
            .bg(self.theme.surface_active)
            .border_1()
            .border_color(self.theme.accent)
            .rounded_md()
            .text_size(px(13.))
            .text_color(self.theme.text)
            .child(self.title.clone())
    }
}

/// A stateless tab strip.
///
/// The bar owns no selection state: the parent view passes the current tabs and
/// the active index on every render, and reacts to [`TabBar::on_select`],
/// [`TabBar::on_close`], [`TabBar::on_context_menu`] and [`TabBar::on_new`].
/// The context menu itself is the parent's too — the bar only reports where the
/// right-click landed.
///
/// The tab list scrolls horizontally once it overflows; the dropdown listing
/// every tab and the "+" button stay pinned to the right edge. Scrolling the
/// active tab back into view is the parent's job, through the handle it passes
/// to [`TabBar::scroll_handle`].
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<TabItem>,
    active: usize,
    scroll_handle: Option<ScrollHandle>,
    menu_open: bool,
    on_select: Option<IndexHandler>,
    on_close: Option<IndexHandler>,
    on_context_menu: Option<ContextHandler>,
    on_new: Option<PlainHandler>,
    on_menu_open_change: Option<OpenChangeHandler>,
    drag_source: Option<u64>,
    on_reorder: Option<ReorderHandler>,
    drop_indicator: Option<usize>,
    on_drag_over: Option<DragOverHandler>,
    focus_handle: Option<FocusHandle>,
    scrollbar: Option<Scrollbar>,
    menu_icon: Option<SharedString>,
    new_icon: Option<SharedString>,
    menu_tooltip: Option<SharedString>,
    new_tooltip: Option<SharedString>,
    close_tooltip: Option<SharedString>,
}

impl TabBar {
    /// Creates an empty tab bar.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            active: 0,
            scroll_handle: None,
            menu_open: false,
            scrollbar: None,
            on_select: None,
            on_close: None,
            on_context_menu: None,
            on_new: None,
            on_menu_open_change: None,
            drag_source: None,
            on_reorder: None,
            drop_indicator: None,
            on_drag_over: None,
            focus_handle: None,
            menu_icon: None,
            new_icon: None,
            menu_tooltip: None,
            new_tooltip: None,
            close_tooltip: None,
        }
    }

    /// Draws `bar` over the tabs as the overlay scroll indicator.
    ///
    /// Passed in rather than built here, and only while it should be on screen:
    /// a bar comes and goes with the scrolling, which is state this widget
    /// cannot keep — it is built afresh on every render. The owner keeps a
    /// [`ScrollbarState`](crate::scrollbar::ScrollbarState) beside the handle it
    /// gives [`TabBar::scroll_handle`], and owns the drag too, since the same
    /// id it built the bar with is what tells that drag from any other.
    pub fn scrollbar(mut self, bar: Scrollbar) -> Self {
        self.scrollbar = Some(bar);
        self
    }

    /// Draws the asset at `path` on the tab dropdown instead of its glyph.
    pub fn menu_icon(mut self, path: impl Into<SharedString>) -> Self {
        self.menu_icon = Some(path.into());
        self
    }

    /// Draws the asset at `path` on the "+" button instead of its glyph.
    pub fn new_icon(mut self, path: impl Into<SharedString>) -> Self {
        self.new_icon = Some(path.into());
        self
    }

    /// Sets the hover labels of the bar's three buttons: the tab dropdown, the
    /// "+", and the close button on every tab.
    ///
    /// Passed in rather than looked up, like every other string here: this layer
    /// carries no text of its own, so the localised wording belongs to the view
    /// that builds the bar. Any of them left unset simply shows no tooltip.
    pub fn tooltips(
        mut self,
        menu: impl Into<SharedString>,
        new: impl Into<SharedString>,
        close: impl Into<SharedString>,
    ) -> Self {
        self.menu_tooltip = Some(menu.into());
        self.new_tooltip = Some(new.into());
        self.close_tooltip = Some(close.into());
        self
    }

    /// Sets the tabs to render, in display order.
    pub fn tabs(mut self, tabs: Vec<TabItem>) -> Self {
        self.tabs = tabs;
        self
    }

    /// Sets the index of the highlighted tab.
    pub fn active(mut self, index: usize) -> Self {
        self.active = index;
        self
    }

    /// Tracks the horizontal scroll of the tab list with `handle`.
    ///
    /// The handle indexes the tabs in display order, so the parent can bring the
    /// active tab back into view with [`gpui::ScrollHandle::scroll_to_item`].
    pub fn scroll_handle(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(handle.clone());
        self
    }

    /// Sets whether the tab dropdown is currently shown.
    pub fn menu_open(mut self, open: bool) -> Self {
        self.menu_open = open;
        self
    }

    /// Called with the index of the tab the user clicked.
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Called with the index of the tab whose close button was clicked.
    ///
    /// Setting this handler is what makes the close buttons appear.
    pub fn on_close(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Called with the index of the right-clicked tab and the window-space
    /// position of the pointer, for the parent to open a context menu at.
    ///
    /// A right-click deliberately does *not* also select the tab: the commands a
    /// tab menu offers differ for the active tab and for any other one, so the
    /// selection has to survive the click that opens the menu.
    pub fn on_context_menu(
        mut self,
        handler: impl Fn(usize, Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_context_menu = Some(Rc::new(handler));
        self
    }

    /// Called when the "+" button is clicked.
    ///
    /// Setting this handler is what makes the "+" button appear.
    pub fn on_new(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_new = Some(Rc::new(handler));
        self
    }

    /// Called with the open state the tab dropdown would like to be in.
    ///
    /// Setting this handler is what makes the dropdown button appear; it is
    /// still left out while the bar has no tabs to list.
    pub fn on_menu_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_menu_open_change = Some(Rc::new(handler));
        self
    }

    /// Tags this bar as a drag source, which is what makes its tabs draggable.
    ///
    /// Without a tag the strip neither starts drags nor answers drops; every
    /// other handler keeps working exactly as before, so bars that do not opt
    /// in — the connection strip, for one — are untouched. The tag is what a
    /// drop carries home in [`DraggedTab::source`], so the owner can tell its
    /// own tabs from foreign ones: pass whatever identifies the strip, such
    /// as the pane id behind it.
    pub fn drag_source(mut self, source: u64) -> Self {
        self.drag_source = Some(source);
        self
    }

    /// Called when a dragged tab is dropped on a tab of this bar or on its
    /// empty stretch.
    ///
    /// Setting this handler is what makes the bar a drop target. The index is
    /// an insertion gap, `0..=len`: the one the pointer's half of the tab
    /// under it names, or the strip's length over the empty stretch past the
    /// last tab — the same gap [`TabBar::on_drag_over`] last reported and
    /// [`TabBar::drop_indicator`] drew, so a drop on a tab's right half lands
    /// after it rather than before. The bar remembers that gap itself, so an
    /// owner that draws no indicator still gets it. Dropping a tab on itself,
    /// or into either gap beside it, is reported like any other drop and left
    /// to the owner to ignore.
    ///
    /// The keyboard reorder of a focusable bar ([`TabBar::focus_handle`])
    /// reports through this handler too, with the bar's own
    /// [`TabBar::drag_source`] tag as the source.
    ///
    /// Up to 0.1.7 the index named the tab dropped on, whichever half took
    /// the drop. An owner that mapped it through its own record of the last
    /// hovered gap — the glue [`TabDropState`] replaces — reads the same index
    /// either way.
    ///
    /// To see it work: drag a tab along its own strip — the tabs reorder —
    /// then across to another pane's strip, its empty stretch, or the pane
    /// body below it, where the tab moves panes and the accent frame follows
    /// it. The ghost trailing the pointer carries the dragged tab's title.
    pub fn on_reorder(
        mut self,
        handler: impl Fn(DraggedTab, usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reorder = Some(Rc::new(handler));
        self
    }

    /// Draws the insertion bar at `index` while a tab is dragged over the strip.
    ///
    /// `None` draws no bar. `Some(index)` draws a 2 px accent bar in the gap
    /// before the tab at `index`, or past the last tab when it names the
    /// strip's length. The owner keeps the latest index its
    /// [`TabBar::on_drag_over`] reported — [`TabDropState`] does exactly that
    /// — and the drop then lands in the gap the bar stands in.
    ///
    /// Laid over the tabs rather than between them, so raising and moving it
    /// shifts no tab: a bar that took up room would push the tab under the
    /// pointer aside, change which half the pointer is over, and flicker
    /// between two gaps.
    pub fn drop_indicator(mut self, index: Option<usize>) -> Self {
        self.drop_indicator = index;
        self
    }

    /// Called while a tab is dragged over the strip with where it would land.
    ///
    /// Called once per pointer move of any tab drag in the window: with the
    /// gap the pointer's half of the tab under it names, the strip's length
    /// over the empty stretch past the last tab, or `None` while the pointer is
    /// anywhere outside the scrolling row — the dropdown and the "+" included,
    /// so a bar never lingers over buttons that take no drop. Unset, the strip
    /// reports nothing and draws no bar — the bars that do not opt in keep
    /// working exactly as before.
    pub fn on_drag_over(
        mut self,
        handler: impl Fn(DraggedTab, Option<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drag_over = Some(Rc::new(handler));
        self
    }

    /// Makes the strip a keyboard stop, tracked by `handle`.
    ///
    /// The handle is the owner's, kept beside the scroll handle, since this
    /// widget is rebuilt every frame and could not keep one of its own. While
    /// it holds focus the active tab wears an accent ring and the keys [`init`]
    /// binds work the strip: Left and Right (Home and End) ask
    /// [`TabBar::on_select`] for the neighbouring (first, last) tab, Delete
    /// asks [`TabBar::on_close`] to close the active one when it is closable,
    /// and Ctrl+Shift+PageUp / PageDown (Cmd on macOS) ask
    /// [`TabBar::on_reorder`] to move it one place, as a drop in the gap past
    /// its neighbour would — which needs a [`TabBar::drag_source`] tag, the
    /// source a reorder reports. At either end of the strip a key does nothing.
    ///
    /// The bar does not take focus when a tab is clicked: a click on a tab
    /// usually means "show me this", and the owner moves focus into what it
    /// shows. How the keyboard reaches the strip — a command, a tab stop — is
    /// the owner's call too.
    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }
}

/// What a keyboard command on a strip of `len` tabs whose active one is
/// `active` asks of the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyRequest {
    /// Select the tab at this index.
    Select(usize),
    /// Close the tab at this index.
    Close(usize),
    /// Move the tab at `from` into the insertion gap `to`.
    Move { from: usize, to: usize },
}

/// The keyboard commands, as plain arithmetic the tests can reach without a
/// window: `None` wherever the command has nothing to do.
fn key_request(command: KeyCommand, active: usize, len: usize) -> Option<KeyRequest> {
    if active >= len {
        return None;
    }
    match command {
        KeyCommand::Prev => active.checked_sub(1).map(KeyRequest::Select),
        KeyCommand::Next => (active + 1 < len).then_some(KeyRequest::Select(active + 1)),
        KeyCommand::First => (active != 0).then_some(KeyRequest::Select(0)),
        KeyCommand::Last => (active + 1 != len).then_some(KeyRequest::Select(len - 1)),
        KeyCommand::Close => Some(KeyRequest::Close(active)),
        // One place left is the gap before the left neighbour; one place
        // right is the gap past the right neighbour, since the gap straight
        // after the tab is where it already is.
        KeyCommand::MoveLeft => active
            .checked_sub(1)
            .map(|to| KeyRequest::Move { from: active, to }),
        KeyCommand::MoveRight => (active + 1 < len).then_some(KeyRequest::Move {
            from: active,
            to: active + 2,
        }),
    }
}

/// The actions above, folded into one value for [`key_request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyCommand {
    Prev,
    Next,
    First,
    Last,
    Close,
    MoveLeft,
    MoveRight,
}

impl RenderOnce for TabBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme(cx);
        let id = self.id;
        let active = self.active;
        let on_select = self.on_select;
        let on_close = self.on_close;
        let on_context_menu = self.on_context_menu;
        let close_tooltip = self.close_tooltip;
        let scroll_handle = self.scroll_handle.clone();
        let drag_source = self.drag_source;
        let on_reorder = self.on_reorder;
        let drop_indicator = self.drop_indicator;
        let on_drag_over = self.on_drag_over;
        let indicator_color = theme.accent;
        let tab_count = self.tabs.len();
        // The gap a drag over this strip currently names, kept across frames
        // because the drop that ends the drag is answered by the listeners of
        // the frame drawn after the last move — the one the owner's
        // `on_drag_over` notify asked for — which never saw that move. Keyed by
        // the bar's id, so every strip of a window keeps its own.
        let drop_gap = (on_reorder.is_some() || on_drag_over.is_some()).then(|| {
            window
                .use_keyed_state(ElementId::from((id.clone(), "drop-gap")), cx, |_, _| {
                    Rc::new(Cell::new(None::<usize>))
                })
                .read(cx)
                .clone()
        });
        // Read here rather than inside the wheel handler below. gpui pushes an
        // element's text style around its children's layout, so this is the
        // line height the scrolling row itself is laid out with — the very one
        // gpui would convert a line-based wheel delta with. By the time an event
        // is dispatched that stack is empty again and the same call would answer
        // with the default font instead, scrolling a tab faster than the gap
        // beside it.
        let line_height = window.line_height();

        // An empty bar has nothing to list, so its dropdown stays away.
        let on_menu_open_change = self.on_menu_open_change.filter(|_| !self.tabs.is_empty());
        let menu = on_menu_open_change.map(|on_open_change| {
            let entries = self
                .tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    // The list is a choice, not a set of commands: the row of
                    // the tab that is showing carries the menu's check mark.
                    // It used to be pushed into the shortcut slot, on the right
                    // and in the muted hint colour, because there was nowhere
                    // else to put it.
                    let mut entry = MenuEntry::new(tab.title.clone()).checked(index == active);
                    if let Some(handler) = on_select.clone() {
                        entry = entry.on_activate(move |window, cx| handler(index, window, cx));
                    }
                    entry
                })
                .collect();

            MenuButton::new(ElementId::from((id.clone(), "tab-menu")))
                .glyph(TAB_MENU_GLYPH)
                .when_some(self.menu_icon.clone(), MenuButton::icon)
                .when_some(self.menu_tooltip.clone(), MenuButton::tooltip)
                .open(self.menu_open)
                .entries(entries)
                .on_open_change(move |open, window, cx| on_open_change(open, window, cx))
        });

        // The keyboard's half of the bar, answered from what this frame was
        // built with: the selection the keys move from is the one the owner
        // passed in, so a key the owner has not caught up with yet simply asks
        // again from the same place.
        let focused = self
            .focus_handle
            .as_ref()
            .is_some_and(|handle| handle.is_focused(window));
        let keys = self.focus_handle.map(|handle| {
            let closable = self.tabs.get(active).is_some_and(|tab| tab.closable);
            let on_select = on_select.clone();
            let on_close = on_close.clone().filter(|_| closable);
            let on_reorder = on_reorder.clone();
            let answer = move |command, window: &mut Window, cx: &mut App| match key_request(
                command, active, tab_count,
            ) {
                Some(KeyRequest::Select(index)) => {
                    if let Some(handler) = &on_select {
                        handler(index, window, cx);
                    }
                }
                Some(KeyRequest::Close(index)) => {
                    if let Some(handler) = &on_close {
                        handler(index, window, cx);
                    }
                }
                Some(KeyRequest::Move { from, to }) => {
                    if let (Some(handler), Some(source)) = (&on_reorder, drag_source) {
                        handler(
                            DraggedTab {
                                source,
                                index: from,
                            },
                            to,
                            window,
                            cx,
                        );
                    }
                }
                None => {}
            };
            (handle, Rc::new(answer))
        });

        let tab_theme = theme.clone();
        let on_reorder_tabs = on_reorder.clone();
        let drop_gap_tabs = drop_gap.clone();
        let tabs: Vec<_> = self
            .tabs
            .into_iter()
            .enumerate()
            .map(move |(index, tab)| {
                let theme = &tab_theme;
                let is_active = index == active;
                let group = SharedString::from(format!("rugpui-tab-{index}"));
                let close_id = ElementId::from((tab.id.clone(), "close"));
                let mark_id = ElementId::from((tab.id.clone(), "mark"));
                let on_close = on_close.clone().filter(|_| tab.closable);
                let ghost_title = tab.title.clone();

                let indicator_left = drop_indicator == Some(index);
                let indicator_right = index + 1 == tab_count && drop_indicator == Some(tab_count);

                div()
                    .id(tab.id)
                    // The anchor of the insertion bar, which is laid over the
                    // tab rather than beside it.
                    .relative()
                    // The strip may sit inside a drag area when the toolbar
                    // doubles as the title bar; occluding is what keeps a click on
                    // a tab from being read as "move the window" instead.
                    .occlude()
                    .group(group.clone())
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(10.))
                    .border_b_2()
                    .border_color(if is_active {
                        theme.accent
                    } else {
                        transparent_black()
                    })
                    .bg(if is_active {
                        theme.surface_active
                    } else {
                        transparent_black()
                    })
                    .text_size(px(13.))
                    .text_color(if is_active {
                        theme.text
                    } else {
                        theme.text_muted
                    })
                    .cursor_pointer()
                    .hover(|style| {
                        style.bg(if is_active {
                            theme.surface_active
                        } else {
                            theme.surface_hover
                        })
                    })
                    .when_some(on_select.clone(), |this, handler| {
                        this.on_click(move |_, window, cx| handler(index, window, cx))
                    })
                    .when_some(on_context_menu.clone(), |this, handler| {
                        this.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            // The press belongs to the menu, not to whatever is
                            // underneath the strip.
                            cx.stop_propagation();
                            handler(index, event.position, window, cx);
                        })
                    })
                    // A drag starts on a left press held past gpui's threshold,
                    // so an ordinary click still selects and a right press still
                    // opens the menu: neither gesture travels far enough to count.
                    .when_some(drag_source, |this, source| {
                        let ghost_title = ghost_title.clone();
                        this.on_drag(DraggedTab { source, index }, move |_, _, _, cx| {
                            let theme = crate::theme::theme(cx);
                            cx.new(|_| DragGhost {
                                title: ghost_title.clone(),
                                theme,
                            })
                        })
                    })
                    .when_some(
                        on_reorder_tabs.clone().zip(drop_gap_tabs.clone()),
                        |this, (handler, gap)| {
                            // Innermost first: a drop on a tab stops here and
                            // never reaches the strip's own target below, so the
                            // two cannot both answer one gesture. It lands in the
                            // gap the last move named — the one the bar stands
                            // in — and only a drop no move preceded falls back to
                            // the gap before this tab.
                            this.on_drop::<DraggedTab>(move |dragged, window, cx| {
                                let at = gap.take().unwrap_or(index);
                                handler(*dragged, at, window, cx)
                            })
                        },
                    )
                    .when_some(drop_gap_tabs.clone(), |this, gap| {
                        // Every move reaches every listener in the capture phase
                        // with no hover check, parents before children. The row's
                        // wrapper has already said whether the pointer is inside
                        // the visible row at all; a tab under the pointer then
                        // narrows that to the gap its halves name. Asking the
                        // wrapper first is what keeps a tab scrolled in under the
                        // dropdown or the "+" — still there, merely clipped —
                        // from answering for them.
                        this.on_drag_move::<DraggedTab>(
                            move |event: &DragMoveEvent<DraggedTab>, _window, _cx| {
                                let pointer = event.event.position;
                                if gap.get().is_some() && event.bounds.contains(&pointer) {
                                    gap.set(Some(gap_over_tab(
                                        index,
                                        event.bounds.left(),
                                        event.bounds.right(),
                                        pointer.x,
                                    )));
                                }
                            },
                        )
                    })
                    .when_some(scroll_handle.clone(), |this, handle| {
                        // The occlusion above cuts the scrolling row out of the hit
                        // test wherever a tab covers it, and gpui asks the row's own
                        // hit box before it scrolls anything — so a wheel turned
                        // over a tab, which is most of the strip, would move
                        // nothing. Answering it here puts that back.
                        //
                        // Deliberately the same arithmetic gpui applies to the row
                        // itself, so the two are indistinguishable: a wheel over a
                        // tab and one over the gap beside it move the strip by the
                        // same amount. The row scrolls on one axis, which is why a
                        // vertical wheel — every plain mouse has one, and no
                        // horizontal one — folds onto it. Nothing is clamped here
                        // either: gpui pins the offset to the scrollable range on
                        // the next layout pass, and this writes to the very cell it
                        // pins.
                        this.on_scroll_wheel(move |event, window, _cx| {
                            let delta = event.delta.pixel_delta(line_height);
                            let delta_x = if delta.x.is_zero() { delta.y } else { delta.x };
                            if delta_x.is_zero() {
                                return;
                            }

                            let mut offset = handle.offset();
                            offset.x += delta_x;
                            handle.set_offset(offset);
                            window.refresh();
                        })
                    })
                    .when_some(
                        tab.dot
                            .or_else(|| tab.status.map(|status| status.color(theme))),
                        |this, color| {
                            this.child(div().flex_none().size(px(6.)).rounded_full().bg(color))
                        },
                    )
                    .child(div().whitespace_nowrap().child(tab.title))
                    .when_some(tab.mark, |this, mark| {
                        this.child(
                            // Behind the title rather than in front of it, where
                            // the dot already is: the dot reports on a state every
                            // tab has, the mark on something only some tabs are
                            // doing, and a second symbol before the title would
                            // push the titles of a strip out of line with one
                            // another for the sake of the few tabs that carry one.
                            //
                            // Identified, and not for the sake of the hit test: a
                            // tooltip is kept in gpui's element state, which only
                            // an element with an id of its own is given — an
                            // unidentified one would simply never appear. Not
                            // occluding, for the same reason the close button does
                            // not: this sits inside the tab, whose own occlusion
                            // already answers for it, and taking the tab out of the
                            // hit list under the pointer would take the close
                            // button's `group_hover` with it.
                            div()
                                .id(mark_id)
                                .flex()
                                .flex_none()
                                .items_center()
                                .tooltip(tooltip_label(mark.tooltip))
                                .child(
                                    svg()
                                        .size(px(MARK_SIZE))
                                        .flex_none()
                                        .path(mark.icon)
                                        .text_color(theme.icon),
                                ),
                        )
                    })
                    .when_some(on_close, |this, handler| {
                        this.child(
                            div()
                                .id(close_id)
                                // Deliberately *not* occluding, unlike the tab and
                                // the "+": this button only ever sits inside a tab,
                                // whose own occlusion already keeps the drag area
                                // behind the strip from answering for it. Occluding
                                // here would be worse than redundant — gpui's hit
                                // test stops at the first occluding hitbox, so the
                                // tab's hitbox would drop out of the hit list the
                                // moment the pointer reached the button, the
                                // `group_hover` below would read as false, and the
                                // button would hide itself out from under the
                                // pointer. A hidden element paints no listeners, so
                                // the click that followed would land on nothing.
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size(px(16.))
                                .rounded_sm()
                                .text_size(px(12.))
                                // The mark on a button, not a word in the title
                                // beside it, so it takes the icon tint.
                                .text_color(theme.icon)
                                .invisible()
                                .group_hover(group.clone(), |style| style.visible())
                                .hover(|style| style.bg(theme.surface_hover).text_color(theme.text))
                                // Keep the click from also selecting the tab.
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| handler(index, window, cx))
                                .when_some(close_tooltip.clone(), |this, tooltip| {
                                    this.tooltip(tooltip_label(tooltip))
                                })
                                .child("\u{00d7}"),
                        )
                    })
                    // A ring rather than a fill or a border: the active tab
                    // already wears both, and a border would take room and
                    // nudge the title. Inset, so the strip's clip keeps all
                    // four sides.
                    .when(focused && is_active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .border_1()
                                .border_color(theme.accent)
                                .rounded_sm(),
                        )
                    })
                    // Last, so the bar paints over the tab it sits on.
                    .when(indicator_left, |this| {
                        this.child(insertion_bar(indicator_color).left(px(if index == 0 {
                            0.
                        } else {
                            -INDICATOR_WIDTH / 2.
                        })))
                    })
                    .when(indicator_right, |this| {
                        this.child(insertion_bar(indicator_color).right_0())
                    })
            })
            .collect();

        div()
            .id(id.clone())
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(36.))
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
            .when_some(keys, |this, (handle, answer)| {
                let on = |command| {
                    let answer = answer.clone();
                    move |window: &mut Window, cx: &mut App| answer(command, window, cx)
                };
                let (prev, next, first, last) = (
                    on(KeyCommand::Prev),
                    on(KeyCommand::Next),
                    on(KeyCommand::First),
                    on(KeyCommand::Last),
                );
                let (close, left, right) = (
                    on(KeyCommand::Close),
                    on(KeyCommand::MoveLeft),
                    on(KeyCommand::MoveRight),
                );
                this.track_focus(&handle)
                    .key_context(KEY_CONTEXT)
                    .on_action(move |_: &SelectPrevTab, window, cx| prev(window, cx))
                    .on_action(move |_: &SelectNextTab, window, cx| next(window, cx))
                    .on_action(move |_: &SelectFirstTab, window, cx| first(window, cx))
                    .on_action(move |_: &SelectLastTab, window, cx| last(window, cx))
                    .on_action(move |_: &CloseActiveTab, window, cx| close(window, cx))
                    .on_action(move |_: &MoveTabLeft, window, cx| left(window, cx))
                    .on_action(move |_: &MoveTabRight, window, cx| right(window, cx))
            })
            .child(
                // The wrapper is what the overlay bar is placed against, and
                // it exists only for that: the scrolling row cannot hold its
                // own bar, because its children are what scroll away. Sized to
                // the row, so the bar spans the tabs and stops short of the
                // dropdown and the "+" beside them.
                div()
                    .relative()
                    .flex_grow_1()
                    .min_w_0()
                    .h_full()
                    .when_some(drop_gap.clone(), |this, gap| {
                        // First of the drag listeners to hear a move, being the
                        // outermost: the pointer inside the visible row starts
                        // out naming the empty stretch past the last tab, which
                        // the tab under it, if any, then narrows. Anywhere else
                        // — the dropdown and the "+" included — the drag is not
                        // over the row, and whatever gap was named no longer
                        // applies.
                        this.on_drag_move::<DraggedTab>(
                            move |event: &DragMoveEvent<DraggedTab>, _window, _cx| {
                                let inside = event.bounds.contains(&event.event.position);
                                gap.set(inside.then_some(tab_count));
                            },
                        )
                    })
                    // An empty strip has no tab to lay its bar over.
                    .when(tab_count == 0 && drop_indicator == Some(0), |this| {
                        this.child(insertion_bar(indicator_color).left_0())
                    })
                    .child(
                        div()
                            .id(ElementId::from((id.clone(), "tabs")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .size_full()
                            .overflow_x_scroll()
                            .when_some(self.scroll_handle.as_ref(), |this, handle| {
                                this.track_scroll(handle)
                            })
                            // Past the last tab, where there is no tab to land
                            // on: the strip's length, which the owner appends at.
                            .when_some(
                                on_reorder.clone().zip(drop_gap.clone()),
                                |this, (handler, gap)| {
                                    this.on_drop::<DraggedTab>(move |dragged, window, cx| {
                                        let at = gap.take().unwrap_or(tab_count);
                                        handler(*dragged, at, window, cx)
                                    })
                                },
                            )
                            .children(tabs),
                    )
                    .when_some(
                        on_drag_over.clone().zip(drop_gap.clone()),
                        |this, (handler, gap)| {
                            // Last of the drag listeners to hear a move, being
                            // painted after the row and every tab in it: by now the
                            // wrapper and the tab under the pointer have settled the
                            // gap, so the owner hears one answer per move instead of
                            // a provisional one followed by its correction. Sized to
                            // nothing — it is here for its place in the order, and
                            // must not stand in the way of the hit test.
                            this.child(div().absolute().size_0().on_drag_move::<DraggedTab>(
                                move |event: &DragMoveEvent<DraggedTab>, window, cx| {
                                    handler(*event.drag(cx), gap.get(), window, cx);
                                },
                            ))
                        },
                    )
                    .children(self.scrollbar.and_then(|bar| bar.render(&theme))),
            )
            .children(menu)
            .when_some(self.on_new, |this, handler| {
                // An SVG face keeps its own `text_color`, which does not
                // inherit from the button the way the glyph's does, so the
                // hover shade has to reach it through the group — the same
                // arrangement as the dropdown's icon.
                let hover_text = theme.text;
                let face = match self.new_icon.clone() {
                    Some(path) => svg()
                        .size(px(16.))
                        .flex_none()
                        .path(path)
                        .text_color(theme.icon)
                        .group_hover(NEW_GROUP, move |style| style.text_color(hover_text))
                        .into_any_element(),
                    None => "+".into_any_element(),
                };
                this.child(
                    div()
                        .id(ElementId::from((id.clone(), "new")))
                        // Same reason as the tab itself: a drag area may lie
                        // behind the strip.
                        .occlude()
                        .group(NEW_GROUP)
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(28.))
                        .mx(px(4.))
                        .rounded_md()
                        .text_size(px(16.))
                        // Reaches the bare "+" this button falls back to when
                        // no icon was handed in; the icon carries its own tint
                        // above, and the two have to agree.
                        .text_color(theme.icon)
                        .cursor_pointer()
                        .hover(|style| style.bg(theme.surface_hover).text_color(theme.text))
                        .on_click(move |_, window, cx| handler(window, cx))
                        .when_some(self.new_tooltip.clone(), |this, tooltip| {
                            this.tooltip(tooltip_label(tooltip))
                        })
                        .child(face),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ops::Deref;
    use std::time::Duration;

    use gpui::{
        Bounds, Context, DragMoveEvent, Entity, Modifiers, MouseUpEvent, Render, ScrollDelta,
        ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, WindowControlArea, point,
    };

    use crate::scrollbar::{
        DraggedThumb, FADE_OUT, Fade, SCROLL_LINGER, ScrollbarAxis, ScrollbarState, hide_later,
        hide_now, scroll_to, scrolled,
    };

    use super::*;

    /// Vertical middle of the strip; every row of it is one tab tall.
    const ROW_MIDDLE: f32 = 18.;

    /// Vertical middle of the reference row, which sits under the strip.
    const REFERENCE_MIDDLE: f32 = 54.;

    /// A column comfortably inside the first tab, whatever its title.
    const INSIDE_FIRST_TAB: f32 = 12.;

    /// How far right of the strip's left edge the sweep looks for the button.
    ///
    /// Wide enough to clear one short tab title at any font the test platform
    /// picks, and still short of the "+" that follows the strip.
    const SWEEP_WIDTH: i32 = 200;

    /// Width of the reference row's single child.
    ///
    /// Only has to overflow the window by enough that a wheel turn or two never
    /// reaches the end of the scrollable range, where clamping would hide a
    /// difference in how far the two rows moved.
    const REFERENCE_CONTENT: f32 = 8000.;

    /// How many tabs the scrolling tests put in the strip.
    ///
    /// Same reasoning as [`REFERENCE_CONTENT`]: enough that the strip overflows
    /// any plausible test display several times over.
    const CROWDED: usize = 200;

    /// Long enough for a timer that has come due to be seen to have come due,
    /// and far too short to reach the next one.
    const A_MOMENT: Duration = Duration::from_millis(10);

    /// A row of the strip the overlay bar covers: low enough to be on the
    /// thumb, which rides the bottom edge.
    const BAR_MIDDLE: f32 = 31.;

    /// Element id of the harness's overlay bar, as the workspace names its own.
    const BAR: &str = "tab-scrollbar";

    /// The text size the workspace root sets, which the strip inherits.
    ///
    /// The harness repeats it because it is what a line-based wheel delta is
    /// converted with — a harness in the default font would be measuring a
    /// conversion the app never performs.
    const INHERITED_TEXT: f32 = 13.;

    /// What a sweep of the tab row answered with, counted per handler.
    #[derive(Clone, Default)]
    struct Tally {
        selected: Rc<Cell<usize>>,
        closed: Rc<Cell<usize>>,
        dragged: Rc<Cell<usize>>,
    }

    /// A tab strip in the arrangement that made the close button unclickable:
    /// inside an occluding drag area, the way the toolbar renders it when it
    /// doubles as the title bar.
    ///
    /// The drag area counts the presses that reach it. That stands in for what
    /// the real title bar does with them — start a window move on Linux, answer
    /// `HTCAPTION` on Windows — and both read the same hit test this does.
    ///
    /// Under it is a bare horizontally scrolling row with nothing occluding it,
    /// laid out in the same inherited text style: gpui's own answer to a wheel,
    /// to hold the strip's against.
    struct Harness {
        tally: Tally,
        tabs: Vec<TabItem>,
        strip: ScrollHandle,
        bar: ScrollbarState,
        reference: ScrollHandle,
        fade: Rc<Cell<Fade>>,
    }

    impl Harness {
        /// The strip's overlay bar, the way the workspace builds it.
        fn scrollbar(&self) -> Scrollbar {
            Scrollbar::for_handle(BAR, ScrollbarAxis::Horizontal, &self.strip).fade(self.bar.fade())
        }

        /// Lets go of the thumb, and starts the clock on the bar again.
        fn release(&mut self, cx: &mut Context<Self>) {
            if let Some(epoch) = self.bar.release() {
                hide_later(epoch, cx, |harness| Some(&mut harness.bar));
                cx.notify();
            }
        }

        /// Answers the pointer arriving on the strip's bottom edge and leaving
        /// it again, as the workspace does.
        fn hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
            if hovered {
                if self.bar.hover_enter() {
                    cx.notify();
                }
                return;
            }

            if let Some(epoch) = self.bar.hover_leave() {
                hide_now(self, epoch, cx, |harness| Some(&mut harness.bar));
            }
        }
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let Tally {
                selected,
                closed,
                dragged,
            } = self.tally.clone();

            // The owner's half of a bar, copied from the workspace: notice the
            // strip moving, arm the expiry from inside the render that noticed
            // it, and answer drags of the thumb from the root.
            if let Some(epoch) = self
                .bar
                .moved(scrolled(&self.strip, ScrollbarAxis::Horizontal))
            {
                hide_later(epoch, cx, |harness| Some(&mut harness.bar));
            }
            self.fade.set(self.bar.fade());

            div()
                .flex()
                .flex_col()
                .size_full()
                .text_size(px(INHERITED_TEXT))
                .on_drag_move::<DraggedThumb>(cx.listener(
                    |harness, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                        let Some(progress) = harness.scrollbar().dragged(event, cx) else {
                            return;
                        };
                        harness.bar.hold();
                        scroll_to(&harness.strip, ScrollbarAxis::Horizontal, progress);
                        cx.notify();
                    },
                ))
                // Both halves, as the workspace wires them: a drag that ends
                // with the pointer outside the window — which is where a thumb
                // dragged off the end of its track leaves it — is released by
                // the second.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|harness, _: &MouseUpEvent, _window, cx| {
                        harness.release(cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|harness, _: &MouseUpEvent, _window, cx| {
                        harness.release(cx);
                    }),
                )
                .child(
                    div()
                        .id("toolbar")
                        .occlude()
                        .window_control_area(WindowControlArea::Drag)
                        .on_mouse_down(MouseButton::Left, move |_, _, _| {
                            dragged.set(dragged.get() + 1)
                        })
                        .w_full()
                        .h(px(36.))
                        .child(
                            TabBar::new("tabs")
                                .tabs(self.tabs.clone())
                                .active(0)
                                .scroll_handle(&self.strip)
                                .scrollbar(self.scrollbar().on_hover(cx.listener(
                                    |harness, hovered: &bool, _window, cx| {
                                        harness.hover(*hovered, cx);
                                    },
                                )))
                                .on_select(move |_, _, _| selected.set(selected.get() + 1))
                                .on_close(move |_, _, _| closed.set(closed.get() + 1)),
                        ),
                )
                .child(
                    div()
                        .id("reference")
                        .flex()
                        .flex_row()
                        .w_full()
                        .h(px(36.))
                        .overflow_x_scroll()
                        .track_scroll(&self.reference)
                        .child(div().flex_none().w(px(REFERENCE_CONTENT)).h_full()),
                )
        }
    }

    /// Everything a test needs to read back out of a running harness.
    struct Handles {
        tally: Tally,
        strip: ScrollHandle,
        reference: ScrollHandle,
        /// What the bar was doing the last time the harness drew.
        fade: Rc<Cell<Fade>>,
    }

    /// Opens a window on a strip of `tabs` and hands back its handles.
    fn open(cx: &mut TestAppContext, tabs: Vec<TabItem>) -> (Handles, VisualTestContext) {
        let handles = Handles {
            tally: Tally::default(),
            strip: ScrollHandle::new(),
            reference: ScrollHandle::new(),
            fade: Rc::new(Cell::new(Fade::Hidden)),
        };

        let window = cx.add_window({
            let tally = handles.tally.clone();
            let strip = handles.strip.clone();
            let reference = handles.reference.clone();
            let fade = handles.fade.clone();
            move |_, _| Harness {
                tally,
                tabs,
                strip,
                bar: ScrollbarState::new(),
                reference,
                fade,
            }
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        // A bar is built from the strip as the previous frame measured it, so
        // the opening frame has nothing to build one out of. One more draw is
        // what a window gets in the ordinary run of things, and it is what gives
        // the tests below a bar to point at.
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        (handles, cx)
    }

    /// One tab, which is all the click tests need.
    fn one_tab() -> Vec<TabItem> {
        vec![TabItem::new("tab-0", "one")]
    }

    /// Enough tabs that the strip overflows and can be scrolled.
    fn many_tabs() -> Vec<TabItem> {
        (0..CROWDED)
            .map(|index| TabItem::new(ElementId::from(("tab", index)), format!("tab {index}")))
            .collect()
    }

    /// Turns a wheel over `position`, hovering it first so the hit test is
    /// settled by the time the wheel arrives.
    fn turn_the_wheel(cx: &mut VisualTestContext, position: Point<Pixels>, delta: ScrollDelta) {
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta,
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
    }

    /// What a single clicked column answered with.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Answer {
        /// The tab took the click and asked to be selected.
        Select,
        /// The close button took the click.
        Close,
        /// The press reached the drag area behind the strip.
        Drag,
        /// Nothing at all answered.
        Nothing,
    }

    /// Clicks every column of the strip in turn, reporting what each answered.
    ///
    /// Sweeping rather than aiming: the close button's x depends on how wide the
    /// test platform draws the title, which is not something a test should
    /// pretend to know. Each column is hovered before it is clicked, because the
    /// button only exists once the pointer is on the tab.
    fn sweep_the_strip(cx: &mut TestAppContext) -> Vec<Answer> {
        let (handles, mut cx) = open(cx, one_tab());
        let tally = handles.tally;

        let mut answers = Vec::new();
        let mut seen = (0, 0, 0);
        for x in 0..SWEEP_WIDTH {
            let position = point(px(x as f32), px(ROW_MIDDLE));
            cx.simulate_mouse_move(position, None, Modifiers::none());
            cx.simulate_click(position, Modifiers::none());

            let now = (
                tally.selected.get(),
                tally.closed.get(),
                tally.dragged.get(),
            );
            answers.push(match (now.0 - seen.0, now.1 - seen.1, now.2 - seen.2) {
                (0, 0, 0) => Answer::Nothing,
                (1, 0, 0) => Answer::Select,
                (0, 1, 0) => Answer::Close,
                (0, 0, 1) => Answer::Drag,
                other => panic!("column {x} answered more than once: {other:?}"),
            });
            seen = now;
        }

        answers
    }

    /// The regression this file exists to hold on to: the close button used to
    /// occlude, which cut its own tab out of the hit test and so out of the
    /// `group_hover` that reveals it — leaving a button that hid itself from
    /// under the pointer and answered no click at all.
    /// The two badges a tab can wear are independent of one another, and the
    /// builder keeps both the icon and the words that name it.
    #[test]
    fn a_tab_can_wear_a_dot_and_a_mark_at_once() {
        assert!(TabItem::new("only", "tab").mark.is_none());

        let tab = TabItem::new("only", "tab")
            .status(TabStatus::Connected)
            .mark("icons/pin.svg", "held open");
        assert!(tab.status.is_some(), "the dot is untouched by the mark");
        let mark = tab.mark.expect("the mark is kept");
        assert_eq!(mark.icon.as_ref(), "icons/pin.svg");
        assert_eq!(mark.tooltip.as_ref(), "held open");
    }

    /// A marked tab is still a tab: the icon behind the title takes neither the
    /// press nor the space the title was being hit in.
    #[gpui::test]
    fn a_tab_wearing_a_mark_still_takes_a_click(cx: &mut TestAppContext) {
        let tabs = vec![
            TabItem::new("only", "tab")
                .status(TabStatus::Connected)
                .mark("icons/pin.svg", "held open"),
        ];
        let (handles, mut cx) = open(cx, tabs);

        let position = point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.simulate_click(position, Modifiers::none());

        assert_eq!(handles.tally.selected.get(), 1, "the tab answered");
        assert_eq!(
            handles.tally.dragged.get(),
            0,
            "and the press stopped at the tab rather than reaching the strip"
        );
    }

    #[gpui::test]
    fn the_close_button_takes_a_click_inside_a_drag_area(cx: &mut TestAppContext) {
        let answers = sweep_the_strip(cx);

        assert!(
            answers.contains(&Answer::Close),
            "no column of the tab closed it: {answers:?}"
        );
        assert!(
            answers.contains(&Answer::Select),
            "no column of the tab selected it: {answers:?}"
        );
    }

    #[gpui::test]
    fn a_permanent_tab_can_be_selected_but_has_no_close_target(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(
            cx,
            vec![TabItem::new("generate", "Generate").closable(false)],
        );

        for x in 0..SWEEP_WIDTH {
            let position = point(px(x as f32), px(ROW_MIDDLE));
            cx.simulate_mouse_move(position, None, Modifiers::none());
            cx.simulate_click(position, Modifiers::none());
        }

        assert!(
            handles.tally.selected.get() > 0,
            "the permanent tab is selectable"
        );
        assert_eq!(
            handles.tally.closed.get(),
            0,
            "the permanent tab has no close target"
        );
    }

    /// A wheel puts the overlay bar up, and it comes up over the tabs rather
    /// than beside them: the strip is the same height with it as without.
    #[gpui::test]
    fn scrolling_the_strip_shows_the_overlay_bar(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        assert!(
            handles.fade.get() == Fade::Hidden,
            "the bar was up before anything moved"
        );

        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -3.)),
        );

        assert_eq!(
            handles.fade.get(),
            Fade::In,
            "scrolling did not fade the bar in"
        );
    }

    /// And it goes away again on its own: up for the linger that tells a stopped
    /// wheel from a paused one, then a fade during which it is still drawn, and
    /// only then gone.
    #[gpui::test]
    fn the_overlay_bar_fades_out_once_the_scrolling_stops(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -3.)),
        );

        cx.executor().advance_clock(SCROLL_LINGER / 2);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::In,
            "the bar started going before its time was up"
        );

        // Past the expiry but not past the fade it starts, which is the whole
        // window this test is about.
        cx.executor().advance_clock(SCROLL_LINGER / 2 + A_MOMENT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Out,
            "the bar did not start fading when its time was up"
        );

        cx.executor().advance_clock(FADE_OUT / 2);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Out,
            "the bar vanished part way through its fade instead of fading"
        );

        cx.executor().advance_clock(FADE_OUT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar never finished going"
        );
    }

    /// A pointer reaching the edge the bar rides brings it up with nothing
    /// having scrolled, holds it there for as long as it stays, and lets it go
    /// the moment it leaves — no linger, because a pointer leaving says so.
    #[gpui::test]
    fn the_pointer_on_the_edge_brings_the_bar_up_and_takes_it_away(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        let on_the_edge = point(px(INSIDE_FIRST_TAB), px(BAR_MIDDLE));
        let off_it = point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE));
        assert_eq!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar was up before anything asked for it"
        );

        cx.simulate_mouse_move(on_the_edge, None, Modifiers::none());
        assert_eq!(
            handles.fade.get(),
            Fade::In,
            "the pointer on the edge did not bring the bar up"
        );

        cx.executor().advance_clock(SCROLL_LINGER * 4);
        cx.run_until_parked();
        assert_ne!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar went while the pointer was still on it"
        );

        cx.simulate_mouse_move(off_it, None, Modifiers::none());
        assert_eq!(
            handles.fade.get(),
            Fade::Out,
            "the bar waited to start going after the pointer had left"
        );

        cx.executor().advance_clock(FADE_OUT + A_MOMENT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar never finished going"
        );
    }

    /// Scrolling again while it is on its way out catches it: back to full
    /// strength, and the fade it interrupted never completes.
    #[gpui::test]
    fn scrolling_during_the_fade_catches_the_bar(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        let wheel = point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE));
        turn_the_wheel(&mut cx, wheel, ScrollDelta::Lines(point(0., -3.)));

        cx.executor().advance_clock(SCROLL_LINGER + A_MOMENT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Out,
            "the bar was not on its way out"
        );

        turn_the_wheel(&mut cx, wheel, ScrollDelta::Lines(point(0., -3.)));
        assert_eq!(
            handles.fade.get(),
            Fade::Shown,
            "a bar caught mid-fade did not come back at full strength"
        );

        // The interrupted fade must not finish behind the new showing's back.
        // Just past where it was due, and short of the linger the second wheel
        // armed, so the only thing that could move the bar here is the fade that
        // was called off.
        cx.executor().advance_clock(FADE_OUT + A_MOMENT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Shown,
            "the fade that was interrupted completed anyway"
        );
    }

    /// A bar on its way out can still be caught by the pointer, because it can
    /// still be seen — and catching it cancels the fade rather than letting it
    /// go on going while it is being dragged.
    #[gpui::test]
    fn a_fading_bar_can_still_be_grabbed(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        let wheel = point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE));
        turn_the_wheel(&mut cx, wheel, ScrollDelta::Lines(point(0., -1.)));

        let bar = Scrollbar::for_handle(BAR, ScrollbarAxis::Horizontal, &handles.strip)
            .thumb()
            .expect("an overflowing strip");
        cx.executor().advance_clock(SCROLL_LINGER + A_MOMENT);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Out,
            "the bar was not on its way out"
        );

        let before = handles.strip.offset().x;
        let grab = point(bar.start + px(4.), px(BAR_MIDDLE));
        cx.simulate_mouse_move(grab, None, Modifiers::none());
        cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        for step in 1..=2 {
            cx.simulate_mouse_move(
                point(grab.x + px(step as f32 * 40.), grab.y),
                Some(MouseButton::Left),
                Modifiers::none(),
            );
        }

        assert!(
            handles.strip.offset().x < before,
            "a fading thumb did not answer the pointer that caught it"
        );
        assert_eq!(
            handles.fade.get(),
            Fade::Shown,
            "catching the thumb did not cancel the fade"
        );
    }

    /// The thumb takes the strip wherever it is dragged, and keeps the point it
    /// was grabbed by under the pointer while it does.
    #[gpui::test]
    fn dragging_the_thumb_scrolls_the_strip(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -1.)),
        );
        assert_ne!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar has to be up to be dragged"
        );

        let bar = Scrollbar::for_handle(BAR, ScrollbarAxis::Horizontal, &handles.strip)
            .thumb()
            .expect("an overflowing strip");
        let track = handles.strip.bounds().size.width;
        let before = handles.strip.offset().x;

        // Down on the thumb, then past the threshold gpui uses to tell a drag
        // from a click, then a little way along, and finally clear off the end
        // of the track — which should leave the strip pinned to its end rather
        // than running past it.
        let grab = point(bar.start + px(4.), px(BAR_MIDDLE));
        cx.simulate_mouse_move(grab, None, Modifiers::none());
        cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(grab.x + px(40.), grab.y),
            Some(MouseButton::Left),
            Modifiers::none(),
        );

        let halfway = point(bar.start + (track - bar.length) / 2. + px(4.), grab.y);
        cx.simulate_mouse_move(halfway, Some(MouseButton::Left), Modifiers::none());
        let dragged = handles.strip.offset().x;
        assert!(
            dragged < before,
            "dragging the thumb left the strip where it was"
        );

        let past_the_end = point(track * 4., grab.y);
        cx.simulate_mouse_move(past_the_end, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(past_the_end, MouseButton::Left, Modifiers::none());

        assert_eq!(
            handles.strip.offset().x,
            -handles.strip.max_offset().x,
            "dragging the thumb off the end did not reach the end"
        );

        // Letting go starts the clock again, rather than leaving the bar up for
        // good — which is what a hold that is never released would do.
        assert_eq!(
            handles.fade.get(),
            Fade::Shown,
            "the bar was not held at full strength through the drag"
        );
        cx.executor().advance_clock(SCROLL_LINGER * 2);
        cx.run_until_parked();
        assert_eq!(
            handles.fade.get(),
            Fade::Hidden,
            "the bar stayed up after the thumb was let go"
        );
    }

    /// A pointer that takes hold of the thumb and then keeps perfectly still
    /// keeps the bar up, however long it holds: a motionless drag scrolls
    /// nothing, and nothing scrolling is otherwise what takes a bar down.
    #[gpui::test]
    fn a_held_thumb_outlasts_the_clock(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -1.)),
        );

        let bar = Scrollbar::for_handle(BAR, ScrollbarAxis::Horizontal, &handles.strip)
            .thumb()
            .expect("an overflowing strip");
        let grab = point(bar.start + px(4.), px(BAR_MIDDLE));
        cx.simulate_mouse_move(grab, None, Modifiers::none());
        cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        // Twice: gpui turns the press into a drag on the first move past its
        // threshold, and only reports the moves after that one.
        for step in 1..=2 {
            cx.simulate_mouse_move(
                point(grab.x + px(step as f32 * 20.), grab.y),
                Some(MouseButton::Left),
                Modifiers::none(),
            );
        }

        cx.executor().advance_clock(SCROLL_LINGER * 5);
        cx.run_until_parked();

        assert_eq!(
            handles.fade.get(),
            Fade::Shown,
            "the bar did not stay at full strength under a still pointer"
        );
    }

    /// The thumb is the only part of the bar that takes a press: the track it
    /// slides along is not drawn, so a click beside it reaches the tab under it
    /// exactly as it did before the bar existed.
    #[gpui::test]
    fn the_track_beside_the_thumb_still_reaches_the_tabs(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());
        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -1.)),
        );
        assert!(
            handles.fade.get() != Fade::Hidden,
            "the bar has to be up to be in the way"
        );

        let bar = Scrollbar::for_handle(BAR, ScrollbarAxis::Horizontal, &handles.strip)
            .thumb()
            .expect("an overflowing strip");
        let beyond = bar.start + bar.length + px(40.);
        assert!(
            beyond < handles.strip.bounds().size.width,
            "the thumb filled the track, leaving no bare track to test"
        );

        let selected = handles.tally.selected.get();
        let position = point(beyond, px(BAR_MIDDLE));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.simulate_click(position, Modifiers::none());

        assert_eq!(
            handles.tally.selected.get(),
            selected + 1,
            "a click on the bare track did not reach the tab under it"
        );

        // And the thumb itself is the exception that makes the rule worth
        // stating: a press there belongs to the bar, and the tab under it must
        // not also answer. On Windows the same occlusion is what keeps the
        // press off the title bar's caption hit test.
        let selected = handles.tally.selected.get();
        let on_the_thumb = point(bar.start + bar.length / 2., px(BAR_MIDDLE));
        cx.simulate_mouse_move(on_the_thumb, None, Modifiers::none());
        cx.simulate_click(on_the_thumb, Modifiers::none());

        assert_eq!(
            handles.tally.selected.get(),
            selected,
            "a press on the thumb also selected the tab under it"
        );
        assert_eq!(
            handles.tally.dragged.get(),
            0,
            "a press on the thumb reached the window drag area"
        );
    }

    /// The other half of the same balance, and what the tab's own occlusion is
    /// for: no column of a tab may let the press through to the drag area, or
    /// the title bar takes it and moves the window instead of switching tabs.
    /// Past the last tab the strip is bare, and there the drag area should
    /// answer.
    #[gpui::test]
    fn the_drag_area_answers_only_past_the_last_tab(cx: &mut TestAppContext) {
        let answers = sweep_the_strip(cx);

        let last_tab_column = answers
            .iter()
            .rposition(|answer| matches!(answer, Answer::Select | Answer::Close))
            .expect("the sweep never landed on the tab");
        let first_drag_column = answers
            .iter()
            .position(|answer| *answer == Answer::Drag)
            .expect("the sweep never landed on the bare strip");

        assert!(
            first_drag_column > last_tab_column,
            "the drag area answered a press aimed at a tab: {answers:?}"
        );
    }

    /// The other cost of the tab's occlusion: it hides the scrolling row from
    /// the hit test wherever a tab covers it, which is nearly all of a crowded
    /// strip, so gpui's own wheel handling never ran. The tab answers the wheel
    /// itself now.
    #[gpui::test]
    fn a_wheel_over_a_tab_scrolls_the_strip(cx: &mut TestAppContext) {
        let (handles, mut cx) = open(cx, many_tabs());

        assert!(
            handles.strip.max_offset().x > px(0.),
            "the strip did not overflow, so there was nothing to scroll"
        );
        assert_eq!(
            handles.strip.offset().x,
            px(0.),
            "the strip started scrolled"
        );

        turn_the_wheel(
            &mut cx,
            point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)),
            ScrollDelta::Lines(point(0., -3.)),
        );

        assert!(
            handles.strip.offset().x < px(0.),
            "a wheel over a tab left the strip where it was"
        );
    }

    /// And it answers it the way the row itself would have. The reference row
    /// below the strip is plain gpui scrolling in the same inherited text style,
    /// so a wheel delta has to land both rows in the same place — otherwise a
    /// wheel would carry further over a tab than over the bare strip beside it.
    ///
    /// Both a line delta and a pixel delta, because only the first is converted
    /// through the line height, which is exactly what is easy to get wrong.
    #[gpui::test]
    fn a_wheel_moves_a_tab_and_plain_gpui_the_same_distance(cx: &mut TestAppContext) {
        for delta in [
            ScrollDelta::Lines(point(0., -3.)),
            ScrollDelta::Lines(point(-2., 0.)),
            ScrollDelta::Pixels(point(px(-40.), px(0.))),
            ScrollDelta::Pixels(point(px(0.), px(-40.))),
        ] {
            let (handles, mut cx) = open(cx, many_tabs());

            turn_the_wheel(&mut cx, point(px(INSIDE_FIRST_TAB), px(ROW_MIDDLE)), delta);
            turn_the_wheel(
                &mut cx,
                point(px(INSIDE_FIRST_TAB), px(REFERENCE_MIDDLE)),
                delta,
            );

            assert_eq!(
                handles.strip.offset().x,
                handles.reference.offset().x,
                "a {delta:?} moved the strip and plain gpui different distances"
            );
            assert!(
                handles.strip.offset().x < px(0.),
                "a {delta:?} moved neither row, so they agreed on nothing"
            );
        }
    }

    /// Tag of the one strip the drop harness draws.
    const STRIP: u64 = 7;

    /// What the drop harness heard from its strip.
    #[derive(Clone, Default)]
    struct DropLog {
        /// Every report of `on_drag_over`, in order.
        reports: Rc<RefCell<Vec<Option<usize>>>>,
        /// Every drop `on_reorder` reported, in order.
        drops: Rc<RefCell<Vec<(DraggedTab, usize)>>>,
    }

    /// A strip that drags and takes drops, wired the way an app wires one:
    /// a [`TabDropState`] behind the indicator, the dropdown and the "+" beside
    /// the tabs, and the root clearing the bar on any release.
    struct DropHarness {
        tabs: Vec<TabItem>,
        strip: ScrollHandle,
        drop: TabDropState,
        log: DropLog,
    }

    impl Render for DropHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity().downgrade();
            let log = self.log.clone();
            div()
                .size_full()
                .text_size(px(INHERITED_TEXT))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|harness, _: &MouseUpEvent, _window, cx| {
                        if harness.drop.clear() {
                            cx.notify();
                        }
                    }),
                )
                .child(
                    TabBar::new("tabs")
                        .tabs(self.tabs.clone())
                        .scroll_handle(&self.strip)
                        .drag_source(STRIP)
                        .drop_indicator(self.drop.indicator(STRIP))
                        .on_menu_open_change(|_, _, _| {})
                        .on_new(|_, _| {})
                        .on_drag_over({
                            let this = this.clone();
                            let reports = log.reports.clone();
                            move |dragged, index, _window, cx| {
                                reports.borrow_mut().push(index);
                                let _ = this.update(cx, |harness, cx| {
                                    if harness
                                        .drop
                                        .drag_over(STRIP, index, dragged.source == STRIP)
                                    {
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .on_reorder({
                            let drops = log.drops.clone();
                            move |dragged, to, _window, cx| {
                                drops.borrow_mut().push((dragged, to));
                                let _ = this.update(cx, |harness, cx| {
                                    harness.drop.clear();
                                    cx.notify();
                                });
                            }
                        }),
                )
        }
    }

    /// Opens a window on a three-tab drop harness.
    fn open_drop(
        cx: &mut TestAppContext,
    ) -> (
        DropLog,
        ScrollHandle,
        Entity<DropHarness>,
        VisualTestContext,
    ) {
        let log = DropLog::default();
        let strip = ScrollHandle::new();
        let window = cx.add_window({
            let log = log.clone();
            let strip = strip.clone();
            move |_, _| DropHarness {
                tabs: (0..3usize)
                    .map(|index| {
                        TabItem::new(ElementId::from(("tab", index)), format!("tab {index}"))
                    })
                    .collect(),
                strip,
                drop: TabDropState::new(),
                log,
            }
        });
        let entity = window.root(cx).expect("the harness window");
        let cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        (log, strip, entity, cx)
    }

    /// Where tab `index` lies, as the last frame laid it out.
    fn tab_bounds(strip: &ScrollHandle, index: usize) -> Bounds<Pixels> {
        strip.bounds_for_item(index).expect("a laid-out tab")
    }

    /// Presses on tab `from` and drags it to `to`, without letting go.
    ///
    /// Twice at the end: gpui turns the press into a drag on the first move
    /// past its threshold, and only reports the moves after that one.
    fn drag_tab(cx: &mut VisualTestContext, strip: &ScrollHandle, from: usize, to: Point<Pixels>) {
        let grab = tab_bounds(strip, from).center();
        cx.simulate_mouse_move(grab, None, Modifiers::none());
        cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    }

    /// A point on the left (`right = false`) or right quarter of tab `index`.
    fn on_half(strip: &ScrollHandle, index: usize, right: bool) -> Point<Pixels> {
        let bounds = tab_bounds(strip, index);
        let quarter = bounds.size.width / 4.;
        let x = if right {
            bounds.right() - quarter
        } else {
            bounds.left() + quarter
        };
        point(x, px(ROW_MIDDLE))
    }

    /// The regression: a drop on a tab's right half used to draw the bar
    /// after the tab and report the tab itself to `on_reorder`, so the tab
    /// moved one place short of where the bar said. Both halves of a tab now
    /// land where the bar stands.
    #[gpui::test]
    fn a_drop_lands_in_the_gap_the_bar_shows(cx: &mut TestAppContext) {
        for (right, gap) in [(false, 1), (true, 2)] {
            let (log, strip, harness, mut cx) = open_drop(cx);
            let target = on_half(&strip, 1, right);
            drag_tab(&mut cx, &strip, 0, target);

            assert_eq!(
                log.reports.borrow().last().copied(),
                Some(Some(gap)),
                "the hover named the wrong gap"
            );
            assert_eq!(
                harness.read_with(&cx, |harness, _| harness.drop.indicator(STRIP)),
                Some(gap),
                "the owner's bar is not in the hovered gap"
            );

            cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::none());
            assert_eq!(
                log.drops.borrow().as_slice(),
                &[(
                    DraggedTab {
                        source: STRIP,
                        index: 0
                    },
                    gap
                )],
                "the drop landed somewhere other than the bar"
            );
            assert_eq!(
                harness.read_with(&cx, |harness, _| harness.drop.current()),
                None,
                "the bar outlived the drop"
            );
        }
    }

    /// Past the last tab the strip's length is named, and a drop there
    /// appends.
    #[gpui::test]
    fn a_drop_past_the_last_tab_appends(cx: &mut TestAppContext) {
        let (log, strip, _harness, mut cx) = open_drop(cx);
        let past = point(tab_bounds(&strip, 2).right() + px(20.), px(ROW_MIDDLE));
        drag_tab(&mut cx, &strip, 0, past);
        assert_eq!(log.reports.borrow().last().copied(), Some(Some(3)));

        cx.simulate_mouse_up(past, MouseButton::Left, Modifiers::none());
        assert_eq!(log.drops.borrow().last().map(|(_, to)| *to), Some(3));
    }

    /// One report per move, and the final one: the empty stretch's
    /// provisional answer, which a tab under the pointer corrects, never
    /// reaches the owner.
    #[gpui::test]
    fn the_owner_hears_one_settled_report_per_move(cx: &mut TestAppContext) {
        let (log, strip, _harness, mut cx) = open_drop(cx);
        drag_tab(&mut cx, &strip, 0, on_half(&strip, 1, true));
        let before = log.reports.borrow().len();

        cx.simulate_mouse_move(
            on_half(&strip, 2, false),
            Some(MouseButton::Left),
            Modifiers::none(),
        );

        let reports = log.reports.borrow();
        assert_eq!(&reports[before..], &[Some(2)], "{reports:?}");
        assert!(
            !reports.contains(&Some(3)),
            "the provisional end-of-strip gap leaked out: {reports:?}"
        );
    }

    /// The dropdown and the "+" sit inside the bar but outside the scrolling
    /// row, and take no drop: a pointer moving onto them takes the bar down
    /// rather than leaving it standing in the last gap, and a release there
    /// moves nothing.
    #[gpui::test]
    fn the_bar_does_not_linger_over_the_buttons(cx: &mut TestAppContext) {
        let (log, strip, harness, mut cx) = open_drop(cx);
        drag_tab(&mut cx, &strip, 0, on_half(&strip, 1, true));
        assert_eq!(
            harness.read_with(&cx, |harness, _| harness.drop.indicator(STRIP)),
            Some(2)
        );

        let width = cx.update(|window, _| window.viewport_size().width);
        let on_the_plus = point(width - px(18.), px(ROW_MIDDLE));
        cx.simulate_mouse_move(on_the_plus, Some(MouseButton::Left), Modifiers::none());

        assert_eq!(log.reports.borrow().last().copied(), Some(None));
        assert_eq!(
            harness.read_with(&cx, |harness, _| harness.drop.indicator(STRIP)),
            None,
            "the bar stayed up over the \"+\""
        );

        cx.simulate_mouse_up(on_the_plus, MouseButton::Left, Modifiers::none());
        assert!(
            log.drops.borrow().is_empty(),
            "a drop on the \"+\" moved a tab"
        );
    }

    /// The bar lays over the tabs instead of taking room between them, so
    /// raising it moves no tab — a bar with a width of its own would push the
    /// hovered tab aside, flip the half under the pointer, and flicker.
    #[gpui::test]
    fn the_bar_shifts_no_tab(cx: &mut TestAppContext) {
        let (_log, strip, harness, mut cx) = open_drop(cx);
        let before: Vec<_> = (0..3).map(|index| tab_bounds(&strip, index)).collect();

        drag_tab(&mut cx, &strip, 0, on_half(&strip, 1, false));
        assert_eq!(
            harness.read_with(&cx, |harness, _| harness.drop.indicator(STRIP)),
            Some(1),
            "the bar has to be up to be in the way"
        );
        cx.run_until_parked();

        let after: Vec<_> = (0..3).map(|index| tab_bounds(&strip, index)).collect();
        assert_eq!(before, after, "raising the bar moved the tabs");
    }

    /// The halves of a tab name the gaps either side of it, the middle
    /// itself counting as the right half.
    #[test]
    fn a_tabs_halves_name_the_gaps_beside_it() {
        let (left, right) = (px(100.), px(200.));
        assert_eq!(gap_over_tab(3, left, right, px(100.)), 3);
        assert_eq!(gap_over_tab(3, left, right, px(149.)), 3);
        assert_eq!(gap_over_tab(3, left, right, px(150.)), 4);
        assert_eq!(gap_over_tab(3, left, right, px(199.)), 4);
    }

    /// The bookkeeping the apps used to copy: a strip's `None` takes down only
    /// its own bar, a refused tab raises none, and a drop resolves to the bar
    /// of its own strip or to the fallback.
    #[test]
    fn the_drop_state_keeps_one_strip_s_bar() {
        let mut state = TabDropState::new();
        assert!(
            !state.drag_over(1, Some(2), false),
            "a refused tab raised the bar"
        );
        assert!(state.drag_over(1, Some(2), true));
        assert!(
            !state.drag_over(1, Some(2), true),
            "an unchanged bar is no change"
        );
        assert_eq!(state.indicator(1), Some(2));
        assert_eq!(state.indicator(9), None);

        assert!(
            !state.drag_over(9, None, true),
            "another strip's None took the bar"
        );
        assert_eq!(state.current(), Some((1, 2)));
        assert!(state.drag_over(1, None, true));
        assert_eq!(state.current(), None);

        state.drag_over(1, Some(3), true);
        assert_eq!(
            state.resolve(9, 0),
            0,
            "a drop elsewhere took this bar's gap"
        );
        assert_eq!(state.current(), None, "the drop did not end the gesture");
        state.drag_over(1, Some(3), true);
        assert_eq!(state.resolve(1, 0), 3);
        assert!(!state.clear());
    }

    /// A strip the keyboard drives, whose owner applies every request at once
    /// the way an app would: a select moves the selection, a close removes the
    /// tab, and a reorder moves it with the selection riding along.
    struct KeyHarness {
        titles: Vec<&'static str>,
        closable: Vec<bool>,
        active: usize,
        focus: FocusHandle,
    }

    impl Render for KeyHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity().downgrade();
            let tabs = self
                .titles
                .iter()
                .zip(&self.closable)
                .map(|(title, closable)| TabItem::new(*title, *title).closable(*closable))
                .collect();
            div().size_full().child(
                TabBar::new("tabs")
                    .tabs(tabs)
                    .active(self.active)
                    .focus_handle(&self.focus)
                    .drag_source(STRIP)
                    .on_select({
                        let this = this.clone();
                        move |index, _, cx| {
                            let _ = this.update(cx, |harness, cx| {
                                harness.active = index;
                                cx.notify();
                            });
                        }
                    })
                    .on_close({
                        let this = this.clone();
                        move |index, _, cx| {
                            let _ = this.update(cx, |harness, cx| {
                                harness.titles.remove(index);
                                harness.closable.remove(index);
                                harness.active = harness.active.min(harness.titles.len() - 1);
                                cx.notify();
                            });
                        }
                    })
                    .on_reorder(move |dragged, to, _, cx| {
                        let _ = this.update(cx, |harness, cx| {
                            assert_eq!(dragged.source, STRIP, "a reorder from a stranger");
                            let from = dragged.index;
                            let at = if from < to { to - 1 } else { to };
                            let title = harness.titles.remove(from);
                            let closable = harness.closable.remove(from);
                            harness.titles.insert(at, title);
                            harness.closable.insert(at, closable);
                            harness.active = at;
                            cx.notify();
                        });
                    }),
            )
        }
    }

    /// Opens a focused strip of `titles`, the first of them permanent.
    fn open_keys(
        cx: &mut TestAppContext,
        titles: Vec<&'static str>,
    ) -> (Entity<KeyHarness>, VisualTestContext) {
        cx.update(init);
        let window = cx.add_window(|_, cx| KeyHarness {
            closable: (0..titles.len()).map(|index| index != 0).collect(),
            titles,
            active: 0,
            focus: cx.focus_handle(),
        });
        let entity = window.root(cx).expect("the harness window");
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.update(|window, cx| entity.read(cx).focus.clone().focus(window, cx));
        cx.run_until_parked();
        (entity, cx)
    }

    /// The titles in display order, and the active one.
    fn key_state(
        harness: &Entity<KeyHarness>,
        cx: &mut VisualTestContext,
    ) -> (Vec<&'static str>, usize) {
        harness.read_with(cx, |harness, _| (harness.titles.clone(), harness.active))
    }

    /// The platform's command key, as [`init`] binds the reorder chords.
    fn command() -> &'static str {
        if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        }
    }

    /// The arrows walk the selection and stop at the ends; Home and End jump
    /// to them.
    #[gpui::test]
    fn the_arrows_walk_the_tabs(cx: &mut TestAppContext) {
        let (harness, mut cx) = open_keys(cx, vec!["a", "b", "c"]);

        cx.simulate_keystrokes("left");
        assert_eq!(
            key_state(&harness, &mut cx).1,
            0,
            "walked off the first tab"
        );
        cx.simulate_keystrokes("right right");
        assert_eq!(key_state(&harness, &mut cx).1, 2);
        cx.simulate_keystrokes("right");
        assert_eq!(key_state(&harness, &mut cx).1, 2, "walked off the last tab");
        cx.simulate_keystrokes("home");
        assert_eq!(key_state(&harness, &mut cx).1, 0);
        cx.simulate_keystrokes("end");
        assert_eq!(key_state(&harness, &mut cx).1, 2);
        cx.simulate_keystrokes("left");
        assert_eq!(key_state(&harness, &mut cx).1, 1);
    }

    /// Delete closes the active tab, and leaves a permanent one alone.
    #[gpui::test]
    fn delete_closes_the_active_tab_unless_it_is_permanent(cx: &mut TestAppContext) {
        let (harness, mut cx) = open_keys(cx, vec!["a", "b", "c"]);

        cx.simulate_keystrokes("delete");
        assert_eq!(
            key_state(&harness, &mut cx).0,
            vec!["a", "b", "c"],
            "the permanent tab was closed"
        );

        cx.simulate_keystrokes("right delete");
        assert_eq!(key_state(&harness, &mut cx), (vec!["a", "c"], 1));
    }

    /// The reorder chords move the active tab one place at a time, the
    /// selection riding along, and stop at the ends.
    #[gpui::test]
    fn the_reorder_chords_move_the_active_tab(cx: &mut TestAppContext) {
        let (harness, mut cx) = open_keys(cx, vec!["a", "b", "c"]);
        let right = format!("{}-shift-pagedown", command());
        let left = format!("{}-shift-pageup", command());

        cx.simulate_keystrokes(&right);
        assert_eq!(key_state(&harness, &mut cx), (vec!["b", "a", "c"], 1));
        cx.simulate_keystrokes(&right);
        assert_eq!(key_state(&harness, &mut cx), (vec!["b", "c", "a"], 2));
        cx.simulate_keystrokes(&right);
        assert_eq!(
            key_state(&harness, &mut cx),
            (vec!["b", "c", "a"], 2),
            "the last tab moved past the end"
        );
        cx.simulate_keystrokes(&left);
        assert_eq!(key_state(&harness, &mut cx), (vec!["b", "a", "c"], 1));
    }

    /// The keys belong to the focused strip alone: unfocused, the arrows do
    /// nothing to it.
    #[gpui::test]
    fn an_unfocused_strip_ignores_the_keys(cx: &mut TestAppContext) {
        let (harness, mut cx) = open_keys(cx, vec!["a", "b", "c"]);
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();

        cx.simulate_keystrokes("right");
        assert_eq!(key_state(&harness, &mut cx).1, 0);
    }

    /// The arithmetic behind the keys, at the edges included.
    #[test]
    fn key_requests_stop_at_the_ends() {
        use KeyCommand::*;
        assert_eq!(key_request(Prev, 0, 3), None);
        assert_eq!(key_request(Next, 2, 3), None);
        assert_eq!(key_request(First, 0, 3), None);
        assert_eq!(key_request(Last, 2, 3), None);
        assert_eq!(key_request(MoveLeft, 0, 3), None);
        assert_eq!(key_request(MoveRight, 2, 3), None);
        assert_eq!(key_request(Next, 0, 0), None, "an empty strip");
        assert_eq!(
            key_request(MoveRight, 0, 3),
            Some(KeyRequest::Move { from: 0, to: 2 })
        );
        assert_eq!(
            key_request(MoveLeft, 2, 3),
            Some(KeyRequest::Move { from: 2, to: 1 })
        );
        assert_eq!(key_request(Close, 1, 3), Some(KeyRequest::Close(1)));
    }
}
