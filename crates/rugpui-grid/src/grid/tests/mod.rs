use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use gpui::{Entity, Modifiers, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};

use crate::source::{CellCancel, CellCommit, GridColumn, GridColumnKind};

use super::*;

/// The test display, and so the test window, is 1920 by 1080.
const WINDOW_WIDTH: f32 = 1920.;

/// How wide the cells have to play with, which is the window less the
/// gutter.
const CONTENT_WIDTH: f32 = WINDOW_WIDTH - GUTTER_WIDTH;

/// The vertical middle of body row `row`, in window coordinates.
fn row_y(row: usize) -> f32 {
    HEADER_HEIGHT + row as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.
}

/// The horizontal middle of display column `column`, in window coordinates,
/// with the columns at their default width and not scrolled sideways.
fn column_x(column: usize) -> f32 {
    GUTTER_WIDTH + column as f32 * DEFAULT_COLUMN_WIDTH + DEFAULT_COLUMN_WIDTH / 2.
}

/// What a source was asked for, so that "only what is on screen" can be
/// asserted rather than believed.
#[derive(Default)]
struct Probe {
    reads: Cell<usize>,
    max_row: Cell<usize>,
    min_column: Cell<usize>,
    max_column: Cell<usize>,
    /// The same two numbers for the edit-state questions, which are drawn
    /// per row and per cell and are therefore held to the same budget.
    marks: Cell<usize>,
    max_mark_row: Cell<usize>,
}

impl Probe {
    fn note(&self, row: usize, column: usize) {
        self.reads.set(self.reads.get() + 1);
        self.max_row.set(self.max_row.get().max(row));
        self.min_column.set(self.min_column.get().min(column));
        self.max_column.set(self.max_column.get().max(column));
    }

    fn note_mark(&self, row: usize) {
        self.marks.set(self.marks.get() + 1);
        self.max_mark_row.set(self.max_mark_row.get().max(row));
    }

    fn forget(&self) {
        self.reads.set(0);
        self.max_row.set(0);
        self.min_column.set(usize::MAX);
        self.max_column.set(0);
        self.marks.set(0);
        self.max_mark_row.set(0);
    }
}

/// A result of any size at all, generated rather than stored, that counts
/// what it was asked for.
struct Huge {
    rows: Cell<usize>,
    columns: usize,
    state: Cell<GridSourceState>,
    editable: bool,
    probe: Rc<Probe>,
}

impl Huge {
    fn new(rows: usize, columns: usize, probe: Rc<Probe>) -> Self {
        Self {
            rows: Cell::new(rows),
            columns,
            state: Cell::new(GridSourceState::Complete),
            editable: false,
            probe,
        }
    }

    fn growing(mut self) -> Self {
        self.state = Cell::new(GridSourceState::HasMore);
        self
    }

    /// Every cell of it takes an edit, for the tests about what happens to
    /// a field rather than about which cells may have one.
    fn editable(mut self) -> Self {
        self.editable = true;
        self
    }
}

impl GridSource for Huge {
    fn column_count(&self) -> usize {
        self.columns
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        // A `&'static str` rather than a built one: the point of the fixture
        // is that nothing per row or per column is allocated behind the
        // trait either.
        GridColumn::new("column", GridColumnKind::Text).primary_key(index == 0)
    }

    fn row_count(&self) -> usize {
        self.rows.get()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        self.probe.note(row, column);
        GridCell::Text("value")
    }

    fn state(&self) -> GridSourceState {
        self.state.get()
    }

    // Every row of the fixture claims to have been changed, so that a grid
    // that asked about one it cannot see would be caught by the counting
    // rather than by the answers happening to be cheap.
    fn row_status(&self, row: usize) -> RowStatus {
        self.probe.note_mark(row);
        RowStatus::Modified
    }

    fn cell_dirty(&self, row: usize, _column: usize) -> bool {
        self.probe.note_mark(row);
        true
    }

    fn cell_editable(&self, _row: usize, _column: usize) -> bool {
        self.editable
    }
}

/// A small result written out in full, for the tests that care what is in
/// the cells rather than how many of them were touched.
struct Small {
    headings: Vec<(&'static str, GridColumnKind)>,
    rows: Vec<Vec<Option<&'static str>>>,
}

impl GridSource for Small {
    fn column_count(&self) -> usize {
        self.headings.len()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        let (name, kind) = self.headings[index];
        GridColumn::new(name, kind)
    }

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        match self.rows[row][column] {
            Some(text) => GridCell::Text(text),
            None => GridCell::Null,
        }
    }
}

