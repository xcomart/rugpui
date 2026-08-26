//! Where a grid gets its rows, and the shape it insists on seeing them in.
//!
//! The grid never touches a result set. It is handed one through [`GridSource`],
//! exactly as the tree is handed its nodes through
//! [`TreeSource`](rugpui::TreeSource) — and for a sharper reason than reuse:
//! the thing a real grid is showing is a batch handed over by whatever fetched
//! it, and a crate that knew that type would need the fetcher — a database
//! driver, a JVM, a network — to run its own unit tests. Behind this trait
//! a test source is twenty lines and a million rows cost nothing, which is what
//! makes "does it still only touch the visible rows?" a thing that can be
//! asserted rather than eyeballed.
//!
//! ## Values are already strings
//!
//! [`GridCell::Text`] borrows from the source rather than owning, so drawing a
//! screenful of cells allocates nothing per cell for the value itself. That is
//! only possible because the values *are* strings by the time they reach here:
//! whatever decodes the wire hands over its text already decoded, and a grid is
//! a view of text. A source that would have to
//! format a number on the way out has nowhere to put the result, which is the
//! constraint saying so.
//!
//! ## Null is not empty
//!
//! [`GridCell::Null`] and `GridCell::Text("")` are different values and are
//! drawn differently — the marker `NULL` in [`Theme::grid_null`], against a cell
//! with nothing in it. Too many tools cannot tell you which one is in front of
//! you (design notes, §7.5), and a source that flattens the two here
//! has already lost the distinction whatever the widget does.
//!
//! ## What has been changed is the source's business too
//!
//! A grid that can be edited has to draw three more things: which rows have
//! been touched ([`GridSource::row_status`]), which individual cells carry the
//! change ([`GridSource::cell_dirty`]), and which cells will accept one at all
//! ([`GridSource::cell_editable`]). None of that is state the widget keeps.
//! Staging an edit means knowing the table behind the result and the key that
//! aims an `UPDATE` at a row, which is knowledge this crate deliberately does
//! not have (design notes, §7.5) — so the host wraps its result in
//! something that stages, and answers these three the way it already answers
//! [`GridSource::cell`].
//!
//! All three are defaulted to "nothing has been changed and nothing may be",
//! which is the truth for the read-only sources — a plan, a `DESCRIBE`, a diff
//! — that are half of what the grid is pointed at.
//!
//! ## Drawing, and choosing an editor, are the source's business too
//!
//! Two more defaulted hooks, and the reason they are on the *source* rather
//! than on the widget: both answers depend on the column's meaning, and the
//! column's meaning is exactly what the grid has refused to know. A `status`
//! column is a badge, a `total` is a number with a bar under it, a
//! `channel` is one of three words — none of which a widget over "columns and
//! rows" can work out, and all of which the thing that ran the query knows
//! before the first frame.
//!
//! [`GridSource::render_cell`] hands the host the cell's box and lets it draw
//! whatever it likes in it; [`GridSource::cell_editor`] says what opens over
//! that cell when it is edited — a field, a dropdown, or an element of the
//! host's own. Both default to "the grid's own behaviour", so a source that
//! wants neither says nothing.
//!
//! [`Theme::grid_null`]: rugpui::Theme#structfield.grid_null

use std::rc::Rc;

use gpui::{AnyElement, App, Pixels, SharedString, Window};
use rugpui::Theme;

use crate::grid::EditValue;

/// The text drawn in a cell that holds no value.
pub const NULL_TEXT: &str = "NULL";

/// The text drawn in a cell of a staged row the server will fill in itself.
///
/// Untranslated, exactly as [`NULL_TEXT`] is: both stand for a piece of SQL
/// rather than for a word, and a `DEFAULT` that read differently per locale
/// would stop naming the clause it means.
pub const DEFAULT_TEXT: &str = "DEFAULT";

/// Which way the values of a column line up in their cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridColumnAlign {
    /// Against the left-hand edge: text, dates, anything read from the start.
    Left,
    /// Against the right-hand edge, so that digits line up by place value.
    Right,
}

