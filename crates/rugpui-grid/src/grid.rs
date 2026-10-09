//! The widget: a million rows, drawn a screenful at a time.
//!
//! ## Both axes are virtualised
//!
//! Rows go through gpui's [`uniform_list`], which lays out only what the
//! viewport can reach — the same machinery the tree uses, and the reason a
//! result of any length costs the same to draw.
//!
//! Columns are virtualised here, by hand, because there is no `uniform_list`
//! for them and tables with several hundred columns are real. Every column's
//! left edge is kept in a list the grid rebuilds when a width changes, so the run
//! the content area can see is two binary searches; the rest are neither shaped
//! nor painted, and
//! a row is drawn as one absolutely positioned strip slid left by the scroll
//! offset rather than as a flex row of every cell with the invisible ones
//! clipped. Nothing per frame is proportional to the number of rows or to the
//! number of columns — only to the number of both that fit on screen.
//!
//! The horizontal offset is the grid's own field rather than a gpui scroll
//! container's, for the same reason: a scroll container lays its content out in
//! full, which is exactly the cost being avoided. It also makes the header and
//! the body trivially agree — they read the same number.
//!
//! ## What is measured, and when
//!
//! Which columns are visible depends on how wide the content area is, and that
//! is only known once gpui has laid the frame out. A [`canvas`] in the body
//! reports the size during prepaint and asks for a repaint when it changed, so
//! a resize — and the very first frame — costs one extra frame and nothing
//! after that. The overlay scrollbars already trail a resize by a frame for the
//! same reason.
//!
//! ## Columns fit what is in them
//!
//! By default, and exactly. A grid whose every column is the same width shows a
//! timestamp as `2026-02-03 09:14…`, which is not a timestamp; the user has to
//! drag before they can read data they already have. So the first draw that has
//! rows to measure sizes every column to its content, and the frame that
//! decides the widths is the frame that draws them — the fitting happens at the
//! top of `render`, before anything is laid out, which is also the
//! only place with a [`Window`] to shape text with.
//!
//! *Exactly* means shaped, not estimated. Estimating from character cells is
//! what leaves a column a few pixels short on a proportional face, and a few
//! pixels short is an ellipsis. What keeps that affordable is that the estimate
//! is still used — but only to **narrow the field**, never to pick the winner.
//! One allocation-free pass over the sampled rows keeps every value within
//! `FIT_SPREAD` cells of the longest, up to `FIT_CANDIDATES` of them, and only
//! those, plus the heading, go to the text system. It cannot pick the winner
//! because character cells and pixels are different units: `Pinewood Hardware`
//! and `Northwind Traders` are both seventeen cells and one of them is plainly
//! wider. Five hundred rows cost five hundred integer comparisons and at most
//! seventeen shaped lines. That pass is the one thing in the widget proportional
//! to the size of the result rather than to the size of the window, which is why
//! it is bounded at `AUTOFIT_SAMPLE` and why it runs when a batch lands rather
//! than every frame.
//!
//! Whose width it is decides what may happen to it next — see `Sizing`. A width
//! the user dragged is theirs until they ask for it back; a fitted one may be
//! *widened* as later batches of the same result arrive but never narrowed,
//! because a column that shrank under a pointer that was reading it is worse
//! than a column slightly too wide; and a new result ([`GridView::reset`])
//! starts the whole argument over.
//!
//! ## What the grid asks the host to do
//!
//! Five things, all of them round trips the widget has no business making:
//! fetching the next batch ([`GridEvent::NearEnd`]), re-running the query in a
//! different order ([`GridEvent::SortRequested`] — the grid never sorts what it
//! holds, because it holds only the first n rows of an answer the server has all
//! of), opening a cell ([`GridEvent::CellActivated`], which is how a LOB
//! reaches a viewer), staging a typed value ([`GridEvent::EditCommitted`]), and
//! drawing the right-click menu ([`GridEvent::ContextMenu`] — the grid has no
//! strings to name items with, design notes §7.8). Copying is *not*
//! among them: gpui owns the clipboard and the grid owns the selection, so the
//! grid does it itself.
//!
//! ## Editing, and the little of it that lives here
//!
//! The grid draws edit state and hosts the field the user types into; it stages
//! nothing and sends nothing. Which rows are marked and which cells are tinted
//! come from [`GridSource::row_status`] and [`GridSource::cell_dirty`], asked
//! only about what is on screen; whether a cell can be typed into at all comes
//! from [`GridSource::cell_editable`].
//!
//! The field itself has to be here for one reason: it is placed over a cell, and
//! nothing else knows where a cell is. A cell's rectangle falls out of
//! `laid_out`, `h_offset`, the row height and the list's scroll offset — four
//! numbers the grid keeps and nobody else sees — so [`GridView::begin_edit`]
//! owns the [`TextInput`] rather than the host owning it and asking where to put
//! it.
//!
//! *Which* editor goes there is the source's, not the widget's:
//! [`GridSource::cell_editor`] answers with a field, a dropdown over a list the
//! source knows, or an element the host builds itself. All three land in the
//! same box for the same reason — only the grid knows where the cell is — and
//! all three close by the same rules. What differs is when they stage: a field
//! stages on the close, while a dropdown and a host's editor stage the moment
//! the user picks, so everything that merely closes one of those takes it down
//! with nothing staged.
//!
//! ## What is drawn in a cell is the source's too
//!
//! [`GridSource::render_cell`] is offered every visible cell before the grid
//! draws its text, and a source that wants a badge, a bar or a swatch returns an
//! element for it. The grid goes on painting everything *around* the content —
//! the row stripe, the selection, the dirty tint, the cursor outline — so a
//! custom cell is picked and marked exactly as a plain one is, and
//! [`GridSource::cell`] still has to answer, because copying, column fitting
//! and the editor all read a cell's text and none of them can read an element.
//!
//! **A close commits.** `Enter`, focus going elsewhere, a sort, a refresh, a
//! scroll, a column dragged — all of them end the edit by raising
//! [`GridEvent::EditCommitted`], and only `Escape` throws the typing away. The
//! asymmetry is deliberate: what is committed is *staged*, not sent, so the cost
//! of committing something the user did not mean is one undo in the pending
//! changes, while the cost of discarding is the typing. Committing an unchanged
//! field raises nothing at all, so the common case — open a cell, look at it,
//! move on — is silent either way.