/// A result something has been staged against, which is the shape of the
/// overlay a host wraps a real one in: it knows which rows were touched,
/// which cells carry the change, and which columns will take one.
///
/// Column 0 is the key and refuses edits; 1 and 2 take them, and column 1 of
/// row 0 holds no value at all — the cell that must not turn into the empty
/// string by being looked at.
struct Staged {
    rows: Vec<Vec<Option<&'static str>>>,
    status: Vec<RowStatus>,
    dirty: Vec<(usize, usize)>,
    editable: Vec<usize>,
}

impl Staged {
    fn new() -> Self {
        Self {
            rows: vec![
                vec![Some("1"), None, Some("here")],
                vec![Some("2"), Some(""), Some("there")],
            ],
            status: vec![RowStatus::Unchanged, RowStatus::Unchanged],
            dirty: Vec::new(),
            editable: vec![1, 2],
        }
    }
}

impl GridSource for Staged {
    fn column_count(&self) -> usize {
        3
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        GridColumn::new(["id", "nothing", "note"][index], GridColumnKind::Text)
            .primary_key(index == 0)
    }

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        match self.rows[row][column] {
            Some(text) => GridCell::Text(text),
            None => GridCell::Null,
        }
    }

    fn row_status(&self, row: usize) -> RowStatus {
        self.status[row]
    }

    fn cell_dirty(&self, row: usize, column: usize) -> bool {
        self.dirty.contains(&(row, column))
    }

    fn cell_editable(&self, _row: usize, column: usize) -> bool {
        self.editable.contains(&column)
    }
}

/// A source that draws one of its three columns itself, and remembers every
/// cell it was asked to draw.
///
/// The rows are recorded rather than merely counted, because the claim
/// being tested is not "the hook is cheap" but "the hook is only asked
/// about what is on screen" — which is a claim about *which* rows, and a
/// count alone cannot tell a hundred calls on the visible rows from a
/// hundred calls halfway down a million.
struct Drawn {
    rows: usize,
    drawn: Rc<RefCell<Vec<usize>>>,
}

/// The column [`Drawn`] draws for itself.
const DRAWN_COLUMN: usize = 1;

impl GridSource for Drawn {
    fn column_count(&self) -> usize {
        3
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        GridColumn::new(["id", "badge", "note"][index], GridColumnKind::Text)
    }

    fn row_count(&self) -> usize {
        self.rows
    }

    fn cell(&self, _row: usize, _column: usize) -> GridCell<'_> {
        GridCell::Text("value")
    }

    fn render_cell(
        &self,
        row: usize,
        column: usize,
        info: &CellInfo<'_>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        if column != DRAWN_COLUMN {
            return None;
        }
        self.drawn.borrow_mut().push(row);
        Some(
            div()
                .w(info.width)
                .h(info.height)
                .bg(info.theme.accent)
                .into_any_element(),
        )
    }
}

/// A source whose second column is edited with a dropdown rather than a
/// field.
struct Chooser {
    nullable: bool,
    value: Option<&'static str>,
}

/// The column [`Chooser`] opens a dropdown over.
const CHOICE_COLUMN: usize = 1;

/// What that dropdown offers, before the `NULL` row is put in front of it.
const CHOICES: [&str; 3] = ["web", "store", "phone"];

impl GridSource for Chooser {
    fn column_count(&self) -> usize {
        2
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        GridColumn::new(["id", "channel"][index], GridColumnKind::Text)
    }

    fn row_count(&self) -> usize {
        3
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        if column != CHOICE_COLUMN {
            return GridCell::Text(["1", "2", "3"][row]);
        }
        match self.value {
            Some(value) => GridCell::Text(value),
            None => GridCell::Null,
        }
    }

    fn cell_editable(&self, _row: usize, column: usize) -> bool {
        column == CHOICE_COLUMN
    }

    fn cell_editor(&self, _row: usize, _column: usize) -> CellEditor {
        CellEditor::Choice {
            options: CHOICES.iter().map(|value| (*value).into()).collect(),
            nullable: self.nullable,
        }
    }
}

/// A source whose second column is edited by an element of the host's own,
/// which hands the two ways out back to the test.
struct Own {
    ways_out: Rc<RefCell<Option<(CellCommit, CellCancel)>>>,
}

impl GridSource for Own {
    fn column_count(&self) -> usize {
        2
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        GridColumn::new(["id", "when"][index], GridColumnKind::Text)
    }

    fn row_count(&self) -> usize {
        2
    }

    fn cell(&self, _row: usize, _column: usize) -> GridCell<'_> {
        GridCell::Text("2026-02-03")
    }

    fn cell_editable(&self, _row: usize, column: usize) -> bool {
        column == 1
    }

    fn cell_editor(&self, _row: usize, _column: usize) -> CellEditor {
        let ways_out = self.ways_out.clone();
        CellEditor::Custom(Rc::new(move |context, _window, _cx| {
            // A real host would build a date picker here and let it call
            // these two; the test keeps them and calls them itself.
            *ways_out.borrow_mut() = Some((context.commit.clone(), context.cancel.clone()));
            div().w(context.width).h(context.height).into_any_element()
        }))
    }
}

