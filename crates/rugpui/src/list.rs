//! A virtualised flat list whose rows the host draws itself.
//!
//! The same bargain [`tree`](crate::tree) strikes, with the hierarchy taken
//! out. The widget owns the shape of the list — how tall a row is, which one is
//! selected, where the viewport sits — while the host owns the items and hands
//! them over through [`ListSource`], keyed by an id the host invents. What is
//! *inside* a row is never the list's business: a contact card, a saved query,
//! a connection with a status dot are all the same widget, because the only
//! thing the list draws inside a row is the row the host handed back.
//!
//! ## Why a source rather than a vector of elements
//!
//! gpui's [`uniform_list`] lays out only the rows the viewport can reach, and
//! it can do that only if the rows are addressable by index and all the same
//! height. So the list asks for rows one index at a time instead of taking a
//! built column: ten items and ten thousand cost the same to draw, and the host
//! never builds an element nobody will see. The one height is
//! [`ListView::row_height`], set once for the whole list — a two-line card row
//! is a taller list, not a taller row.
//!
//! ## Why ids and not indices
//!
//! Selection is remembered by id and reported by id, for the reason a tree's
//! is: a host that filters, sorts or refetches its items is renumbering them,
//! and a selection kept as `3` would silently come back pointing at something
//! else. An id survives all three. Unlike a tree, though, a flat list has
//! nowhere for a row to be hiding — there is no closed branch it could be
//! inside — so an id the source no longer holds is not a selection that is
//! merely off screen, it is one that is gone, and the list drops it and says
//! so.

use std::hash::Hash;

use gpui::{
    AnyElement, App, ClickEvent, Context, DragMoveEvent, ElementId, EventEmitter, FocusHandle,
    Focusable, KeyBinding, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollHandle,
    ScrollStrategy, UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::scrollbar::{
    DraggedThumb, Scrollbar, ScrollbarAxis, ScrollbarState, hide_later, hide_now, scroll_to,
    scrolled,
};
use crate::theme::theme;

actions!(
    rugpui_list,
    [
        /// Move the selection to the row above.
        SelectPrev,
        /// Move the selection to the row below.
        SelectNext,
        /// Activate the selected row, which is what a double click does.
        Activate,
        /// Move the selection to the first row.
        SelectFirst,
        /// Move the selection to the last row.
        SelectLast,
    ]
);

/// Key context that [`init`] binds the keys above to.
const KEY_CONTEXT: &str = "ListView";

/// Height of one row until the host asks for another, matching a tree's.
///
/// A list of plain labels beside a tree of them should read as one surface, so
/// the two start from the same number; a list of cards overrides it with
/// [`ListView::row_height`].
const DEFAULT_ROW_HEIGHT: f32 = 24.;

/// Padding at both ends of a row.
const ROW_PADDING: f32 = 4.;

/// Registers the key bindings every [`ListView`] relies on.
///
/// Called by [`crate::init`]; scoped to the `ListView` key context, so the
/// arrows keep meaning what they meant everywhere else in the app.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Activate, Some(KEY_CONTEXT)),
        KeyBinding::new("space", Activate, Some(KEY_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(KEY_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(KEY_CONTEXT)),
    ]);
}

/// The state of the row a source is drawing into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListRowInfo {
    /// Where the row sits, counting from zero.
    ///
    /// Not an item identity — a host that reorders its items moves it — but an
    /// *element* identity: gpui asks for a stable [`ElementId`] before it will
    /// let anything stateful hang off an element, and a drag is stateful. The
    /// list already keys its own row container by this index, so a host that
    /// wants a draggable row can key alongside it rather than invent a second
    /// numbering.
    pub index: usize,
    /// Whether the row is the selected one.
    pub selected: bool,
}

/// What the list tells its host about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListEvent<Id> {
    /// A row was activated: `Enter`, `Space`, or a double click.
    ///
    /// A flat list has nothing of its own for a double click to do — no branch
    /// to open — so unlike a tree's, every double click on a row arrives here.
    Activated(Id),
    /// The selection moved, or was dropped because the source stopped holding
    /// it.
    SelectionChanged(Option<Id>),
    /// A row was right-clicked, and the host should open a menu over it.
    ///
    /// The list has already taken focus and moved the selection onto `id` — a
    /// [`ListEvent::SelectionChanged`] arrives first when that changed
    /// anything — so the commands the host puts in the menu can be the ones it
    /// already has for the selection, and the highlight shows which row the
    /// menu is about. The list draws no menu itself: the rows have no strings
    /// in them, so neither can their menu.
    ContextMenu {
        /// The item the pointer went down on.
        id: Id,
        /// Where the pointer was, in window coordinates, for the menu to hang
        /// from.
        position: Point<Pixels>,
    },
}