/// The rough shape of a column's values, as the source understands them.
///
/// A hint and not a type: the grid uses it to decide which way a column lines
/// up and whether a value needs quoting in generated SQL, and nothing else. A
/// source that cannot tell says [`GridColumnKind::Text`], which is the safe
/// answer to both questions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridColumnKind {
    /// Character data, and the fallback for anything unrecognised.
    #[default]
    Text,
    /// Numeric data of any width or scale.
    Number,
    /// A truth value.
    Boolean,
    /// A date, a time, a timestamp or an interval.
    Temporal,
    /// Bytes: `BLOB`, `BYTEA`, `VARBINARY`.
    Binary,
}

impl GridColumnKind {
    /// Which edge values of this kind line up against.
    ///
    /// Only numbers go right, and for the one reason that matters: a column of
    /// right-aligned digits can be read down for magnitude, and a left-aligned
    /// one cannot.
    pub fn align(self) -> GridColumnAlign {
        match self {
            GridColumnKind::Number => GridColumnAlign::Right,
            _ => GridColumnAlign::Left,
        }
    }

    /// Whether a value of this kind is quoted when it is written into SQL.
    ///
    /// Numbers and booleans are literals; everything else — text, dates, bytes
    /// — is quoted, because a bare `2024-01-01` is arithmetic in more dialects
    /// than it is a date.
    pub fn quoted_in_sql(self) -> bool {
        !matches!(self, GridColumnKind::Number | GridColumnKind::Boolean)
    }
}

/// One column's heading, as the grid needs to draw and use it.
///
/// Borrowed from the source, so that asking about a column allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridColumn<'a> {
    /// The label drawn in the header, which is the name the query gave it.
    pub name: &'a str,
    /// What sort of values it holds.
    pub kind: GridColumnKind,
    /// Which edge those values line up against.
    ///
    /// Defaults to [`GridColumnKind::align`]; a source that knows better — a
    /// numeric column it means to show as an identifier, say — overrides it
    /// with [`GridColumn::aligned`].
    pub align: GridColumnAlign,
    /// Whether the column is part of the table's primary key.
    ///
    /// Drawn in [`Theme::grid_pk`](rugpui::Theme#structfield.grid_pk). It is
    /// also half of what decides whether a cell can be edited — an `UPDATE`
    /// needs a key to aim at (design notes, §7.5) — but only half, and
    /// not the half the grid reads: whether a *particular* cell will take an
    /// edit is [`GridSource::cell_editable`], because the answer depends on the
    /// query behind the result and not on the column alone. A result with no key
    /// column at all is a result no cell of which can be edited, and the source
    /// is the only thing that can say so.
    pub primary_key: bool,
}

impl<'a> GridColumn<'a> {
    /// A column of `kind` named `name`, aligned as that kind is usually
    /// aligned and not part of any key.
    pub fn new(name: &'a str, kind: GridColumnKind) -> Self {
        Self {
            name,
            kind,
            align: kind.align(),
            primary_key: false,
        }
    }

    /// Marks the column as part of the primary key.
    pub fn primary_key(mut self, primary_key: bool) -> Self {
        self.primary_key = primary_key;
        self
    }

    /// Overrides which edge the values line up against.
    pub fn aligned(mut self, align: GridColumnAlign) -> Self {
        self.align = align;
        self
    }
}