/// Three columns, two rows, and both of the values that too many tools
/// cannot tell apart.
fn null_and_empty() -> Small {
    Small {
        headings: vec![
            ("id", GridColumnKind::Number),
            ("nothing", GridColumnKind::Text),
            ("empty", GridColumnKind::Text),
        ],
        rows: vec![
            vec![Some("1"), None, Some("")],
            vec![Some("2"), Some("here"), Some("")],
        ],
    }
}

/// The longest value in the `placed_at` column of [`narrow_and_wide`], which
/// is what that column has to be wide enough for.
const LONGEST: &str = "2026-02-03 09:15:07";

/// Two columns that are nothing like the same width: a number two
/// characters wide and a timestamp that does not fit the default.
///
/// The shape the whole feature exists for — the gallery's grid, and every
/// result with an `id` and a date in it.
fn narrow_and_wide() -> Small {
    Small {
        headings: vec![
            ("id", GridColumnKind::Number),
            ("placed_at", GridColumnKind::Text),
        ],
        rows: vec![
            vec![Some("1"), Some("2026-02-03 09:14:22")],
            vec![Some("2"), Some(LONGEST)],
        ],
    }
}

/// What a column holding `text` ought to fit to, worked out the way the grid
/// works it out: shape the value with the window's own text system, then add
/// what the cell's padding and border take out of the width.
fn fitted_width_of(text: &str, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, _| {
        let font = window.text_style().font();
        (shaped_width(text, &font, px(TEXT_SIZE), window) + CELL_PADDING * 2. + CELL_BORDER).ceil()
    })
}

/// A view that does nothing but hold the grid, as a result panel would.
struct Harness<S: GridSource> {
    grid: Entity<GridView<S>>,
}

impl<S: GridSource> Render for Harness<S> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.grid.clone())
    }
}

/// Everything a test reads back: the grid, and what it announced.
struct Handles<S: GridSource> {
    grid: Entity<GridView<S>>,
    events: Rc<RefCell<Vec<GridEvent>>>,
}

impl<S: GridSource> Handles<S> {
    /// Everything announced since the last look.
    fn drain(&self) -> Vec<GridEvent> {
        self.events.borrow_mut().drain(..).collect()
    }

    /// Reads something off the grid.
    fn read<R>(&self, cx: &mut VisualTestContext, f: impl FnOnce(&GridView<S>) -> R) -> R {
        cx.update(|_, cx| f(self.grid.read(cx)))
    }

    /// Changes the grid, and lets the frame it asks for happen.
    fn update(
        &self,
        cx: &mut VisualTestContext,
        f: impl FnOnce(&mut GridView<S>, &mut Context<GridView<S>>),
    ) {
        cx.update(|_, cx| self.grid.update(cx, f));
        cx.run_until_parked();
    }

    /// Changes the grid where a window is needed too, which everything
    /// about the inline editor is: it has a focus to take.
    fn update_in<R>(
        &self,
        cx: &mut VisualTestContext,
        f: impl FnOnce(&mut GridView<S>, &mut Window, &mut Context<GridView<S>>) -> R,
    ) -> R {
        let out = cx.update(|window, cx| self.grid.update(cx, |grid, cx| f(grid, window, cx)));
        cx.run_until_parked();
        out
    }

    /// What is in the inline editor, if one is open.
    fn typed(&self, cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| {
            self.grid
                .read(cx)
                .editor()
                .map(|input| input.read(cx).content().to_string())
        })
    }

    /// The cells the selection covers, as `(row, display column)`.
    fn selected(
        &self,
        cx: &mut VisualTestContext,
        rows: usize,
        columns: usize,
    ) -> Vec<(usize, usize)> {
        self.read(cx, |grid| {
            (0..rows)
                .flat_map(|row| (0..columns).map(move |column| (row, column)))
                .filter(|(row, column)| grid.is_selected(*row, *column))
                .collect()
        })
    }
}

/// Opens a focused grid over `source` and hands back its handles.
///
/// **Fixed widths**, unlike a grid a host gets: nearly every test here works
/// out where to click from `DEFAULT_COLUMN_WIDTH`, and a grid that sized its
/// own columns would slide the target out from under that arithmetic. It
/// also keeps the "only the visible rows are read" tests honest — a fit
/// reads the sample, which is exactly the budget those tests are policing.
/// The tests that are *about* fitting use [`open_fitted`].
fn open<S: GridSource>(source: S, cx: &mut TestAppContext) -> (Handles<S>, VisualTestContext) {
    open_with(source, true, false, cx)
}

/// The same, with the column fitting a host gets by default left switched
/// on.
fn open_fitted<S: GridSource>(
    source: S,
    cx: &mut TestAppContext,
) -> (Handles<S>, VisualTestContext) {
    open_with(source, false, false, cx)
}