mod editing;
mod input;
mod layout;
use layout::offset;
#[cfg(test)]
use layout::shaped_width;
mod render;
use render::choice_value;
#[cfg(test)]
use render::status_color;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, DragMoveEvent,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, Font, Hsla, IsZero, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle,
    ScrollStrategy, ScrollWheelEvent, SharedString, Size, Subscription, TextRun,
    UniformListScrollHandle, Window, actions, canvas, div, point, prelude::*, px, size,
    uniform_list,
};
use rugpui::scrollbar::{
    DraggedThumb, Scrollbar, ScrollbarAxis, ScrollbarState, hide_later, hide_now, scroll_to,
    scrolled,
};
use rugpui::select::Select;
use rugpui::text_input::TextInput;
use rugpui::theme::{Theme, theme, window_translucent};
use unicode_width::UnicodeWidthStr;

use crate::copy::{CopyFormat, DEFAULT_INSERT_TABLE, copy_payload};
use crate::selection::{CellAddress, Selection};
use crate::source::{
    CellEditor, CellEditorBuilder, CellEditorContext, CellInfo, DEFAULT_TEXT, GridCell,
    GridColumnAlign, GridSource, GridSourceState, NULL_TEXT, RowStatus, cell_label, lob_label,
};

actions!(
    rugpui_grid,
    [
        /// Move the cursor one row up.
        MoveUp,
        /// Move the cursor one row down.
        MoveDown,
        /// Move the cursor one column left.
        MoveLeft,
        /// Move the cursor one column right.
        MoveRight,
        /// Stretch the selection one row up.
        ExtendUp,
        /// Stretch the selection one row down.
        ExtendDown,
        /// Stretch the selection one column left.
        ExtendLeft,
        /// Stretch the selection one column right.
        ExtendRight,
        /// Move to the first column of the current row.
        MoveRowStart,
        /// Move to the last column of the current row.
        MoveRowEnd,
        /// Move to the very first cell.
        MoveFirst,
        /// Move to the very last cell.
        MoveLast,
        /// Move the cursor up by one screenful.
        PageUp,
        /// Move the cursor down by one screenful.
        PageDown,
        /// Stretch the selection up by one screenful.
        ExtendPageUp,
        /// Stretch the selection down by one screenful.
        ExtendPageDown,
        /// Select every cell.
        SelectAll,
        /// Copy the selection as TSV.
        CopyCells,
        /// Open the cell under the cursor, which is what a double click does.
        Activate,
        /// Throw away what has been typed into the inline editor and close it.
        CancelEdit,
        /// Commit the inline editor and open the next editable cell of the row.
        EditNext,
        /// Commit the inline editor and open the previous editable cell of the
        /// row.
        EditPrevious,
    ]
);

/// Key context that [`init`] binds the keys above to.
const KEY_CONTEXT: &str = "GridView";