/// What one cell holds.
///
/// Four variants and not more, because a grid draws text: the codec decodes on
/// the way in, and what is left to decide here is only how a value is *shown*.
/// Three of them are a value the server has (or has not) got; the fourth is the
/// one thing a staging layer can put in a cell that no result set ever holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridCell<'a> {
    /// No value at all. Drawn as [`NULL_TEXT`] in the null colour, which is
    /// what tells it apart from `Text("")` — an empty cell that really does
    /// hold the empty string.
    Null,
    /// No value **yet**: the column has been left out of a row that is staged
    /// to be inserted, and the server will supply it.
    ///
    /// Drawn as [`DEFAULT_TEXT`], muted, the way a null is — and drawn
    /// differently from one on purpose. `DEFAULT` and `NULL` are the two things
    /// an omitted column can mean and they are not the same: leaving an
    /// auto-increment key out gets a key, writing NULL over it gets a rejected
    /// statement. A staging layer that could not tell the user which of the two
    /// a cell holds would have lost the distinction before the `INSERT` is even
    /// written (design notes, §7.9).
    ///
    /// Never returned by a source over a result set. A row that exists on the
    /// server has a value in every column, and the value may be [`GridCell::Null`].
    Default,
    /// The value, already a string. May be empty, and an empty one is not null.
    Text(&'a str),
    /// A large object, whose body is not here.
    ///
    /// Only the size travels with the row; the bytes are fetched in chunks when
    /// the cell is opened, which is what [`GridEvent::CellActivated`] is for.
    ///
    /// [`GridEvent::CellActivated`]: crate::GridEvent::CellActivated
    Lob {
        /// How many bytes the object runs to, when the driver said.
        size: Option<u64>,
    },
}

/// Whether the source has everything, or is still filling up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridSourceState {
    /// Every row of the result is in the source. Scrolling to the bottom asks
    /// for nothing.
    #[default]
    Complete,
    /// The server has more rows than the source holds. Approaching the bottom
    /// raises [`GridEvent::NearEnd`](crate::GridEvent::NearEnd).
    HasMore,
    /// A batch is on its way. The grid asks for nothing while one is, which is
    /// what keeps a fast scroll from firing a fetch per frame.
    Loading,
}

/// What has been staged against one row, and therefore how it is marked.
///
/// The grid draws a marker for anything but [`RowStatus::Unchanged`] and asks
/// no further questions: whether the change can be applied, what SQL it becomes
/// and when it is sent are the staging layer's, which is the only thing that
/// knows the table behind the result.
///
/// The four are the four a `DELETE`/`INSERT`/`UPDATE` batch can be in, and they
/// do not overlap: a row that was inserted and then changed again is still
/// [`RowStatus::Inserted`], because that is the statement it will become.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowStatus {
    /// Nothing has been staged against it. The overwhelming majority, and the
    /// answer every read-only source gives.
    #[default]
    Unchanged,
    /// At least one of its cells holds a value the server has not seen.
    ///
    /// Which cells is [`GridSource::cell_dirty`]; this only says that some are,
    /// so that the row can be found without reading every column of it.
    Modified,
    /// The row is not in the table yet.
    Inserted,
    /// The row is in the table and is staged to go.
    ///
    /// Still drawn, and drawn in its place, rather than taken out of the
    /// result: a deletion that made the row vanish would renumber everything
    /// under it and leave the user nothing to change their mind about.
    Deleted,
}

/// What the grid already worked out about a cell, handed to
/// [`GridSource::render_cell`] so the host does not work it out again.
///
/// Everything here is a number or a flag the widget had to have to draw the
/// cell at all. A host that wanted the selected state would otherwise have to
/// keep a copy of the selection in step with the grid's, and one that wanted
/// the cell's width would have to mirror every column drag — two pieces of
/// duplicated state that could disagree, for information that is one field
/// away.
pub struct CellInfo<'a> {
    /// The kind of the column the cell is in, which is the whole of what the
    /// grid knows about its type.
    ///
    /// The alignment the grid would have used is
    /// [`GridColumnKind::align`] — but only *would have*: a custom element is
    /// given the bare box and lines its own content up. See
    /// [`GridSource::render_cell`].
    pub kind: GridColumnKind,
    /// Whether the cell is inside the selection.
    ///
    /// The grid has already painted the selection behind the element, so this
    /// is for content that has to *react* to it — an icon that goes to the
    /// selected foreground, say — rather than for painting it again.
    pub selected: bool,
    /// Whether [`GridSource::cell_dirty`] said the value is one the server has
    /// not seen. The grid has already painted the tint.
    pub dirty: bool,
    /// Whether the inline editor is open over this very cell.
    ///
    /// True for exactly one cell of one row, and only while somebody is
    /// editing. A host draws a quieter cell under an open editor, or nothing at
    /// all; the grid goes on drawing the cell either way, because the editor is
    /// a separate layer over the rows and not a replacement for one.
    pub editing: bool,
    /// How wide the cell's box is, borders included — the column's current
    /// width, whether that was fitted, dragged or defaulted.
    pub width: Pixels,
    /// How tall the cell's box is, which is the grid's row height and the same
    /// for every cell.
    pub height: Pixels,
    /// The palette this frame is being drawn with.
    ///
    /// Borrowed rather than cloned: a screenful is several hundred cells, and a
    /// theme is not a small struct. A host draws with these colours rather than
    /// with its own, so a custom cell follows the app's theme without being
    /// told it changed.
    pub theme: &'a Theme,
}