/// The same again, wired the way a host wires it: an activated cell opens
/// whatever [`GridSource::cell_editor`] asks for over it.
///
/// Only for the tests about that round trip — a double click reaching a
/// dropdown — since everywhere else the extra subscription would open an
/// editor the test did not ask for.
fn open_activating<S: GridSource>(
    source: S,
    cx: &mut TestAppContext,
) -> (Handles<S>, VisualTestContext) {
    open_with(source, true, true, cx)
}

fn open_with<S: GridSource>(
    source: S,
    fixed: bool,
    activating: bool,
    cx: &mut TestAppContext,
) -> (Handles<S>, VisualTestContext) {
    cx.update(rugpui::init);
    cx.update(crate::init);

    let events: Rc<RefCell<Vec<GridEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let window = cx.add_window({
        let events = events.clone();
        move |window, cx| {
            let grid = cx.new(|cx| {
                let grid = GridView::new(source, cx);
                if fixed { grid.fixed_widths() } else { grid }
            });
            // Cloned rather than copied: `GridEvent::EditCommitted` carries
            // the value that was staged.
            cx.subscribe(&grid, move |_: &mut Harness<S>, _, event: &GridEvent, _| {
                events.borrow_mut().push(event.clone());
            })
            .detach();
            if activating {
                cx.subscribe_in(
                    &grid,
                    window,
                    |_: &mut Harness<S>, grid, event: &GridEvent, window, cx| {
                        if let GridEvent::CellActivated { row, column } = event {
                            grid.update(cx, |grid, cx| grid.begin_edit(*row, *column, window, cx));
                        }
                    },
                )
                .detach();
            }
            Harness { grid }
        }
    });
    let grid = window
        .update(cx, |harness, _, _| harness.grid.clone())
        .expect("the window is open");

    let mut cx = VisualTestContext::from_window(*window.deref(), cx);
    cx.update(|window, cx| {
        let handle = grid.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    });
    cx.run_until_parked();

    (Handles { grid, events }, cx)
}

/// Presses and releases the left button over a point, with modifiers.
fn click_at(cx: &mut VisualTestContext, x: f32, y: f32, modifiers: Modifiers, count: usize) {
    let position = point(px(x), px(y));
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers,
        button: MouseButton::Left,
        click_count: count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers,
        button: MouseButton::Left,
        click_count: count,
    });
    cx.run_until_parked();
}

/// A plain click on the cell at `row` and display column `column`.
fn click_cell(cx: &mut VisualTestContext, row: usize, column: usize) {
    click_at(cx, column_x(column), row_y(row), Modifiers::none(), 1);
}

/// Height of one row of an open [`Select`], from `select.rs`.
const SELECT_ROW_HEIGHT: f32 = 26.;

/// How far below the top of the editor's box a [`Select`] hangs its list,
/// which is its trigger height plus the gap it keeps — `DROP_OFFSET` in
/// `select.rs`.
const SELECT_DROP: f32 = 32. + 4.;

/// The padding above the first row of an open list, `py(4.)` in
/// `select.rs`.
const SELECT_LIST_PADDING: f32 = 4.;

/// The middle of row `index` of the list an open dropdown drew over the
/// cell at `row` and display column `column`, in window coordinates.
///
/// Worked out rather than looked up, because the list is a `deferred`
/// element of another crate and there is nothing to look it up in: the
/// editor's box is where `render_editor` puts it, and the list hangs off
/// that box by the offsets `select.rs` writes down.
fn option_at(row: usize, column: usize, index: usize) -> (f32, f32) {
    let left = GUTTER_WIDTH + column as f32 * DEFAULT_COLUMN_WIDTH;
    let top = HEADER_HEIGHT + row as f32 * ROW_HEIGHT - (EDITOR_HEIGHT - ROW_HEIGHT) / 2.;
    (
        left + DEFAULT_COLUMN_WIDTH / 2.,
        top + SELECT_DROP
            + SELECT_LIST_PADDING
            + index as f32 * SELECT_ROW_HEIGHT
            + SELECT_ROW_HEIGHT / 2.,
    )
}

/// Clicks row `index` of the list an open dropdown drew over that cell.
fn click_option(cx: &mut VisualTestContext, row: usize, column: usize, index: usize) {
    let (x, y) = option_at(row, column, index);
    click_at(cx, x, y, Modifiers::none(), 1);
}

/// Presses and releases the right button over a point, and hands back where
/// it was pressed — which is what the event carries.
fn right_click_at(cx: &mut VisualTestContext, x: f32, y: f32) -> Point<Pixels> {
    let position = point(px(x), px(y));
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
    position
}

mod columns;
mod editing;
mod interaction;