/// Key context that exists only while the inline editor is open.
///
/// Its three keys — `Escape`, `Tab`, `Shift+Tab` — mean nothing to a grid that
/// is merely focused, and binding them on [`KEY_CONTEXT`] would take them away
/// from the app for as long as a grid has the focus: an `Escape` that closed a
/// dialog would close nothing while the user's eye was on a result. A context
/// that only exists for the frames the editor does cannot do that.
///
/// It is a context on the editor's own wrapper, *inside* the grid's, so the
/// stack while typing reads `GridView > GridCellEditor > TextInput`. The field's
/// own bindings sit deepest and therefore win: `Enter` is the field's `Submit`
/// and never the grid's `Activate`, and the arrows walk the caret rather than
/// the selection.
const EDITOR_KEY_CONTEXT: &str = "GridCellEditor";

/// Height of one body row, and therefore the unit [`uniform_list`] measures in.
const ROW_HEIGHT: f32 = 24.;

/// Height of the column header band.
const HEADER_HEIGHT: f32 = 26.;

/// Width of the row-number gutter down the left-hand edge.
const GUTTER_WIDTH: f32 = 56.;

/// Padding at both ends of a cell.
const CELL_PADDING: f32 = 6.;

/// Width of the line between one cell and the next.
///
/// Written down rather than left implicit in the `border_r_1()` the cells and
/// the headings are drawn with, because a fitted width has to leave room for
/// it: gpui sizes a box the way CSS's `border-box` does, so the padding *and*
/// the border come out of the width a column is given, and a column exactly as
/// wide as its padding plus its text is a column one pixel too narrow — which
/// is an ellipsis, which is the fault fitting exists to cure. Keep the two in
/// step if the border ever changes.
const CELL_BORDER: f32 = 1.;

/// The size the grid draws its text at.
///
/// A constant rather than a number in `render` because fitting a column has to
/// shape at exactly the size the cells are drawn at: measured at any other size
/// the answer is a fit to a table nobody is looking at.
const TEXT_SIZE: f32 = 13.;

/// Width a column is given before it has been fitted or dragged.
///
/// With fitting on — which is the default — a column only ever wears this while
/// there are no rows to measure it against, because an empty result says
/// nothing about how wide its values are.
const DEFAULT_COLUMN_WIDTH: f32 = 140.;

/// Narrowest a column may be dragged.
///
/// Not zero: a column dragged shut could not be found again, since the grip is
/// on its right-hand edge.
const MIN_COLUMN_WIDTH: f32 = 32.;

/// Widest a column may be made by *fitting* it.
///
/// A dragged column has no cap — the user can see what they are doing — but a
/// fitted `TEXT` column would otherwise be as wide as a paragraph, and one such
/// column pushes every column after it off the screen.
const MAX_AUTOFIT_WIDTH: f32 = 480.;

/// Width of the invisible strip on a column's edge that answers a resize drag.
const GRIP_WIDTH: f32 = 6.;

/// How far behind the widest sampled value, in character cells, a value may
/// still be worth shaping.
///
/// **Equal character counts are not equal widths.** On the proportional face an
/// app actually ships, `Pinewood Hardware` and `Northwind Traders` are both
/// seventeen cells wide and neither is seventeen anythings wide on screen: `W`
/// and `H` are broad, `i` and `l` are hairlines, and the gap between two
/// same-length values can be tens of pixels. A ranking by character cells
/// therefore says only *roughly* which value is widest, and a fit that shaped
/// the top of that ranking alone would leave whichever near-miss actually
/// shapes widest one ellipsis short — which is the fault fitting exists to
/// cure.
///
/// So the cheap count is not used to pick a winner. It is used to draw a band:
/// everything within this many cells of the leader is a plausible winner and
/// goes to the text system, and everything below it cannot make the difference
/// up in glyph widths. Three cells is comfortably more than the widest-to-
/// narrowest spread of one character in the faces a UI uses.
const FIT_SPREAD: usize = 3;

/// The most values shaped to size one column.
///
/// A ceiling on what [`FIT_SPREAD`]'s band can cost, for the column where five
/// hundred sampled values are all the same length — an `id`, a status, a
/// timestamp — and every one of them is therefore in the band. Sixteen shaped
/// lines is nothing next to the frame that draws them, and if more than sixteen
/// values tie it is the widest by character count that are kept: with that many
/// candidates the odds of the true winner being outside them are remote, and
/// something has to give or the cheap pass was pointless.
const FIT_CANDIDATES: usize = 16;

/// How many rows short of the end the next batch is asked for.
///
/// Asked for *before* the bottom is reached, and by a margin: a fetch that
/// starts when the last row appears has already lost, because the scroll stops
/// while it runs. With the default batch of 500 rows (design notes,
/// §7.5) this leaves a fifth of a batch of runway.
const NEAR_END_ROWS: usize = 100;

/// How many rows auto-fit looks at.
///
/// The first `n`, not all of them: fitting a column of a million rows would
/// have to read a million values, and the first screenful or two is what the
/// user is looking at anyway.
const AUTOFIT_SAMPLE: usize = 500;