/// What the host is handed when it builds an editor of its own.
///
/// The cell's identity, what was in it, the box to draw in, and the two ways
/// out — which are the only things a custom editor cannot work out for itself,
/// since the grid stages nothing and knows where the cell is.
pub struct CellEditorContext {
    /// The row being edited.
    pub row: usize,
    /// The **source** column being edited, the same numbering
    /// [`GridSource::cell`] takes.
    pub column: usize,
    /// The cell's text, as [`cell_label`] would have drawn it — empty for a
    /// cell that holds no value.
    pub seeded: String,
    /// Whether the cell held no value at all, which `seeded` being empty does
    /// not say: a cell holding the empty string seeds the same empty editor and
    /// is not the same cell.
    pub was_null: bool,
    /// How wide the editor's box is: the column's current width.
    pub width: Pixels,
    /// How tall the editor's box is.
    pub height: Pixels,
    /// Stages a value and closes the editor.
    ///
    /// Raises [`GridEvent::EditCommitted`](crate::GridEvent::EditCommitted)
    /// unless the value is what the cell already held, exactly as the grid's
    /// own field does.
    pub commit: CellCommit,
    /// Closes the editor with nothing staged.
    pub cancel: CellCancel,
}

/// How a custom editor stages a value. See
/// [`CellEditorContext::commit`].
pub type CellCommit = Rc<dyn Fn(EditValue, &mut Window, &mut App)>;

/// How a custom editor gives up. See [`CellEditorContext::cancel`].
pub type CellCancel = Rc<dyn Fn(&mut Window, &mut App)>;

/// How the host builds its own editor. See [`CellEditor::Custom`].
pub type CellEditorBuilder = Rc<dyn Fn(&CellEditorContext, &mut Window, &mut App) -> AnyElement>;

/// What opens over a cell when it is edited.
///
/// Three, and only three, because they are the three shapes of *where the value
/// comes from*: the user types it, the user picks it out of a list the source
/// knows, or the host has something better than either. There is deliberately
/// no `Boolean` variant — a truth column is
/// `Choice { options: vec!["true".into(), "false".into()], nullable }`, which
/// spells the two values the way the server will read them back and lets a
/// dialect that says `t`/`f` say so.
#[derive(Clone)]
pub enum CellEditor {
    /// A one-line field seeded with the cell's text. The default, and what
    /// every source got before this existed.
    Text,
    /// A dropdown over a fixed list, opened at once over the cell.
    ///
    /// Picking a row stages it there and then — there is no `Enter` to press,
    /// because there is nothing half-typed to confirm.
    Choice {
        /// The values, in the order they are shown. They are the values
        /// themselves and not labels for them: what is picked is what is
        /// staged.
        options: Vec<SharedString>,
        /// Whether the list gains a leading [`NULL_TEXT`] row that stages
        /// [`EditValue::Null`] — the gesture that clears a cell rather than
        /// emptying it, which is the distinction this crate is built around.
        nullable: bool,
    },
    /// An element of the host's own: a date picker, a colour swatch, a lookup
    /// against another table.
    ///
    /// The host is handed a [`CellEditorContext`] and gives back an element.
    /// It should take the focus itself, because the grid's rules about closing
    /// are written in terms of the focus leaving; an editor that never calls
    /// `commit` or `cancel` is simply dismissed by `Escape` or by a click
    /// elsewhere, with nothing staged.
    Custom(CellEditorBuilder),
}