/// Where a [`ListView`] gets its items and how their rows are drawn.
///
/// Implemented on whatever the host already keeps its items in; the list owns
/// the value and hands it back through [`ListView::source_mut`], so there is one
/// copy of the data rather than two that can disagree.
pub trait ListSource: 'static {
    /// How the host names an item.
    ///
    /// The selection is remembered and reported by this rather than by a row
    /// number, which is what lets a host filter, sort or refetch its items
    /// without the highlight landing on a different one. Ids therefore have to
    /// be stable across such a change: a key or a qualified name, not a
    /// position.
    type Id: Clone + Eq + Hash + 'static;

    /// How many rows there are.
    ///
    /// Asked once per rebuild rather than once per row, so it may count; it
    /// may not fetch.
    fn len(&self) -> usize;

    /// The id of the row at `index`, which is below [`ListSource::len`].
    ///
    /// Called for every row the viewport can see, and again for each row of a
    /// linear scan when the list is looking for the selection, so it should be
    /// a lookup rather than a computation.
    fn id(&self, index: usize) -> Self::Id;

    /// Whether there are no rows at all.
    ///
    /// Defaults to `len() == 0` and exists for the convention's sake; the list
    /// itself asks [`ListSource::len`].
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Draws the whole inside of a row.
    ///
    /// The list has drawn the row's height, its horizontal padding and its
    /// hover or selection background, and nothing else — no icon column, no
    /// indent, no label. It is handed a [`ListRowInfo`] so that what it draws
    /// can follow the row's state without keeping a copy of it.
    fn render_item(
        &self,
        id: &Self::Id,
        info: ListRowInfo,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;

    /// Draws the list's area when there are no rows at all.
    ///
    /// The default is nothing, because the obvious default would be a sentence
    /// and this layer has no strings: "No items" in English inside a Korean
    /// application would be worse than an empty box. A host with a wording of
    /// its own — or a glyph, or an illustration and a button — overrides this.
    fn render_empty(&self, _window: &mut Window, _cx: &mut App) -> AnyElement {
        div().into_any_element()
    }
}

/// A list of rows the host supplies, drawn one viewport at a time.
///
/// Created as an entity and rendered as a child element, like
/// [`TreeView`](crate::TreeView):
///
/// ```ignore
/// let list = cx.new(|cx| ListView::new(Contacts::default(), cx).row_height(px(44.)));
/// cx.subscribe(&list, |view, list, event, cx| match event {
///     ListEvent::Activated(id) => view.open(id, cx),
///     ListEvent::SelectionChanged(_) => {}
///     ListEvent::ContextMenu { id, position } => view.open_menu(id, *position, cx),
/// })
/// .detach();
/// ```
///
/// The keys are `Up`/`Down` to move, `Enter` and `Space` to activate, `Home` and
/// `End` for the ends of the list. A click selects, a double click activates,
/// and a right-click selects and asks the host for a menu.
pub struct ListView<S: ListSource> {
    source: S,
    focus_handle: FocusHandle,
    /// The selected item, or `None` — including when the source stopped
    /// holding what was selected; see the module docs.
    selected: Option<S::Id>,
    /// Where [`ListView::selected`] sits, kept beside it rather than looked up
    /// per frame: finding an id is a linear scan of the source, and a draw
    /// asks about the selection once per visible row.
    ///
    /// `Some` exactly when `selected` is.
    selected_index: Option<usize>,
    /// [`ListSource::len`] as of the last rebuild.
    len: usize,
    row_height: Pixels,
    /// Whether the two fields above still describe the source.
    dirty: bool,
    scroll: UniformListScrollHandle,
    bar: ScrollbarState,
    /// Id of the overlay bar, made unique per entity so that two lists in one
    /// window do not answer each other's drags.
    bar_id: ElementId,
}