/// Width of the strip down the gutter's left edge that marks a changed row.
///
/// Narrow on purpose: the row number has to stay readable beside it, and the
/// mark is answering "which rows did I touch?" at a glance down the column
/// rather than being read one row at a time.
const STATUS_WIDTH: f32 = 3.;

/// How hard a dirty cell is tinted.
///
/// Low enough that the text on top keeps the contrast the palette promised it,
/// and that a whole dirty row does not out-shout the selection drawn over it.
const DIRTY_TINT: f32 = 0.16;

/// How hard a whole inserted or deleted row is tinted.
///
/// Weaker than a dirty cell: this one covers the full width of the result, so
/// the same alpha would read as a change of theme rather than a change of row.
const ROW_TINT: f32 = 0.10;

/// How tall the inline editor is.
///
/// [`TextInput`] renders at a fixed height, which is taller than a row; the
/// field is centred on the cell rather than squeezed into it, so it reads as
/// something laid *over* the grid — which is what it is.
const EDITOR_HEIGHT: f32 = 32.;

/// Marker drawn in the header of an ascending column.
const SORT_ASCENDING: &str = "\u{25b4}";

/// Marker drawn in the header of a descending column.
const SORT_DESCENDING: &str = "\u{25be}";

/// The size a sort marker is drawn at.
///
/// Small: it is an answer to "which column is this ordered by?", asked of the
/// whole header at once, and the name beside it is the thing being read.
const SORT_MARKER_SIZE: f32 = 8.;

/// The gap between a heading's name and its sort marker.
const HEADING_GAP: f32 = 4.;

/// Registers the key bindings every [`GridView`] relies on.
///
/// Scoped to the `GridView` key context, so the arrows and the clipboard chords
/// keep meaning what they mean everywhere else in the app.
pub fn init(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };

    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(KEY_CONTEXT)),
        KeyBinding::new("left", MoveLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-up", ExtendUp, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-down", ExtendDown, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-left", ExtendLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-right", ExtendRight, Some(KEY_CONTEXT)),
        KeyBinding::new("home", MoveRowStart, Some(KEY_CONTEXT)),
        KeyBinding::new("end", MoveRowEnd, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{modifier}-home"), MoveFirst, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{modifier}-end"), MoveLast, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PageUp, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", PageDown, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-pageup", ExtendPageUp, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-pagedown", ExtendPageDown, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{modifier}-a"), SelectAll, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{modifier}-c"), CopyCells, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Activate, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", CancelEdit, Some(EDITOR_KEY_CONTEXT)),
        KeyBinding::new("tab", EditNext, Some(EDITOR_KEY_CONTEXT)),
        KeyBinding::new("shift-tab", EditPrevious, Some(EDITOR_KEY_CONTEXT)),
    ]);
}

/// Which way a column is ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// `ORDER BY … ASC`.
    Ascending,
    /// `ORDER BY … DESC`.
    Descending,
}

/// What a right click landed on, so that the host knows which menu to draw.
///
/// The grid does not name the items and does not run them: it says where the
/// press was and what was under it, and the host — which owns the strings and
/// the commands — does the rest (design notes, §7.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTarget {
    /// The body: a cell, or a row number in the gutter.
    ///
    /// Which cells the menu acts on is [`GridView::selection`], not this — a
    /// right click inside the selection leaves it alone, so the pressed cell is
    /// not necessarily the interesting one.
    Cell,
    /// A column heading.
    Header {
        /// The source column, unaffected by hiding or by column widths.
        column: usize,
    },
}

/// A value the user staged, on its way to whatever writes it.
///
/// Two variants, because a cell can be given a value or be given *no* value,
/// and those are different statements — the distinction the whole crate is
/// built around (design notes, §7.5). A `Lob` for a body that arrives
/// from a file instead of a keyboard is the one still to come; matching on the
/// enum now costs a host nothing and saves it a signature change later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditValue {
    /// What was in the field, verbatim.
    ///
    /// Not parsed and not trimmed: the grid has no idea what the column's type
    /// will make of it, and a layer that silently trimmed a `CHAR(10)` would be
    /// wrong in a way nobody could see.
    Text(String),
    /// The cell is to hold no value at all: `SET x = NULL`, not `SET x = ''`.
    ///
    /// Raised by the clearing gesture — the `NULL` row of a
    /// [`CellEditor::Choice`] whose `nullable` is set, or a custom editor that
    /// commits it — and never by an emptied field, because emptying a field is
    /// how the empty string is typed. A cell that already held no value stages
    /// nothing when this arrives, exactly as an unchanged field does.
    Null,
}