impl std::fmt::Debug for CellEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CellEditor::Text => f.write_str("Text"),
            CellEditor::Choice { options, nullable } => f
                .debug_struct("Choice")
                .field("options", options)
                .field("nullable", nullable)
                .finish(),
            // The closure has nothing printable in it.
            CellEditor::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

/// Where a [`GridView`](crate::GridView) gets its columns and rows.
///
/// Implemented on whatever the host already keeps the result in, so that there
/// is one copy of the data rather than two that can disagree. Every method is
/// asked only about what is on screen, and none of them may block: a grid over
/// a million rows calls [`GridSource::cell`] a few hundred times per frame and
/// never once for a row nobody can see.
pub trait GridSource: 'static {
    /// How many columns the result has, hidden ones included.
    fn column_count(&self) -> usize;

    /// The heading of column `index`.
    ///
    /// Asked once per visible column per frame, so it must be cheap; a source
    /// that would have to build the name should keep it.
    fn column(&self, index: usize) -> GridColumn<'_>;

    /// How many rows the source holds *now*.
    ///
    /// Not how many the query will return: a result being paged in grows this
    /// number batch by batch, and the grid follows it.
    fn row_count(&self) -> usize;

    /// The value at `row` and `column`.
    ///
    /// `column` is an index into the source's own columns, unaffected by which
    /// of them are hidden or how wide they have been dragged.
    fn cell(&self, row: usize, column: usize) -> GridCell<'_>;

    /// Whether more rows are coming.
    ///
    /// Defaults to [`GridSourceState::Complete`], which is right for a source
    /// that was handed a finished list.
    fn state(&self) -> GridSourceState {
        GridSourceState::Complete
    }

    /// What has been staged against `row`.
    ///
    /// Asked once per *visible* row per frame — the same budget
    /// [`GridSource::cell`] is held to, and for the same reason: a source that
    /// answered this by walking a million rows would undo the virtualisation on
    /// its own. A staging layer keeps its changes in a map keyed by row, so the
    /// answer is a lookup.
    ///
    /// Defaults to [`RowStatus::Unchanged`], which is the whole truth for a
    /// source nothing can be staged against.
    fn row_status(&self, _row: usize) -> RowStatus {
        RowStatus::Unchanged
    }

    /// Whether the value at `row` and `column` is one the server has not seen.
    ///
    /// The per-cell half of [`RowStatus::Modified`]: the row marker says the row
    /// was touched, this says where. Asked once per visible cell per frame.
    ///
    /// `column` is a source column, exactly as in [`GridSource::cell`], and the
    /// value [`GridSource::cell`] returns for a dirty cell is the *staged* one —
    /// the grid draws what it is given and knows nothing of what was there
    /// before.
    fn cell_dirty(&self, _row: usize, _column: usize) -> bool {
        false
    }

    /// Whether the cell at `row` and `column` will accept an edit.
    ///
    /// The gate on [`GridView::begin_edit`](crate::GridView::begin_edit) and on
    /// where `Tab` lands while editing, so it is asked per gesture rather than
    /// per frame — the grid never scans a row for it while drawing.
    ///
    /// A source says `false` for anything it could not turn into SQL it would be
    /// willing to send: a result with no key to aim an `UPDATE` at, a
    /// computed column, a `GridCell::Lob` whose body is not even here. Defaults
    /// to `false`, so a source that has not thought about editing cannot be
    /// edited by accident.
    fn cell_editable(&self, _row: usize, _column: usize) -> bool {
        false
    }

    /// Draws the cell at `row` and `column` itself, or `None` to let the grid
    /// draw its text.
    ///
    /// The way a result gets a badge, a bar, a swatch or a sparkline without
    /// the widget learning what any of those mean. What the grid keeps is
    /// everything *around* the content: the row stripe, the selection
    /// background, the dirty tint and the cursor outline are painted by the
    /// wrapper, under and over the element, so a custom cell is picked, tinted
    /// and outlined exactly as a plain one is.
    ///
    /// The contract, which is short on purpose:
    ///
    /// * The element is laid out in a box of `info.width` by `info.height` and
    ///   clipped to it. Unlike the grid's own text the box carries no padding
    ///   and no alignment — the whole cell is the host's, which is what lets a
    ///   bar reach the edges — so an element that wants to look like the
    ///   neighbouring cells supplies its own.
    /// * It is built **once per visible cell per frame**, the same budget
    ///   [`GridSource::cell`] is held to. Several hundred calls happen between
    ///   one frame and the next, so it must allocate little and compute
    ///   nothing.
    /// * It must not re-enter the grid. The widget is mid-render while this
    ///   runs; reading the palette out of `cx` is fine, updating the grid's
    ///   entity is not.
    /// * **[`GridSource::cell`] still has to answer.** Copying, column fitting
    ///   and the inline editor all read the cell's *text*, and none of them can
    ///   read an element. A cell drawn as a swatch is still copied as
    ///   `#3b82f6`.
    ///
    /// Defaults to `None` for every cell, which is the grid drawing its own
    /// text — what every source did before this existed.
    fn render_cell(
        &self,
        _row: usize,
        _column: usize,
        _info: &CellInfo<'_>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        None
    }

    /// What opens over the cell at `row` and `column` when it is edited.
    ///
    /// Asked by [`GridView::begin_edit`](crate::GridView::begin_edit), right
    /// after [`GridSource::cell_editable`] has said the cell will take an edit
    /// at all — so it is asked per gesture and never per frame, and a source
    /// may build the option list here rather than keeping one.
    ///
    /// Defaults to [`CellEditor::Text`], the field the grid has always opened.
    fn cell_editor(&self, _row: usize, _column: usize) -> CellEditor {
        CellEditor::Text
    }
}