impl<S: ListSource> ListView<S> {
    /// A list over `source`, with nothing selected.
    pub fn new(source: S, cx: &mut Context<Self>) -> Self {
        Self {
            source,
            focus_handle: cx.focus_handle(),
            selected: None,
            selected_index: None,
            len: 0,
            row_height: px(DEFAULT_ROW_HEIGHT),
            dirty: true,
            scroll: UniformListScrollHandle::new(),
            bar: ScrollbarState::new(),
            bar_id: ElementId::from(("rugpui-list-scrollbar", cx.entity_id())),
        }
    }

    /// Draws every row `height` tall instead of the default 24 px.
    ///
    /// One number for the whole list, because that is the condition
    /// [`uniform_list`] virtualises under: a row two lines deep is a list of
    /// 44 px rows, not one row that grew.
    pub fn row_height(mut self, height: Pixels) -> Self {
        self.row_height = height;
        self
    }

    /// Places the list at `index` in the window's tab order.
    ///
    /// Lists without one stay out of the tab ring, as fields do.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.focus_handle = self.focus_handle.clone().tab_index(index).tab_stop(true);
        self
    }

    /// The source, to read.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// The source, to change — dropping newly arrived items in, most of the
    /// time.
    ///
    /// Rereads it on the next draw, so the caller has nothing to remember. A
    /// host that keeps its items somewhere else entirely, and whose source only
    /// reads them, has to call [`ListView::refresh`] itself.
    pub fn source_mut(&mut self, cx: &mut Context<Self>) -> &mut S {
        self.dirty = true;
        cx.notify();
        &mut self.source
    }

    /// Rereads the source.
    ///
    /// For changes the list cannot have seen: data that lives outside the
    /// source, or was written into it without going through
    /// [`ListView::source_mut`].
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        cx.notify();
    }

    /// The selected item.
    pub fn selected(&self) -> Option<&S::Id> {
        self.selected.as_ref()
    }

    /// Moves the selection and scrolls its row into view.
    ///
    /// An id the source does not hold is not a selection the list can point at,
    /// and clears it instead of being remembered — the opposite of a tree,
    /// which keeps one that may be inside something closed.
    pub fn set_selected(&mut self, id: Option<S::Id>, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        let index = id.as_ref().and_then(|id| self.index_of(id));
        self.set_selection(id.filter(|_| index.is_some()), index, cx);
        if let Some(ix) = index {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        }
    }

    /// Where the selection sits, as of the last rebuild.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Brings row `index` into view, leaving the selection where it is.
    ///
    /// An index past the end is ignored rather than clamped: it names a row
    /// that is not there, and scrolling to the last one instead would be an
    /// answer to a different question.
    pub fn scroll_to(&mut self, index: usize, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        if index < self.len {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
            cx.notify();
        }
    }

    /// Rereads the source when it may have moved on.
    fn ensure_fresh(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            self.rebuild(cx);
        }
    }

    /// Recounts the rows and finds the selection again.
    ///
    /// This is where a selection the host has thrown away is noticed: the id is
    /// looked for once per rebuild rather than once per draw, which is what
    /// makes the linear scan affordable.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.len = self.source.len();
        self.dirty = false;

        let Some(selected) = self.selected.clone() else {
            self.selected_index = None;
            return;
        };
        self.selected_index = self.index_of(&selected);
        if self.selected_index.is_none() {
            self.selected = None;
            cx.emit(ListEvent::SelectionChanged(None));
            cx.notify();
        }
    }

    /// Where `id` sits, or `None` when the source no longer holds it.
    fn index_of(&self, id: &S::Id) -> Option<usize> {
        (0..self.len).find(|ix| self.source.id(*ix) == *id)
    }

    /// Selects the item on row `ix` and brings it into view.
    fn select_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        let id = self.source.id(ix);
        self.set_selection(Some(id), Some(ix), cx);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    /// Records a selection and the row carrying it, announcing it only when it
    /// really changed.
    fn set_selection(&mut self, id: Option<S::Id>, index: Option<usize>, cx: &mut Context<Self>) {
        if self.selected == id {
            // The row may still have moved under an unchanged id, which is
            // what the caller has just looked up.
            self.selected_index = index;
            return;
        }
        self.selected = id.clone();
        self.selected_index = index;
        cx.emit(ListEvent::SelectionChanged(id));
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        let next = match self.selected_index {
            Some(0) => None,
            Some(ix) => Some(ix - 1),
            // No selection and a key that means "upwards" lands on the row
            // furthest that way, as it does in a tree.
            None => self.len.checked_sub(1),
        };
        if let Some(ix) = next {
            self.select_row(ix, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        let next = match self.selected_index {
            Some(ix) => Some(ix + 1).filter(|ix| *ix < self.len),
            None => (self.len > 0).then_some(0),
        };
        if let Some(ix) = next {
            self.select_row(ix, cx);
        }
    }

    fn select_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        if self.len > 0 {
            self.select_row(0, cx);
        }
    }

    fn select_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        if let Some(ix) = self.len.checked_sub(1) {
            self.select_row(ix, cx);
        }
    }

    fn activate_selected(&mut self, _: &Activate, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_fresh(cx);
        if let Some(id) = self.selected.clone() {
            cx.emit(ListEvent::Activated(id));
        }
    }

    /// The scroll container behind the list, which is what the bar measures.
    fn base_handle(&self) -> ScrollHandle {
        self.scroll.0.borrow().base_handle.clone()
    }

    /// The overlay bar as it stands this frame.
    fn scrollbar(&self) -> Scrollbar {
        Scrollbar::for_handle(
            self.bar_id.clone(),
            ScrollbarAxis::Vertical,
            &self.base_handle(),
        )
        .fade(self.bar.fade())
    }

    /// Lets go of the thumb and starts the clock that takes the bar down.
    fn release_thumb(&mut self, cx: &mut Context<Self>) {
        if let Some(epoch) = self.bar.release() {
            hide_later(epoch, cx, |list: &mut Self| Some(&mut list.bar));
            cx.notify();
        }
    }

    /// Puts the bar up while the pointer rests on the edge it rides, and starts
    /// it going the moment the pointer leaves.
    fn hover_scrollbar(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.bar.hover_enter() {
                cx.notify();
            }
            return;
        }

        let Some(epoch) = self.bar.hover_leave() else {
            return;
        };
        hide_now(self, epoch, cx, |list: &mut Self| Some(&mut list.bar));
    }

    /// Draws row `ix`: the height, the padding and the background are the
    /// list's, everything inside them is the source's.
    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme(cx);
        let id = self.source.id(ix);
        let selected = self.selected.as_ref() == Some(&id);
        let info = ListRowInfo {
            index: ix,
            selected,
        };
        let content = self.source.render_item(&id, info, window, cx);
        let menu_id = id.clone();

        div()
            .id(ElementId::from(("list-row", ix)))
            .flex()
            .flex_row()
            .items_center()
            .h(self.row_height)
            .w_full()
            .px(px(ROW_PADDING))
            .when(selected, |this| this.bg(theme.surface_active))
            .cursor_pointer()
            .hover(|style| style.bg(theme.surface_hover))
            .on_click(cx.listener(move |list, event: &ClickEvent, window, cx| {
                list.focus_handle.focus(window, cx);
                list.set_selected(Some(id.clone()), cx);
                // Nothing here competes with the host for the second click:
                // a row is the thing itself, so opening it is the host's to
                // define and every double click travels on.
                if event.click_count() >= 2 {
                    cx.emit(ListEvent::Activated(id.clone()));
                }
            }))
            // Taken on the press rather than on the click, which is gpui's name
            // for a left button only, and because a menu that appeared on
            // release would lag the gesture everywhere else in the shell.
            //
            // The press is swallowed: it belongs to the menu about to open, not
            // to whatever the list is sitting in.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |list, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    list.focus_handle.focus(window, cx);
                    // The menu's commands act on the selection — the host has
                    // no other handle on a row — so the right-click has to move
                    // it first, or "delete" would name whatever the user last
                    // clicked instead of what they just aimed at.
                    list.set_selected(Some(menu_id.clone()), cx);
                    cx.emit(ListEvent::ContextMenu {
                        id: menu_id.clone(),
                        position: event.position,
                    });
                }),
            )
            // The host's row fills what is left, and is told so rather than
            // left to size itself: a card that lays its second line out with
            // `justify_between` needs the width to justify against, and
            // `min_w_0` is what lets an over-long line be clipped instead of
            // pushing the row wider than the list.
            .child(div().flex_1().min_w_0().child(content))
            .into_any_element()
    }
}