/// What the grid asks its host for.
///
/// [`Clone`] but not [`Copy`], since [`GridEvent::EditCommitted`] carries the
/// text the user typed. Every other variant is still four words of nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridEvent {
    /// The viewport has come within a hundred rows of the last row the source
    /// holds, and the source said there are more.
    ///
    /// Raised once per row count: the host fetches the next batch, drops it into
    /// its source, and the grid — now looking at a longer result — asks again
    /// when the new end comes into view. A burst of scrolling that never reaches
    /// new rows asks once.
    NearEnd,
    /// The user clicked a column header, and wants the query re-run in that
    /// order.
    ///
    /// `direction` is `None` for the third click, which drops the ordering
    /// altogether. The grid does not sort: it holds the first `n` rows of a
    /// result the server holds all of, so sorting what is here would put the
    /// wrong rows at the top (design notes, §7.5). The host re-runs
    /// with a new `ORDER BY` and replaces the source; until it does, the grid
    /// goes on showing the old order under the new marker.
    SortRequested {
        /// The source column index, unaffected by hiding or by column widths.
        column: usize,
        /// The order asked for, or `None` to drop the ordering.
        direction: Option<SortDirection>,
    },
    /// A cell was double clicked or `Enter` was pressed on it.
    ///
    /// How a LOB reaches its viewer, and how a cell reaches the editor: a host
    /// that answers this with [`GridView::begin_edit`] has DBeaver's gesture,
    /// and one that answers it with a viewer has the old one. The grid raises
    /// the same event either way, because which of the two a cell deserves
    /// depends on the column's type and on whether the result can be written
    /// to — neither of which is the widget's to judge.
    CellActivated {
        /// The row.
        row: usize,
        /// The source column index.
        column: usize,
    },
    /// The user finished typing into the inline editor, and the value is
    /// different from the one that was in the cell.
    ///
    /// Raised by `Enter`, by `Tab`, by the focus going elsewhere and by anything
    /// that moves the cell out from under the field — see the module docs on why
    /// a close commits. *Not* raised when the field was left as it was found,
    /// which is what keeps opening a null cell and thinking better of it from
    /// turning `NULL` into the empty string.
    ///
    /// The grid has staged nothing and changed nothing by raising this: the
    /// value it holds is what was typed, and the cell goes on drawing whatever
    /// [`GridSource::cell`] returns until the host's staging layer says
    /// otherwise.
    EditCommitted {
        /// The row.
        row: usize,
        /// The source column index, unaffected by hiding or by column widths.
        column: usize,
        /// What was typed.
        value: EditValue,
    },
    /// The user right clicked, and wants the menu for what is under the
    /// pointer.
    ///
    /// The grid has already taken the focus and moved the selection if it had
    /// to; what is left — deciding which items exist, what they are called,
    /// which are greyed out and what they do — is the host's, because this
    /// layer holds no strings (design notes, §7.8). Everything such a
    /// menu needs is on [`GridView`] already: [`GridView::copy`],
    /// [`GridView::select_all`], [`GridView::clear_selection`],
    /// [`GridView::toggle_sort`], [`GridView::set_column_hidden`],
    /// [`GridView::show_all_columns`], [`GridView::autofit_column`],
    /// [`GridView::autofit_all_columns`], and
    /// [`GridView::sort`], [`GridView::is_column_hidden`],
    /// [`GridView::hidden_column_count`], [`GridView::column_name`] to label
    /// and disable them.
    ContextMenu {
        /// What was under the pointer.
        target: MenuTarget,
        /// Where the pointer was, in **window** coordinates, which is what the
        /// menu anchors to.
        position: Point<Pixels>,
    },
}

/// Where a column's width came from, and therefore what the grid may do to it
/// without being asked.
///
/// Kept per column rather than as one flag on the grid because a real table is
/// a mix: two columns the user dragged to where they want them, and thirty the
/// grid sized itself and can go on sizing as more rows arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sizing {
    /// Never measured and never dragged — what a column wears until there are
    /// rows to fit it to, and all it ever wears with fitting turned off.
    Default,
    /// Measured against the content. May be widened again as the rest of the
    /// result arrives, and is thrown away by a *new* result.
    Fitted,
    /// Dragged, or set by the host. The grid never touches it again on its own:
    /// a column that sprang back to the grid's idea of the right width one page
    /// after the user set it would be a widget arguing with its user.
    User,
}