/// How a large object is written where its bytes cannot go: in a cell, and in
/// every copied format.
///
/// Deliberately not a valid value in any of them. A LOB's body is not in the
/// grid, so it cannot be copied out of one, and a placeholder that could be
/// mistaken for data would be worse than one that cannot.
pub fn lob_label(size: Option<u64>) -> String {
    match size {
        Some(size) => format!("[LOB {size}]"),
        None => "[LOB]".to_string(),
    }
}

/// What one cell draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellLabel {
    /// The text itself, which is empty for a cell holding the empty string.
    pub text: SharedString,
    /// Whether the text stands in for a value rather than being one — the null
    /// marker, or a LOB placeholder — and is therefore drawn in
    /// [`Theme::grid_null`](rugpui::Theme#structfield.grid_null) instead of
    /// the ordinary foreground.
    pub muted: bool,
}

/// What `cell` draws, which is the whole of how null is told from empty.
///
/// Split out of the widget so that the distinction can be asserted without a
/// window: `cell_label(&GridCell::Null)` and `cell_label(&GridCell::Text(""))`
/// differ in both fields.
pub fn cell_label(cell: &GridCell<'_>) -> CellLabel {
    match cell {
        GridCell::Null => CellLabel {
            text: SharedString::new_static(NULL_TEXT),
            muted: true,
        },
        GridCell::Default => CellLabel {
            text: SharedString::new_static(DEFAULT_TEXT),
            muted: true,
        },
        GridCell::Text(text) => CellLabel {
            text: SharedString::from(text.to_string()),
            muted: false,
        },
        GridCell::Lob { size } => CellLabel {
            text: SharedString::from(lob_label(*size)),
            muted: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the whole crate exists to keep: a cell with no value and
    /// a cell with an empty value do not draw the same thing.
    #[test]
    fn null_and_the_empty_string_draw_differently() {
        let null = cell_label(&GridCell::Null);
        let empty = cell_label(&GridCell::Text(""));

        assert_eq!(null.text, NULL_TEXT);
        assert!(null.muted, "the null marker is not a value");
        assert_eq!(empty.text, "");
        assert!(!empty.muted, "the empty string is a value");
        assert_ne!(null, empty);
    }

    /// And the third empty-looking thing is not either of them: a column the
    /// server is going to fill in says so, rather than borrowing the null
    /// marker and claiming an `INSERT` will write NULL over a default.
    #[test]
    fn a_default_is_neither_null_nor_empty() {
        let default = cell_label(&GridCell::Default);

        assert_eq!(default.text, DEFAULT_TEXT);
        assert!(default.muted, "a default is not a value either");
        assert_ne!(default, cell_label(&GridCell::Null));
        assert_ne!(default, cell_label(&GridCell::Text("")));
    }

    /// And a cell holding the *string* `NULL` is not the null marker either:
    /// the text matches, the colour does not.
    #[test]
    fn the_string_null_is_not_the_null_marker() {
        let marker = cell_label(&GridCell::Null);
        let text = cell_label(&GridCell::Text(NULL_TEXT));

        assert_eq!(marker.text, text.text);
        assert_ne!(marker.muted, text.muted);
        assert_ne!(marker, text);
    }

    /// A LOB says how big it is and nothing else, because nothing else came.
    #[test]
    fn a_lob_shows_its_size() {
        assert_eq!(
            cell_label(&GridCell::Lob { size: Some(4096) }).text,
            "[LOB 4096]"
        );
        assert_eq!(cell_label(&GridCell::Lob { size: None }).text, "[LOB]");
        assert!(cell_label(&GridCell::Lob { size: None }).muted);
    }

    /// Numbers line up on the right and are written bare into SQL; everything
    /// else does neither.
    #[test]
    fn only_numbers_are_right_aligned_and_unquoted() {
        assert_eq!(GridColumnKind::Number.align(), GridColumnAlign::Right);
        assert!(!GridColumnKind::Number.quoted_in_sql());
        assert!(!GridColumnKind::Boolean.quoted_in_sql());

        for kind in [
            GridColumnKind::Text,
            GridColumnKind::Boolean,
            GridColumnKind::Temporal,
            GridColumnKind::Binary,
        ] {
            assert_eq!(kind.align(), GridColumnAlign::Left, "{kind:?}");
        }
        assert!(GridColumnKind::Temporal.quoted_in_sql());
        assert!(GridColumnKind::Binary.quoted_in_sql());
    }

    /// A source written before there was any such thing as an edit still
    /// compiles, and answers the three new questions the only way it could:
    /// nothing has been changed, and nothing may be.
    #[test]
    fn a_source_that_knows_nothing_of_editing_still_implements_the_trait() {
        struct Ancient;

        impl GridSource for Ancient {
            fn column_count(&self) -> usize {
                1
            }

            fn column(&self, _: usize) -> GridColumn<'_> {
                GridColumn::new("id", GridColumnKind::Number)
            }

            fn row_count(&self) -> usize {
                1
            }

            fn cell(&self, _: usize, _: usize) -> GridCell<'_> {
                GridCell::Text("1")
            }
        }

        let source = Ancient;
        assert_eq!(source.row_status(0), RowStatus::Unchanged);
        assert_eq!(RowStatus::default(), RowStatus::Unchanged);
        assert!(!source.cell_dirty(0, 0));
        assert!(
            !source.cell_editable(0, 0),
            "a source that never opted in was made editable"
        );
    }

    /// A column takes its alignment from its kind unless the source says
    /// otherwise.
    #[test]
    fn a_column_can_override_the_alignment_of_its_kind() {
        let id = GridColumn::new("id", GridColumnKind::Number).primary_key(true);
        assert_eq!(id.align, GridColumnAlign::Right);
        assert!(id.primary_key);

        let code = GridColumn::new("code", GridColumnKind::Number).aligned(GridColumnAlign::Left);
        assert_eq!(code.align, GridColumnAlign::Left);
        assert!(!code.primary_key);
    }
}