impl<S: ListSource> EventEmitter<ListEvent<S::Id>> for ListView<S> {}

impl<S: ListSource> Focusable for ListView<S> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<S: ListSource> Render for ListView<S> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_fresh(cx);
        let theme = theme(cx);
        let count = self.len;
        let list = cx.entity();

        // The owner's half of an overlay bar, as every scrolling surface in the
        // app wires it: notice the list has moved, and arm the expiry from
        // inside the draw that noticed.
        if let Some(epoch) = self
            .bar
            .moved(scrolled(&self.base_handle(), ScrollbarAxis::Vertical))
        {
            hide_later(epoch, cx, |list: &mut Self| Some(&mut list.bar));
        }

        let body = if count == 0 {
            // The source's own answer for an empty list, given the whole area
            // rather than a row: what goes there is usually centred, and a
            // 24 px band at the top is not somewhere to centre anything in.
            div()
                .size_full()
                .child(self.source.render_empty(window, cx))
                .into_any_element()
        } else {
            // Only the rows the viewport can reach are built, which is what
            // lets a list of ten thousand items be scrolled at all.
            let mut rows = uniform_list("list-rows", count, move |range, window, cx| {
                list.update(cx, |list, cx| {
                    range
                        .map(|ix| list.render_row(ix, window, cx))
                        .collect::<Vec<_>>()
                })
            })
            .track_scroll(&self.scroll)
            .size_full();
            // Keeps a sideways wheel off this list's vertical scroll, the way
            // every other scrolling surface here asks for it (see
            // [`crate::scrollbar`]). Spelled against the interactivity rather
            // than through `restrict_scroll_to_axis()` because that method
            // belongs to gpui's *stateful* half of the interactive traits,
            // which a `UniformList` — scrolled by a handle of its own rather
            // than by an element id — does not implement. The flag itself lives
            // on the shared style the same paint code reads for both, so the
            // effect is identical.
            rows.interactivity().base_style.restrict_scroll_to_axis = Some(true);
            rows.into_any_element()
        };

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .text_size(px(13.))
            .text_color(theme.text)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::activate_selected))
            .on_drag_move::<DraggedThumb>(cx.listener(
                |list, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                    let Some(progress) = list.scrollbar().dragged(event, cx) else {
                        return;
                    };
                    list.bar.hold();
                    scroll_to(&list.base_handle(), ScrollbarAxis::Vertical, progress);
                    cx.notify();
                },
            ))
            // Both halves: a thumb dragged off the end of its track lets go with
            // the pointer outside the window, which only the second sees.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|list, _: &MouseUpEvent, _window, cx| list.release_thumb(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|list, _: &MouseUpEvent, _window, cx| list.release_thumb(cx)),
            )
            .child(body)
            .children(
                self.scrollbar()
                    .on_hover(cx.listener(|list, hovered: &bool, _window, cx| {
                        list.hover_scrollbar(*hovered, cx);
                    }))
                    .render(&theme),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::ops::Deref;
    use std::rc::Rc;

    use gpui::{Entity, Modifiers, SharedString, TestAppContext, VisualTestContext, point};

    use super::*;

    /// Item ids are plain strings here, as a host's would be a key.
    type Id = &'static str;

    /// Vertical middle of the first row.
    const FIRST_ROW: f32 = DEFAULT_ROW_HEIGHT / 2.;

    /// Vertical middle of the second row.
    const SECOND_ROW: f32 = DEFAULT_ROW_HEIGHT + DEFAULT_ROW_HEIGHT / 2.;

    /// A column well inside the row, on whatever the source drew.
    const ON_THE_ROW: f32 = 60.;

    /// A source that is nothing but the vector a host would keep, plus a note
    /// of what it was asked to draw.
    #[derive(Default)]
    struct Fixture {
        items: Vec<Id>,
        /// Every `(index, selected)` a row was handed, in the order the frames
        /// asked for them; see [`Handles::drawn`].
        seen: Rc<RefCell<Vec<(usize, bool)>>>,
    }

    impl ListSource for Fixture {
        type Id = Id;

        fn len(&self) -> usize {
            self.items.len()
        }

        fn id(&self, index: usize) -> Id {
            self.items[index]
        }

        fn render_item(
            &self,
            id: &Id,
            info: ListRowInfo,
            _window: &mut Window,
            _cx: &mut App,
        ) -> AnyElement {
            self.seen.borrow_mut().push((info.index, info.selected));
            div().child(SharedString::from(*id)).into_any_element()
        }
    }

    /// A source over `items`, with nothing drawn yet.
    fn source(items: &[Id]) -> Fixture {
        Fixture {
            items: items.to_vec(),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// A view that does nothing but hold the list, as a panel would.
    struct Harness {
        list: Entity<ListView<Fixture>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.list.clone())
        }
    }

    /// Everything a test reads back: the list, what it announced, and what its
    /// source was asked to draw.
    struct Handles {
        list: Entity<ListView<Fixture>>,
        events: Rc<RefCell<Vec<ListEvent<Id>>>>,
        seen: Rc<RefCell<Vec<(usize, bool)>>>,
    }

    impl Handles {
        /// The selected item.
        fn selected(&self, cx: &mut VisualTestContext) -> Option<Id> {
            cx.update(|_, cx| self.list.read(cx).selected().copied())
        }

        /// Everything announced since the last look.
        fn drain(&self) -> Vec<ListEvent<Id>> {
            self.events.borrow_mut().drain(..).collect()
        }

        /// Every `(index, selected)` handed to the source since the last look.
        fn drawn(&self) -> Vec<(usize, bool)> {
            self.seen.borrow_mut().drain(..).collect()
        }

        /// Replaces the items the way a host does when a filter or a refetch
        /// changes what there is.
        fn replace(&self, items: &[Id], cx: &mut VisualTestContext) {
            let items = items.to_vec();
            cx.update(|_, cx| {
                self.list
                    .update(cx, |list, cx| list.source_mut(cx).items = items)
            });
            cx.run_until_parked();
        }
    }

    /// Opens a focused list over `fixture` and hands back its handles.
    fn open(fixture: Fixture, cx: &mut TestAppContext) -> (Handles, VisualTestContext) {
        cx.update(crate::init);

        let seen = fixture.seen.clone();
        let events: Rc<RefCell<Vec<ListEvent<Id>>>> = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_window({
            let events = events.clone();
            move |_, cx| {
                let list = cx.new(|cx| ListView::new(fixture, cx));
                cx.subscribe(
                    &list,
                    move |_: &mut Harness, _, event: &ListEvent<Id>, _| {
                        events.borrow_mut().push(event.clone());
                    },
                )
                .detach();
                Harness { list }
            }
        });
        let list = window
            .update(cx, |harness, _, _| harness.list.clone())
            .expect("the window is open");

        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.update(|window, cx| {
            let handle = list.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        });
        cx.run_until_parked();

        (Handles { list, events, seen }, cx)
    }

    /// Presses the left button `count` times over `position` in one go, which
    /// is how a double click reaches an element.
    fn click(cx: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
        });
        cx.run_until_parked();
    }

    /// Presses and releases the right button over `position`.
    ///
    /// Both halves, even though the list answers the press: a gesture that only
    /// ever went down would leave gpui holding state no real pointer leaves
    /// behind.
    fn right_click(cx: &mut VisualTestContext, position: Point<Pixels>) {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Right,
            click_count: 1,
        });
        cx.run_until_parked();
    }

    /// One click picks a row and says so, and picking the same row twice says
    /// nothing the second time.
    #[gpui::test]
    fn a_click_selects_the_row_and_announces_it(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);

        click(&mut cx, point(px(ON_THE_ROW), px(SECOND_ROW)), 1);
        assert_eq!(list.selected(&mut cx), Some("grace"));
        assert_eq!(
            list.drain(),
            vec![ListEvent::SelectionChanged(Some("grace"))],
            "one click is not an activation"
        );

        click(&mut cx, point(px(ON_THE_ROW), px(SECOND_ROW)), 1);
        assert_eq!(list.drain(), vec![], "the row was already selected");
    }

    /// Unlike a tree's, every double click reaches the host: a flat row is the
    /// thing itself, and there is no branch for the gesture to open instead.
    #[gpui::test]
    fn a_double_click_activates_the_row(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);
        let on_the_first = point(px(ON_THE_ROW), px(FIRST_ROW));

        click(&mut cx, on_the_first, 1);
        list.drain();

        click(&mut cx, on_the_first, 2);
        assert_eq!(list.selected(&mut cx), Some("ada"));
        assert_eq!(list.drain(), vec![ListEvent::Activated("ada")]);
    }

    /// A right-click hands the host the row and the pointer, and moves the
    /// selection there first so that the menu it builds acts on what was aimed
    /// at.
    #[gpui::test]
    fn a_right_click_selects_the_row_and_asks_for_a_menu(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);

        let on_the_second = point(px(ON_THE_ROW), px(SECOND_ROW));
        right_click(&mut cx, on_the_second);
        assert_eq!(list.selected(&mut cx), Some("grace"));
        assert_eq!(
            list.drain(),
            vec![
                ListEvent::SelectionChanged(Some("grace")),
                ListEvent::ContextMenu {
                    id: "grace",
                    position: on_the_second,
                },
            ],
            "the selection moves before the menu is asked for"
        );

        // A second right-click on the row that is already selected still asks
        // for a menu, and announces no selection change.
        right_click(&mut cx, on_the_second);
        assert_eq!(
            list.drain(),
            vec![ListEvent::ContextMenu {
                id: "grace",
                position: on_the_second,
            }]
        );
    }

    /// Up and down walk the rows, `Home` and `End` jump to the ends, and none
    /// of them walks off either one.
    #[gpui::test]
    fn the_arrow_keys_walk_the_rows_and_stop_at_the_ends(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);

        cx.simulate_keystrokes("down");
        assert_eq!(list.selected(&mut cx), Some("ada"));
        cx.simulate_keystrokes("down down");
        assert_eq!(list.selected(&mut cx), Some("alan"));
        cx.simulate_keystrokes("down");
        assert_eq!(
            list.selected(&mut cx),
            Some("alan"),
            "the last row is the last"
        );

        cx.simulate_keystrokes("up up");
        assert_eq!(list.selected(&mut cx), Some("ada"));
        cx.simulate_keystrokes("up");
        assert_eq!(
            list.selected(&mut cx),
            Some("ada"),
            "the first row is the first"
        );

        cx.simulate_keystrokes("end");
        assert_eq!(list.selected(&mut cx), Some("alan"));
        cx.simulate_keystrokes("home");
        assert_eq!(list.selected(&mut cx), Some("ada"));
    }

    /// Enter and space are the host's cue to do something with the row.
    #[gpui::test]
    fn enter_and_space_activate_the_selection(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);

        cx.simulate_keystrokes("enter");
        assert_eq!(
            list.drain(),
            vec![],
            "nothing is selected, so there is nothing to activate"
        );

        cx.simulate_keystrokes("down");
        list.drain();
        cx.simulate_keystrokes("enter");
        assert_eq!(list.drain(), vec![ListEvent::Activated("ada")]);
        cx.simulate_keystrokes("space");
        assert_eq!(list.drain(), vec![ListEvent::Activated("ada")]);
    }

    /// The one place a list parts company with a tree: an id the source has
    /// stopped holding is gone rather than hidden, so the selection goes with
    /// it and the host is told.
    #[gpui::test]
    fn a_selection_the_host_removed_is_dropped(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);
        click(&mut cx, point(px(ON_THE_ROW), px(SECOND_ROW)), 1);
        list.drain();

        // A reorder that keeps the row is not a removal: the selection follows
        // its id to the row it landed on.
        list.replace(&["grace", "ada"], &mut cx);
        assert_eq!(list.selected(&mut cx), Some("grace"));
        assert_eq!(
            cx.update(|_, cx| list.list.read(cx).selected_index()),
            Some(0)
        );
        assert_eq!(list.drain(), vec![]);

        list.replace(&["ada", "alan"], &mut cx);
        assert_eq!(list.selected(&mut cx), None);
        assert_eq!(cx.update(|_, cx| list.list.read(cx).selected_index()), None);
        assert_eq!(list.drain(), vec![ListEvent::SelectionChanged(None)]);
    }

    /// What a row is told about itself: where it sits, and whether it is the
    /// selected one.
    #[gpui::test]
    fn a_row_is_told_where_it_sits_and_whether_it_is_selected(cx: &mut TestAppContext) {
        let (list, mut cx) = open(source(&["ada", "grace", "alan"]), cx);

        // The tail rather than the head: `uniform_list` draws the first row on
        // its own to measure one before it knows how many fit, so the run that
        // ends a frame is the frame's real range.
        let first = list.drawn();
        assert_eq!(
            &first[first.len() - 3..],
            &[(0, false), (1, false), (2, false)],
            "the rows are drawn in order, and nothing is selected yet"
        );

        click(&mut cx, point(px(ON_THE_ROW), px(SECOND_ROW)), 1);
        let after = list.drawn();
        assert!(
            after.contains(&(1, true)),
            "the clicked row is drawn as the selected one: {after:?}"
        );
        assert!(
            after.contains(&(0, false)) && !after.contains(&(0, true)),
            "and no other row is: {after:?}"
        );
    }
}