/// What has been done to one column.
///
/// Indexed by *source* column, so hiding one does not renumber the rest and a
/// width survives a hide. Reordering, when it lands, becomes an order vector
/// beside this one rather than a permutation of it, for exactly that reason.
#[derive(Clone, Copy, Debug)]
struct ColumnState {
    width: f32,
    hidden: bool,
    /// Whose width `width` is — see [`Sizing`].
    sizing: Sizing,
    /// Whether this column is to be measured at the next draw.
    ///
    /// The explicit requests — a double click on the grip,
    /// [`GridView::autofit_column`], [`GridView::autofit_all_columns`] — arrive
    /// without a [`Window`] and so cannot measure anything themselves; all they
    /// can do is leave this behind for the draw that can.
    fit: bool,
    // TODO(M3): `pinned: bool` — a pinned column is drawn in the gutter's strip
    // rather than in the scrolling one, so it never leaves the screen.
}

/// One column's place along the header, worked out from the widths.
///
/// Only the columns that are showing are in this list, and the index into it is
/// a column's *display* position — which is what the selection is written in
/// (see [`crate::selection`]).
#[derive(Clone, Copy, Debug)]
struct Placed {
    /// The source column this is.
    column: usize,
    /// Its left edge, measured from the left of the first column.
    x: f32,
}

/// A resize drag in progress.
#[derive(Clone, Copy, Debug)]
struct Resize {
    /// The source column being dragged.
    column: usize,
    /// Where the pointer was when it took hold.
    from: Pixels,
    /// How wide the column was then, so that the drag is absolute rather than a
    /// running total that could drift.
    width: f32,
}

/// Which of the three editors is open, and the state that one needs.
///
/// The rest of what an edit is — which cell, what was in it, whether that was a
/// value at all — is the same for all three and lives on [`Editing`] beside
/// this. Only the *mechanism* differs, and it differs in what holds the focus:
/// a field has one of its own, while a dropdown and a host's element are put
/// inside a box the grid focuses, so that `Escape` reaches
/// [`EDITOR_KEY_CONTEXT`] whatever is drawn in it.
enum OpenEditor {
    /// The one-line field, which is what every edit was before there was a
    /// choice.
    Field {
        /// The field. Rebuilt per edit rather than kept and re-seeded: a field
        /// carries a caret, a selection and an in-flight IME composition, and
        /// none of those mean anything in the next cell.
        input: Entity<TextInput>,
    },
    /// A [`Select`] opened over the cell.
    Choice {
        /// The box the list hangs from, focused so the keys reach the grid.
        focus: FocusHandle,
        /// The rows as they are drawn, the leading `NULL` one included — so
        /// that an index out of the list is an index into this.
        rows: Vec<SharedString>,
        /// Whether row zero is the `NULL` row, and therefore stages
        /// [`EditValue::Null`] rather than its own text.
        nullable: bool,
        /// Which row the keyboard is on, which is also the row the list marks
        /// as current.
        highlight: usize,
    },
    /// An element the host built, redrawn every frame like everything else.
    Custom {
        /// The box the element sits in, focused so that a host element which
        /// takes no focus of its own can still be dismissed with `Escape`.
        focus: FocusHandle,
        /// What builds it. Kept rather than the element it makes: an element is
        /// consumed by the frame that draws it.
        build: CellEditorBuilder,
    },
}

/// The inline editor, while it is open.
///
/// Holds the editor and the three things needed to decide what a staged value
/// *means* when it closes — which cell it was opened over, what was in that cell
/// and whether that was a value at all.
struct Editing {
    /// The row being edited.
    row: usize,
    /// The **source** column being edited, which is what the event names.
    column: usize,
    /// Which editor is open, and what it needs.
    editor: OpenEditor,
    /// What the field was seeded with, so that a close can tell a value the user
    /// changed from one they only looked at.
    seeded: String,
    /// Whether the cell held no value.
    ///
    /// Kept apart from `seeded` being empty, because the two are different
    /// cells: leaving an emptied field on a cell that was `NULL` leaves it
    /// `NULL`, while leaving it on a cell that held the empty string leaves the
    /// empty string. Flattening them here would lose exactly the distinction
    /// [`crate::source`] exists to keep.
    was_null: bool,
    /// Whether a frame has been drawn since the field opened.
    ///
    /// Opening one can scroll the result to bring its row into view, and until
    /// the list has laid itself out again the grid's idea of which rows are on
    /// screen is the one from before that scroll. Asking "has my row scrolled
    /// away?" against it would close the field on the frame it opened, so the
    /// first frame is not asked.
    settled: bool,
    /// The focus-out subscription. Dropped with the rest of this struct, which
    /// is what keeps closing an editor from being heard as the editor blurring.
    _blur: Subscription,
}

impl Editing {
    /// Whether `value` is something other than what the cell held.
    ///
    /// The whole of "was anything actually changed?", and the reason opening a
    /// cell and pressing `Enter` stages nothing. A cell that held no value is
    /// changed the moment anything is typed into it and not before: an empty
    /// field over a null cell is still the null, which is why `was_null` is a
    /// field of its own rather than `seeded.is_empty()`. The clearing gesture is
    /// the same rule read the other way round — clearing a cell that was already
    /// empty of any value changes nothing.
    fn changed(&self, value: &EditValue) -> bool {
        match value {
            EditValue::Text(typed) if self.was_null => !typed.is_empty(),
            EditValue::Text(typed) => *typed != self.seeded,
            EditValue::Null => !self.was_null,
        }
    }
}

/// What the pointer landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    /// The row-number gutter, on the given row.
    Gutter(usize),
    /// A cell.
    Cell(CellAddress),
}

/// A result set, drawn a screenful at a time.
///
/// Created as an entity and rendered as a child element, like the tree:
///
/// ```ignore
/// let grid = cx.new(|cx| GridView::new(Results::default(), cx));
/// cx.subscribe(&grid, |view, grid, event, cx| match event {
///     GridEvent::NearEnd => view.fetch_more(cx),
///     GridEvent::SortRequested { column, direction } => view.reorder(*column, *direction, cx),
///     GridEvent::CellActivated { row, column } => view.open_cell(*row, *column, cx),
///     GridEvent::ContextMenu { target, position } => view.open_menu(*target, *position, cx),
/// })
/// .detach();
/// ```
pub struct GridView<S: GridSource> {
    source: S,
    focus_handle: FocusHandle,
    /// One entry per source column, in source order.
    columns: Vec<ColumnState>,
    /// Whether the grid sizes its own columns — see [`GridView::autofit`].
    autofit: bool,
    /// How many rows the last fit measured, and `None` before the first one.
    ///
    /// The whole of "fit once, then only widen". A result arrives in batches, so
    /// the first batch is what the first fit sees; every batch after it may
    /// widen a fitted column but never narrow one, and only while this is still
    /// short of `AUTOFIT_SAMPLE` — past that the sample is full and another page
    /// can teach it nothing.
    fitted_at: Option<usize>,
    /// The showing columns and their left edges, in display order.
    laid_out: Vec<Placed>,
    /// How wide every showing column is, together.
    total_width: f32,
    /// How far the columns are scrolled sideways, counting up from the left.
    h_offset: f32,
    /// How wide the content area is, as of the last frame that measured it.
    viewport_width: f32,
    selection: Selection,
    /// The column the host has been asked to order by, if any.
    sort: Option<(usize, SortDirection)>,
    /// The row count the last [`GridEvent::NearEnd`] was raised at, which is
    /// what keeps a burst of scrolling from raising a fetch per frame.
    asked_at: Option<usize>,
    /// The rows [`uniform_list`] built last frame, which is both what "near the
    /// end" is measured against and what a page key moves by.
    visible_rows: Range<usize>,
    /// The table name written into a copied `INSERT`.
    insert_table: Option<SharedString>,
    resizing: Option<Resize>,
    /// Whether the pointer is dragging a selection out.
    dragging: bool,
    /// The inline editor, when one is open.
    editing: Option<Editing>,
    /// Whether the next frame has to take the focus back.
    ///
    /// Closing the editor drops the field, and with it the focus handle the
    /// keyboard was pointing at; something has to catch it or the grid goes
    /// deaf. It cannot be done where the closing happens — a host that calls
    /// [`GridView::refresh`] has no [`Window`] to hand — so the draw that
    /// notices the editor is gone does it instead. Not set by the one close
    /// that starts with the focus already having left.
    refocus: bool,
    scroll: UniformListScrollHandle,
    v_bar: ScrollbarState,
    h_bar: ScrollbarState,
    v_bar_id: ElementId,
    h_bar_id: ElementId,
}

impl<S: GridSource> GridView<S> {
    /// A grid over `source`, with nothing selected and nothing sorted.
    pub fn new(source: S, cx: &mut Context<Self>) -> Self {
        let mut grid = Self {
            source,
            focus_handle: cx.focus_handle(),
            columns: Vec::new(),
            autofit: true,
            fitted_at: None,
            laid_out: Vec::new(),
            total_width: 0.,
            h_offset: 0.,
            viewport_width: 0.,
            selection: Selection::new(),
            sort: None,
            asked_at: None,
            visible_rows: 0..0,
            insert_table: None,
            resizing: None,
            dragging: false,
            editing: None,
            refocus: false,
            scroll: UniformListScrollHandle::new(),
            v_bar: ScrollbarState::new(),
            h_bar: ScrollbarState::new(),
            v_bar_id: ElementId::from(("rugpui-grid-vbar", cx.entity_id())),
            h_bar_id: ElementId::from(("rugpui-grid-hbar", cx.entity_id())),
        };
        grid.ensure_layout();
        grid
    }

    /// Whether the grid sizes every column to what is in it (the default), or
    /// leaves them all at the same starting width.
    ///
    /// On is the right default because a column the user has to drag before
    /// they can read their own data is a column that failed at the one thing it
    /// is for: a timestamp shown as `2026-02-03 09:14…` is not a timestamp.
    /// Turn it off for a grid whose widths the host sets itself, or one where
    /// two grids lining up column for column matters more than the values
    /// fitting.
    ///
    /// Fitting never overrules the user: a width that was dragged is left alone
    /// until [`GridView::reset`], [`GridView::autofit_column`] or
    /// [`GridView::autofit_all_columns`] asks for it back.
    pub fn autofit(mut self, enabled: bool) -> Self {
        self.autofit = enabled;
        self
    }

    /// Every column at `DEFAULT_COLUMN_WIDTH` until something moves it — the
    /// same as `autofit(false)`, spelled the way a host reads it.
    pub fn fixed_widths(self) -> Self {
        self.autofit(false)
    }

    /// Places the grid at `index` in the window's tab order.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.focus_handle = self.focus_handle.clone().tab_index(index).tab_stop(true);
        self
    }

    /// Sets the table name written into a copied `INSERT`.
    ///
    /// Without one, [`DEFAULT_INSERT_TABLE`] is used — a name that will not
    /// parse, on purpose.
    pub fn insert_table(mut self, table: impl Into<SharedString>) -> Self {
        self.insert_table = Some(table.into());
        self
    }

    /// Sets the table name written into a copied `INSERT`, after the fact.
    pub fn set_insert_table(&mut self, table: Option<SharedString>) {
        self.insert_table = table;
    }

    /// The source, to read.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// The source, to change — dropping a fetched batch in, most of the time.
    ///
    /// Re-reads the shape on the next draw, so the caller has nothing to
    /// remember. Ends any edit in progress first: the rows are about to be
    /// something else, and a field left hanging over the y coordinate its cell
    /// used to be at is a field over the wrong cell.
    pub fn source_mut(&mut self, cx: &mut Context<Self>) -> &mut S {
        self.commit_edit(cx);
        cx.notify();
        &mut self.source
    }

    /// Re-reads the source, for a change the grid cannot have seen.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        cx.notify();
    }

    /// Throws away everything the user has done to the columns and the
    /// selection, which is what a *new* result — as opposed to another batch of
    /// the same one — deserves.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.columns.clear();
        self.laid_out.clear();
        self.selection.clear();
        self.sort = None;
        self.asked_at = None;
        // A new result is the one thing that earns a column the user dragged
        // being taken back: the widths were chosen for values that are not
        // there any more.
        self.fitted_at = None;
        self.h_offset = 0.;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.ensure_layout();
        cx.notify();
    }

    /// What is selected.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Whether the cell at `row` and display position `column` is selected.
    pub fn is_selected(&self, row: usize, column: usize) -> bool {
        self.selection.contains(row, column)
    }

    /// The column the host has been asked to order by, and which way.
    pub fn sort(&self) -> Option<(usize, SortDirection)> {
        self.sort
    }

    /// The rows [`uniform_list`] built for the last frame.
    ///
    /// What the "only the visible rows are touched" guarantee is stated in, and
    /// what a page key moves by.
    pub fn visible_rows(&self) -> Range<usize> {
        self.visible_rows.clone()
    }

    /// The source columns that are showing, left to right.
    ///
    /// The index into this is a cell's display column, which is how the
    /// selection and [`GridView::is_selected`] address one.
    pub fn visible_column_indices(&self) -> Vec<usize> {
        self.laid_out.iter().map(|placed| placed.column).collect()
    }

    /// How wide `column` is, in pixels.
    pub fn column_width(&self, column: usize) -> f32 {
        self.columns
            .get(column)
            .map_or(DEFAULT_COLUMN_WIDTH, |state| state.width)
    }

    /// Sets how wide `column` is, clamped to something that can still be found
    /// and dragged.
    ///
    /// The width becomes the user's: the grid will not fit this column again on
    /// its own, however many more rows arrive. [`GridView::reset`],
    /// [`GridView::autofit_column`] and [`GridView::autofit_all_columns`] are
    /// the three ways back, and all three are somebody asking.
    pub fn set_column_width(&mut self, column: usize, width: f32, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(state) = self.columns.get_mut(column) else {
            return;
        };
        let width = width.max(MIN_COLUMN_WIDTH);
        // Claimed before the width is compared, so that dragging a column back
        // to exactly where it already was still counts as having chosen it.
        state.sizing = Sizing::User;
        state.fit = false;
        if state.width == width {
            return;
        }
        state.width = width;
        self.relayout();
        self.clamp_h_offset();
        cx.notify();
    }
}

impl<S: GridSource> EventEmitter<GridEvent> for GridView<S> {}

impl<S: GridSource> Focusable for GridView<S> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests;
