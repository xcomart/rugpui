# The grid

`rugpui-grid` is one widget — [`GridView`](../crates/rugpui-grid/src/grid.rs) — over a table of rows it never fetches itself. Reach for it when you have a result set: a query result, a `DESCRIBE`, an execution plan, a diff. Anything with columns and rows and more of them than fit on a screen.

The rows arrive through a trait the host implements, [`GridSource`](../crates/rugpui-grid/src/source.rs). That boundary is the whole design: the crate knows `rugpui` and gpui and nothing else, so it can be pointed at any shape of answer, and its own tests need no server of any kind.

## What it holds to

Five things, from the [crate header](../crates/rugpui-grid/src/lib.rs):

* **A million rows scroll without a stutter.** Neither axis lays out more than the viewport can show, and no per-frame work is proportional to the size of the result.
* **Null is not the empty string.** `GridCell::Null` draws the marker `NULL` in a muted colour; `GridCell::Text("")` draws an empty cell. They are different values and they look different. Two of the four copy formats can carry the difference; two cannot, and [`copy.rs`](../crates/rugpui-grid/src/copy.rs) says which.
* **The grid does not sort.** It holds the first *n* rows of an answer the server holds all of, so sorting what is here would put the wrong rows at the top. A header click raises `GridEvent::SortRequested` and nothing moves until the host comes back with new rows.
* **The grid does not stage an edit either.** It draws which rows and cells have been changed and it hosts the editor — because only it knows where a cell is on screen. What a staged value *becomes* is the host's, and reaches it as `GridEvent::EditCommitted`.
* **What a cell shows, and what opens over it, are the source's.** `render_cell` lets the source draw a cell itself; `cell_editor` lets it say whether that cell is edited with a field, a dropdown or something the host built. Both default to the grid's own behaviour.

## The shape of it

```mermaid
flowchart LR
    H["Host view<br/>(owns the result)"]
    S["GridSource<br/>column_count / column<br/>row_count / cell<br/>state / row_status<br/>cell_dirty / cell_editable<br/>render_cell / cell_editor"]
    G["GridView&lt;S&gt;<br/>selection, widths,<br/>hidden columns, sort marker,<br/>inline editor"]
    E["GridEvent"]

    H -- "implements" --> S
    S -- "asked only about<br/>what is on screen" --> G
    G -- "emits" --> E
    E -- "NearEnd / SortRequested /<br/>CellActivated / EditCommitted /<br/>ContextMenu" --> H
    H -- "source_mut / refresh / reset" --> G
```

Copying is deliberately *not* in that loop: gpui owns the clipboard and the grid owns the selection, so the grid does it itself.

Call `rugpui_grid::init(cx)` once at start-up, after `rugpui::init(cx)`, so the key bindings are registered. See [getting started](./getting-started.md) and the [README](../README.md) for the three `init`s.

## Implementing `GridSource`

Implement it on whatever the host already keeps the result in, so there is one copy of the data rather than two that can disagree. Every method is asked only about what is on screen, and none of them may block.

| method | required | returns | notes |
| --- | --- | --- | --- |
| `column_count(&self)` | yes | `usize` | Every column, hidden ones included. |
| `column(&self, index)` | yes | `GridColumn<'_>` | Asked once per visible column per frame — keep the name, don't build it. |
| `row_count(&self)` | yes | `usize` | How many rows are held *now*, not how many the query will return. A result being paged in grows this number and the grid follows it. |
| `cell(&self, row, column)` | yes | `GridCell<'_>` | `column` is a source column, unaffected by hiding or by dragged widths. |
| `state(&self)` | no | `GridSourceState` | Defaults to `Complete`. |
| `row_status(&self, row)` | no | `RowStatus` | Defaults to `Unchanged`. Asked once per *visible* row per frame — must be a lookup. |
| `cell_dirty(&self, row, column)` | no | `bool` | Defaults to `false`. Asked once per visible cell per frame. |
| `cell_editable(&self, row, column)` | no | `bool` | Defaults to `false`, so a source that has not thought about editing cannot be edited by accident. Asked per gesture, not per frame. |
| `render_cell(&self, row, column, info, window, cx)` | no | `Option<AnyElement>` | Defaults to `None`, which is the grid drawing its own text. Asked once per visible cell per frame. See [drawing a cell yourself](#drawing-a-cell-yourself). |
| `cell_editor(&self, row, column)` | no | `CellEditor` | Defaults to `CellEditor::Text`, the field the grid has always opened. Asked per gesture, right after `cell_editable`. See [choosing the editor](#choosing-the-editor). |

The five defaulted methods are the truth for the read-only sources — a plan, a `DESCRIBE`, a diff — which are half of what the grid is pointed at.

Here is the gallery's source, trimmed. See [`rugpui-gallery/src/data.rs`](../crates/rugpui-gallery/src/data.rs) for the whole of it:

```rust
use rugpui_grid::{GridCell, GridColumn, GridColumnKind, GridSource};

const COLUMNS: &[(&str, GridColumnKind)] = &[
    ("order_id", GridColumnKind::Number),
    ("customer", GridColumnKind::Text),
    ("placed_at", GridColumnKind::Temporal),
    ("total", GridColumnKind::Number),
    ("channel", GridColumnKind::Text),
    ("note", GridColumnKind::Text),
];

// `None` is a null; `Some("")` is the empty string. They are not the same cell.
const ROWS: &[[Option<&str>; 6]] = &[
    [Some("10241"), Some("Northwind Traders"), Some("2026-02-03 09:14:22"), Some("1284.50"), Some("web"),   Some("expedited")],
    [Some("10242"), Some("Blue Ridge Supply"), Some("2026-02-03 11:02:07"), Some("312.00"),  Some("store"), None],
    [Some("10244"), Some("Meridian Foods"),    Some("2026-02-04 15:20:11"), Some("2940.00"), Some("web"),   Some("")],
];

pub struct Orders;

impl GridSource for Orders {
    fn column_count(&self) -> usize {
        COLUMNS.len()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        let (name, kind) = COLUMNS[index];
        GridColumn::new(name, kind).primary_key(index == 0)
    }

    fn row_count(&self) -> usize {
        ROWS.len()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        match ROWS[row][column] {
            Some(text) => GridCell::Text(text),
            None => GridCell::Null,
        }
    }
}
```

### Values are already strings

`GridCell::Text` borrows from the source rather than owning, so drawing a screenful of cells allocates nothing per cell for the value itself. That only works because the values *are* strings by the time they reach here: whatever decodes the wire hands over its text already decoded. A source that would have to format a number on the way out has nowhere to put the result — which is the constraint saying that formatting belongs upstream.

### `GridColumn` and `GridColumnKind`

`GridColumn::new(name, kind)` builds a heading; `.primary_key(true)` marks it as part of the key and `.aligned(GridColumnAlign::Left)` overrides the alignment its kind would have chosen.

`GridColumnKind` is a **hint and not a type**. The grid uses it for exactly two things, both readable on the enum itself:

* `kind.align()` — only `Number` goes `GridColumnAlign::Right`, because a column of right-aligned digits can be read down for magnitude.
* `kind.quoted_in_sql()` — `Number` and `Boolean` are written bare into a copied `INSERT`; `Text`, `Temporal` and `Binary` are quoted, because a bare `2024-01-01` is arithmetic in more dialects than it is a date.

A source that cannot tell says `GridColumnKind::Text`, which is the safe answer to both questions and the `Default`.

### The four cells

| variant | drawn as | muted |
| --- | --- | --- |
| `GridCell::Null` | `NULL_TEXT` (`"NULL"`) | yes |
| `GridCell::Default` | `DEFAULT_TEXT` (`"DEFAULT"`) | yes |
| `GridCell::Text(&str)` | the text, possibly empty | no |
| `GridCell::Lob { size: Option<u64> }` | `lob_label(size)` — `[LOB 4096]` or `[LOB]` | yes |

`GridCell::Default` is never returned by a source over a result set: a row that exists on the server has a value in every column, and that value may be `Null`. It exists for a staging layer holding a row that is not inserted yet, where a column left out means "the server will supply it" — which is not the same as writing `NULL` over an auto-increment key.

The whole of "how a cell draws" is the free function `cell_label(&GridCell<'_>) -> CellLabel`, which returns `{ text: SharedString, muted: bool }`. It lives outside the widget so the null-versus-empty distinction can be asserted without a window.

### Paging: `GridSourceState` and `NearEnd`

`state()` returns `Complete` (nothing more is coming), `HasMore` (the server has rows the source does not) or `Loading` (a batch is on its way).

While the source says `HasMore`, the grid raises `GridEvent::NearEnd` once the viewport comes within a hundred rows of the last one held. It is raised **once per row count**: a burst of scrolling that never reaches new rows asks once. The host fetches, drops the batch into its source through `source_mut`, and the grid — now looking at a longer result — asks again when the new end comes into view. Returning `Loading` while a fetch is in flight is what keeps a fast scroll from firing one fetch per frame.

### Staged edits: `RowStatus` and `cell_dirty`

`row_status(row)` says how a whole row is marked — `Unchanged`, `Modified`, `Inserted` or `Deleted`. The four do not overlap: a row that was inserted and then changed again is still `Inserted`, because that is the statement it will become. A `Deleted` row is still drawn, in its place, with its values struck through; making it vanish would renumber everything under it and leave the user nothing to change their mind about.

`cell_dirty(row, column)` is the per-cell half of `Modified` — the row marker says the row was touched, this says where. A dirty cell is tinted with the accent colour behind its text. Note that `cell()` returns the *staged* value for a dirty cell: the grid draws what it is given and knows nothing of what was there before.


### Drawing a cell yourself

`render_cell` is offered every visible cell before the grid draws its text. Return `None` — the default — and the grid draws `cell_label(cell())` as it always has; return an element and that element goes into the cell's box instead.

```rust
fn render_cell(
    &self,
    row: usize,
    column: usize,
    info: &CellInfo<'_>,
    _window: &mut Window,
    _cx: &mut App,
) -> Option<AnyElement> { … }
```

`CellInfo` is everything the grid had already worked out to draw the cell at all, handed over so the host does not keep a second copy of it:

| field | is |
| --- | --- |
| `kind: GridColumnKind` | The column's kind — the whole of what the grid knows about its type. |
| `selected: bool` | Whether the cell is inside the selection. The grid has already painted it. |
| `dirty: bool` | What `cell_dirty` said. The grid has already painted the tint. |
| `editing: bool` | Whether the inline editor is open over this very cell. |
| `width: Pixels` | The column's current width, borders included — fitted, dragged or defaulted. |
| `height: Pixels` | The row height, the same for every cell. |
| `theme: &Theme` | The palette this frame is drawn with. Borrowed, because a screenful is several hundred cells. |

The contract, which is short on purpose:

* **The element is laid out in a box of `info.width` by `info.height`, and clipped to it.** Unlike the grid's own text that box carries **no padding and no alignment** — the whole cell is yours, which is what lets a bar reach the edges. An element that wants to look like the cells either side of it supplies both itself. The cell's box is `relative`, so an absolutely positioned child escapes any padding you added.
* **The grid still paints everything around it.** Row stripe, selection background, dirty tint, cursor outline: all four are the wrapper's, drawn under and over your element, so a custom cell is picked and marked exactly as a plain one is. Do not paint them again.
* **It is called once per visible cell per frame.** Several hundred calls between one frame and the next. Allocate little, compute nothing, and never scan the result — a bar that looked up the largest value in the column would be work proportional to the result, per cell, per frame.
* **It must not re-enter the grid.** The widget is mid-render while this runs. Reading the palette out of `cx` is fine; updating the grid's entity is not.
* **`cell()` still has to answer.** Copying, column fitting and the inline editor all read a cell's *text*, and none of them can read an element. A cell drawn as a swatch is still copied as `#3b82f6`.

The gallery draws two of its six columns ([`data.rs`](../crates/rugpui-gallery/src/data.rs)). `channel` is one of three words, which reads better as a badge:

```rust
CHANNEL => Some(
    div()
        .size_full()
        .flex()
        .items_center()
        // A custom cell is handed the bare box, so the padding that lines it
        // up with the columns either side is its own.
        .px(px(CELL_PADDING))
        .child(
            div()
                .flex_none()
                .px(px(6.))
                .rounded_full()
                .bg(info.theme.surface_active)
                .text_color(info.theme.text)
                .text_size(px(10.5))
                .child(SharedString::from(text.to_owned())),
        )
        .into_any_element(),
),
```

and `total` is a number whose size carries as much as its digits do, so it gets a thin bar along the bottom of the cell — absolutely positioned, and therefore outside the padding, because a measure of the whole cell reads as one only if it starts at the edge:

```rust
TOTAL => {
    let share = (text.parse::<f32>().unwrap_or_default() / MAX_TOTAL).clamp(0., 1.);
    Some(
        div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            // Right-aligned by hand: the grid does not align a cell it did not draw.
            .justify_end()
            .px(px(CELL_PADDING))
            .child(SharedString::from(text.to_owned()))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(3.))
                    .w(info.width * share)
                    .bg(info.theme.accent.opacity(0.35)),
            )
            .into_any_element(),
    )
}
_ => None,
```

Note what the gallery does *not* do. There is no selection background, no dirty tint and no cursor outline in either arm. And the one row whose `channel` is null returns `None` too, so that cell falls back to the grid's own `NULL` marker rather than a badge reading "NULL".

## Building the view

`GridView<S>` is an entity, created with `cx.new` and rendered as a child element, exactly like the tree:

```rust
let grid = cx.new(|cx| {
    let mut grid = GridView::new(Orders, cx).insert_table("public.orders");
    grid.select_cell(2, 1, cx);
    grid.extend_selection(4, 3, cx);
    grid
});
```

Give it a bounded box to live in — the gallery puts it in a bordered `div().h(px(256.))` — and render it with `self.grid.clone()`.

### Column widths

**Every column fits its content, by default.** A grid whose columns are all the same width shows a timestamp as `2026-02-03 09:14…`, and a timestamp the user has to drag a column to read is not a timestamp. So the first draw that has rows to measure sizes every column to what is in it. Call `.fixed_widths()` — or `.autofit(false)` — to turn that off, for a grid whose widths the host sets itself or one where two grids lining up column for column matters more than the values fitting.

The fit is a **measurement, not an estimate**: the candidate strings are shaped by the window's own text system, with the font and size the cells are drawn in, and the column is that width plus the cell's padding and border. Guessing from character counts is what leaves a column a few pixels short on a proportional face, and a few pixels short is an ellipsis. `Pinewood Hardware` and `Northwind Traders` are both seventeen characters and are not the same width, which is why counting characters can narrow the field but can never pick the winner.

Each column remembers where its width came from, and that decides what may happen to it next:

| how the width came about | what the grid may do to it |
| --- | --- |
| **default** — no rows to measure yet, or fitting is off | fit it as soon as there are rows |
| **fitted** — the grid measured it | *widen* it as later batches of the same result arrive, while the 500-row sample is still filling up; never narrow it |
| **user** — dragged by the grip, or set through `set_column_width` | nothing, until somebody asks: `reset`, `autofit_column`, `autofit_all_columns` |

The "never narrow" rule is the one that looks odd written down and is obvious in use: a column that shrank as page three landed would slide every column after it sideways under a pointer that was reading them, and that jitter is worse than a column a few pixels wider than it needs to be.

What it costs, per column: one pass over the sampled rows comparing character counts — allocating nothing, shaping nothing — which keeps every value within three characters of the longest, at most sixteen of them; then those, plus the heading, are shaped. Seventeen shaped lines at the very worst, and usually two or three. This is the one thing the grid does that is proportional to the size of the result rather than to the size of the window, and it is paid on the frame a result first has rows and on the frames a further batch arrives, not every frame. The fitting itself happens at the top of `render`, before the header and the rows are laid out, so the frame that decides a width is the frame that draws it — there is no flash of the default width.

| method | argument | effect |
| --- | --- | --- |
| `GridView::new` | `source: S, cx` | A grid over `source`, nothing selected, nothing sorted. |
| `.autofit(enabled)` | `bool` | Whether the grid sizes every column to its content. **On by default**; see [Column widths](#column-widths) (builder; consumes `self`). |
| `.fixed_widths()` | — | Every column at 140 px until something moves it — `autofit(false)`, spelled the way a host reads it (builder). |
| `.tab_index(index)` | `isize` | Places the grid in the window's tab order (builder; consumes `self`). |
| `.insert_table(table)` | `impl Into<SharedString>` | The table name written into a copied `INSERT` (builder). |
| `set_insert_table(table)` | `Option<SharedString>` | The same, after the fact. |
| `source()` / `source_mut(cx)` | — | Read the source, or change it — dropping a fetched batch in, most of the time. `source_mut` commits any edit in progress first and re-reads the shape on the next draw. |
| `refresh(cx)` | — | Re-read the source, for a change the grid cannot have seen. |
| `reset(cx)` | — | Throw away column widths, hidden flags, the selection, the sort marker and the scroll position, and fit the columns afresh — what a *new* result deserves, as opposed to another batch of the same one. |
| `selection()` | — | `&Selection`. |
| `is_selected(row, column)` | display column | Whether that cell is picked. |
| `sort()` / `set_sort(sort, cx)` | `Option<(usize, SortDirection)>` | Read the marker, or put it where the host says the result really is ordered — without asking for anything. |
| `toggle_sort(column, cx)` | source column | Walk the sort on one step (ascending, descending, none) and raise `SortRequested`. What a header click does. |
| `visible_rows()` | — | `Range<usize>`: the rows the list built last frame. |
| `visible_column_indices()` | — | `Vec<usize>`: the source columns showing, left to right. The index into this is a *display* column. |
| `column_width(column)` / `set_column_width(column, width, cx)` | source column, `f32` | Width in pixels, clamped so a column can still be found and dragged. Setting one makes it the *user's*: the grid stops sizing that column on its own. |
| `is_column_hidden(column)` / `set_column_hidden(column, hidden, cx)` | source column, `bool` | Hiding clears the selection, because display positions renumber. |
| `hidden_column_count()` | — | Whether "show every column" is worth offering. |
| `show_all_columns(cx)` | — | The way back: a hidden column has no heading to right click. |
| `column_name(column)` | source column | `Option<&str>`, for labelling a menu item. |
| `autofit_column(column, cx)` | source column | Fit one column to its content, sampling the first 500 rows and capped at 480 px. What a double click on the resize grip does. Explicit, so it takes back a width the user dragged. Lands on the next draw, which is where the measuring can happen. |
| `autofit_all_columns(cx)` | — | The same for every column at once, for a menu's "fit all columns". Also takes back dragged widths. |
| `select_cell(row, column, cx)` / `extend_selection(row, column, cx)` | display column | Replace or stretch, and scroll the cell into view. |
| `select_row(row, cx)` / `select_all(cx)` / `clear_selection(cx)` | — | As a click on a row number, `Ctrl+A`, and nothing. |
| `copy(format, cx)` | `CopyFormat` | Writes the selection to the clipboard. |
| `scroll_to_row(row, cx)` | `usize` | Brings a row into view. |
| `editing()` | — | `Option<(row, source column)>` while the inline editor is open. |
| `editor()` | — | `Option<&Entity<TextInput>>`, for reading the half-typed value. |
| `begin_edit(row, column, window, cx)` | source column | Opens the inline editor. Returns whether it opened. |
| `commit_edit(cx)` / `cancel_edit(cx)` | — | Close, staging or discarding. |

## Events

`GridView<S>` implements `EventEmitter<GridEvent>`, so a host subscribes once. The crate header's example, extended:

```rust
cx.subscribe_in(&grid, window, |view, grid, event, window, cx| match event {
    // The viewport is within a hundred rows of the end and the source said
    // there are more. Fetch, then drop the batch in through `source_mut`.
    GridEvent::NearEnd => view.fetch_next_batch(cx),

    // A header was clicked. `direction` is `None` on the third click, which
    // drops the ordering. Re-run the query with a new `ORDER BY`; the rows do
    // not move until you replace the source.
    GridEvent::SortRequested { column, direction } => {
        view.reorder(*column, *direction, cx)
    }

    // A double click, or `Enter` on the cursor cell. A LOB goes to a viewer;
    // anything writable goes to the editor. The grid raises the same event
    // either way, because which of the two a cell deserves depends on the
    // column's type and on whether the result can be written to.
    GridEvent::CellActivated { row, column } => {
        grid.update(cx, |grid, cx| grid.begin_edit(*row, *column, window, cx));
    }

    // The user staged something and it really is different from what the cell
    // held. Nothing has been staged by the grid: the cell goes on drawing
    // whatever `GridSource::cell` returns until your staging layer says
    // otherwise. `EditValue::Null` is the clearing gesture, not an empty
    // string — see below.
    GridEvent::EditCommitted { row, column, value } => match value {
        EditValue::Text(text) => view.stage(*row, *column, text, cx),
        EditValue::Null => view.stage_null(*row, *column, cx),
    },

    // A right click. The grid has already taken the focus and moved the
    // selection if it had to; the items, their strings and what they do are
    // yours.
    GridEvent::ContextMenu { target, position } => {
        view.open_menu(*target, *position, cx)
    }
})
.detach();
```

`GridEvent` is `Clone` but not `Copy`, because `EditCommitted` carries a value; every other variant is four words of nothing. Every `column` in every variant is a **source** column, unaffected by hiding or by dragged widths.

## Selection

[`selection.rs`](../crates/rugpui-grid/src/selection.rs) is pure arithmetic — no window, no source, no colours. A `Selection` is a `Vec<CellRange>` plus an *anchor* (the corner a `Shift` gesture pivots around) and a *cursor* (the cell the arrow keys move). Five methods cover every gesture: `replace`, `add`, `extend_to`, `replace_rows`/`add_rows`, and `select_all`. `clamp(rows, columns)` drops any part hanging over a source that shrank.

A `CellAddress` is `{ row, column }`, and its `column` is the column's **display position** — the nth column currently on screen, left to right — not the source column. Hiding a column therefore renumbers the ones after it, which is exactly why `set_column_hidden` and `show_all_columns` clear the selection. Addressing by source column instead would let a "rectangle" survive a hide by drawing with a hole in it, which is worse.

`selection.bounds()` is the smallest `CellRange` covering everything picked, and it is what a copy runs over.

### Mouse

| gesture | effect |
| --- | --- |
| click a cell | picks it; starts a drag |
| drag | stretches from the anchor |
| `Shift`-click | stretches the newest rectangle from the anchor |
| `Ctrl`/`Cmd`-click | adds a one-cell rectangle and pivots on it from now on |
| double click | raises `CellActivated` |
| click a row number | picks the whole row, full width of the visible columns |
| `Shift`/`Ctrl`-click a row number | grows the block, or adds a row to it |
| right click | takes the focus, moves the selection only if the press fell *outside* it, then raises `ContextMenu` — no drag, no activation |
| click a heading | `toggle_sort` on that column |
| drag a heading's right edge | resizes, and the width becomes the user's — the grid stops sizing that column itself |
| double click a heading's right edge | fits that column to its content again, dragged width and all |
| right click a heading or its grip | raises `ContextMenu` with `MenuTarget::Header` and leaves the selection alone |
| `Shift`+wheel | scrolls sideways, for a mouse with no horizontal wheel |

### Keys

Bound by `init` in the `GridView` key context, so the arrows and the clipboard chords go on meaning what they mean everywhere else in the app. `{mod}` is `cmd` on macOS and `ctrl` elsewhere.

| key | action |
| --- | --- |
| arrows | `MoveUp` / `MoveDown` / `MoveLeft` / `MoveRight` |
| `Shift`+arrows | `ExtendUp` / `ExtendDown` / `ExtendLeft` / `ExtendRight` |
| `Home` / `End` | `MoveRowStart` / `MoveRowEnd` |
| `{mod}-Home` / `{mod}-End` | `MoveFirst` / `MoveLast` |
| `PageUp` / `PageDown` | one screenful less a row, so the bottom row becomes the top one |
| `Shift-PageUp` / `Shift-PageDown` | the same, stretching |
| `{mod}-A` | `SelectAll` |
| `{mod}-C` | `CopyCells` — TSV |
| `Enter` | `Activate`: raises `CellActivated` on the cursor cell |

With nothing picked yet, the first keystroke lands on the first cell rather than one step away from it.

## Editing

The grid draws edit state and hosts the editor; it stages nothing and sends nothing. The editor has to live here for one reason: it is placed over a cell, and nothing else knows where a cell is.

`begin_edit(row, column, window, cx) -> bool` refuses when the cell is not there, its column is hidden, `cell_editable` says no (the default), or the cell holds a `GridCell::Lob` whose body is not in the grid. Then it asks [`cell_editor`](#choosing-the-editor) which of the three to open. The field — the default, and what every source got before there was a choice — is seeded with the cell's text; a `Null` or `Default` cell seeds an empty one, but the grid remembers that the cell held no value, so leaving that field empty commits nothing rather than quietly turning `NULL` into `''`.

**A close commits.** `Enter`, `Tab`, the focus going elsewhere, a sort, a refresh, a scroll that takes the row off screen, a column dragged — all of them raise `GridEvent::EditCommitted`. Only `Escape` throws the typing away. This is about the *field*: a dropdown and a custom editor have nothing half-finished in them, because both stage the moment the user picks, so the same gestures simply take them down with nothing staged. The asymmetry is deliberate: what is committed is *staged*, not sent, so the cost of committing something the user did not mean is one undo in the pending changes, while the cost of discarding is the typing. Committing an unchanged field raises nothing at all, so opening a cell, looking at it and moving on is silent either way.

While the editor is open a second key context, `GridCellEditor`, exists — `Escape` cancels, `Tab` and `Shift-Tab` commit and open the next or previous editable cell of the same row (stopping at the ends rather than wrapping onto another row). Those three keys mean nothing to a merely focused grid, and binding them on the grid's own context would take `Escape` away from the app for as long as a grid had the focus. The stack while typing reads `GridView > GridCellEditor > TextInput`, so the field's own bindings win: `Enter` is the field's `Submit`, and left and right walk the caret. Up and down are not the field's, so an arrow out of a field commits it and walks on, the way a spreadsheet does. The context is on the box the editor sits in whichever editor that is, which is what gives a dropdown and a host's own element the same `Escape`.

`EditValue` has two variants. `EditValue::Text(String)` is what was typed or picked — verbatim, neither parsed nor trimmed, because the grid has no idea what the column's type will make of it. `EditValue::Null` is the *clearing* gesture: `SET x = NULL`, not `SET x = ''`. Nothing about emptying a field raises it, because emptying a field is how the empty string is typed; it comes from the `NULL` row of a nullable dropdown, or from a custom editor that commits it. A cell that already held no value stages nothing when it arrives, exactly as an unchanged field does. A `Lob` for a body that arrives from a file is the one still to come.

### Choosing the editor

`cell_editor(row, column)` is asked once the cell has agreed to take an edit at all, so it runs per gesture and never per frame — a source may build its option list here rather than keeping one for every cell of the result.

```rust
pub enum CellEditor {
    Text,
    Choice { options: Vec<SharedString>, nullable: bool },
    Custom(Rc<dyn Fn(&CellEditorContext, &mut Window, &mut App) -> AnyElement>),
}
```

All three land in the same box over the cell, for the same reason: only the grid knows where a cell is. What differs is **when they stage**. A field stages on the close; a dropdown and a custom editor stage the moment the user picks, so everything that merely *closes* one of those — a scroll, a sort, a column dragged out from under it — takes it down with nothing staged.

**`Text`** is the default and the field described above.

**`Choice`** is a [`Select`](./widgets/select.md) opened over the cell, already open on the value the cell holds — a trigger the user had to click again would be one gesture too many. Clicking a row stages it there and then, with no `Enter` to press. With `nullable: true` the list gains a leading `NULL` row that stages `EditValue::Null`, which is how a user reaches the null a nullable column can hold; the row is told from a value row by *position*, so a column whose values include the string `NULL` does not clear itself when the user picks the value they meant. `Escape`, or a press anywhere outside the list, dismisses it with nothing staged.

The arrows walk the list without picking as they go, and `Enter` stages where they stopped. That is the one place the grid does not simply hand the keys to the control: the focus is on the box the list hangs from rather than on the trigger inside it, so `Select`'s own arrow handling never sees the keystroke — and an arrow that staged every row it passed over would write three values on the way to the fourth.

There is deliberately **no `Boolean` variant**. A truth column is

```rust
CellEditor::Choice {
    options: vec!["true".into(), "false".into()],
    nullable: true,
}
```

which spells the two values the way the server will read them back, and lets a dialect that says `t`/`f` say so. A checkbox could not.

The gallery's `channel` column is the worked example:

```rust
fn cell_editable(&self, _row: usize, column: usize) -> bool {
    matches!(column, CHANNEL | NOTE)
}

fn cell_editor(&self, _row: usize, column: usize) -> CellEditor {
    match column {
        CHANNEL => CellEditor::Choice {
            options: CHANNELS.iter().map(|value| (*value).into()).collect(),
            nullable: true,
        },
        _ => CellEditor::Text,
    }
}
```

**`Custom`** hands the host a `CellEditorContext` and takes back an element — a date picker, a colour swatch, a lookup against another table.

| field | is |
| --- | --- |
| `row`, `column` | Which cell, `column` being a source column as everywhere else. |
| `seeded: String` | The cell's text, empty for a cell that holds no value. |
| `was_null: bool` | Whether the cell held no value — which `seeded` being empty does not say, since a cell holding the empty string seeds the same empty editor. |
| `width`, `height` | The box the editor is laid out in. |
| `commit: Rc<dyn Fn(EditValue, &mut Window, &mut App)>` | Stages a value and closes. Raises `EditCommitted` unless the value is what the cell already held. |
| `cancel: Rc<dyn Fn(&mut Window, &mut App)>` | Closes with nothing staged. |

The element **should take the focus itself**: the grid's rules about closing are written in terms of the focus leaving, and an editor nobody can type into is a strange thing to open. It is put inside a box the grid focuses, so the focus staying inside that box keeps the editor open and `Escape` reaches the grid either way. An editor that never calls `commit` or `cancel` is not stuck — it is simply dismissed by `Escape` or by a click elsewhere, with nothing staged.

## Copying

Four formats, because four different things get done with a block of rows.

| `CopyFormat` | `label()` | shape | a null becomes |
| --- | --- | --- | --- |
| `Tsv` (default) | `"TSV"` | tab separated, quoted when a field holds a tab, newline or quote | an empty field |
| `Csv` | `"CSV"` | comma separated, RFC 4180 quoting | an empty field |
| `Json` | `"JSON"` | an array of objects keyed by column name | `null` |
| `Insert` | `"INSERT"` | one `INSERT` per row | `NULL` |

`CopyFormat::ALL` is all four in menu order.

Only JSON and `INSERT` are faithful about null. There is no way to write a null in a tab- or comma-separated field that an empty string could not also be — the format has one hole and two things to put in it — so TSV and CSV lose the distinction on the way out. Reach for the other two when it matters.

JSON writes numeric and boolean columns as JSON numbers and booleans **when their text really is one**, and as strings when it is not: a numeric column holding `1,234` is text, whatever the driver called it. `INSERT` writes numbers and booleans bare and quotes everything else with inner quotes doubled — note that MySQL reads a backslash inside a string literal as an escape unless `NO_BACKSLASH_ESCAPES` is set.

`Ctrl`-click can pick blocks no rectangle covers, and no format has a shape for that. The copy runs over `Selection::bounds` and the cells inside the box but outside the selection come out as nulls: nothing is dropped, and nothing unpicked is included as a value. A LOB copies as its `[LOB …]` placeholder, which is deliberately not a valid value in any format — the body is not in the grid, so it cannot leave one.

`GridView::copy(format, cx)` does all of this and writes to the clipboard; an empty selection writes nothing rather than blanking it. The free function behind it is public for a host that wants the text without the clipboard:

```rust
use rugpui_grid::{CopyFormat, DEFAULT_INSERT_TABLE, copy_payload};

let text = copy_payload(&source, &columns, &selection, CopyFormat::Insert, DEFAULT_INSERT_TABLE);
```

`columns` maps display position to source column — `GridView::visible_column_indices()` — which is what makes a hidden column absent from the copy. `DEFAULT_INSERT_TABLE` is `"?table?"`, deliberately not valid SQL: a statement aimed at the wrong table is worse than one that will not parse until the name is filled in. Set the real one with `.insert_table("public.orders")`.

## Context menus

The grid raises `GridEvent::ContextMenu { target, position }` and stops. It does not name the items and does not run them, because this layer holds no strings. `position` is in **window** coordinates, which is what a menu anchors to.

`MenuTarget::Cell` covers the body — a cell or a row number. `MenuTarget::Header { column }` is a column heading, and `column` is the source column.

Everything such a menu needs is on `GridView` already: `copy`, `select_all`, `clear_selection`, `toggle_sort`, `set_column_hidden`, `show_all_columns`, `autofit_column`, `autofit_all_columns` to act, and `sort`, `is_column_hidden`, `hidden_column_count`, `column_name` to label and disable. The host stores the position, renders a [`ContextMenu`](./widgets/menu.md) and clears the position on dismiss:

```rust
GridEvent::ContextMenu { target, position } => {
    self.menu = Some((*target, *position));
    cx.notify();
}
```

```rust
// A helper on the host, called from its `render` and dropped in beside the
// grid with `.children(self.render_menu(cx))`.
fn render_menu(&self, cx: &mut Context<Self>) -> Option<ContextMenu> {
    let (target, position) = self.menu?;
    let grid = self.grid.clone();
    let entries = match target {
        MenuTarget::Cell => vec![
            MenuEntry::new("Copy").shortcut("Ctrl+C").on_activate({
                let grid = grid.clone();
                move |_window, cx| {
                    grid.update(cx, |grid, cx| grid.copy(CopyFormat::Tsv, cx));
                }
            }),
            MenuEntry::separator(),
            MenuEntry::new("Select all"),
        ],
        MenuTarget::Header { column } => {
            let view = grid.read(cx);
            let name = view.column_name(column).unwrap_or_default().to_string();
            vec![
                MenuEntry::new(format!("Hide {name}")),
                MenuEntry::new("Show all columns")
                    .disabled(view.hidden_column_count() == 0),
                MenuEntry::new("Fit to contents"),
            ]
        }
    };

    let this = cx.entity();
    Some(
        ContextMenu::new("grid-context")
            .position(position)
            .entries(entries)
            .on_dismiss(move |_window, cx| {
                this.update(cx, |view, cx| {
                    view.menu = None;
                    cx.notify();
                });
            }),
    )
}
```

The host keeps the open/closed state — the `Option<(MenuTarget, Point<Pixels>)>` above — exactly as it does for every other stateless widget in the kit.

## Theme slots

See [theming](./theming.md) for the palette as a whole. The grid draws with five slots of its own and five general ones:

| slot | where |
| --- | --- |
| `grid_header` | the column header band, and the row-number gutter |
| `grid_row_alt` | the background of every odd row |
| `grid_selection` | the background of a picked cell |
| `grid_null` | the text of a muted label: `NULL`, `DEFAULT`, a LOB placeholder |
| `grid_pk` | a primary-key column's heading text — the one thing a key gets |
| `border` | every cell and header edge |
| `text` | an ordinary heading and cell |
| `text_muted` | the row number in the gutter |
| `accent` | the sort marker, the cursor cell's outline, a dirty cell's tint, a `Modified` row's mark |
| `success` / `danger` | an `Inserted` / `Deleted` row's mark down the gutter |

The dirty tint and the row tint are drawn at low alpha (0.16 and 0.10) so the text keeps the contrast the palette promised it, and so a whole tinted row does not out-shout the selection drawn over it.

## Performance

From the [`grid.rs`](../crates/rugpui-grid/src/grid.rs) header. Nothing per frame is proportional to the number of rows or to the number of columns — only to the number of both that fit on screen.

* **Rows** go through gpui's `uniform_list`, which lays out only what the viewport can reach. `visible_rows()` is the range it built, and is what the guarantee is stated in.
* **Columns** are virtualised by hand, because there is no `uniform_list` for them and tables with several hundred columns are real. Every column's left edge is kept in a list rebuilt when a width changes, so finding the run the content area can see is two binary searches. A row is drawn as one absolutely positioned strip slid left by the scroll offset, not as a flex row of every cell with the invisible ones clipped.
* **The horizontal offset is the grid's own field**, not a gpui scroll container's — a scroll container lays its content out in full, which is exactly the cost being avoided. It also makes the header and the body trivially agree: they read the same number.
* **Hit testing is arithmetic**, not a listener per cell. A cell that answered presses would need an id and a hitbox, and a screenful is several hundred of both, every frame, for a gesture four numbers resolve.
* **The viewport is measured during prepaint** by a `canvas` in the body, which asks for a repaint when the width changed. A resize — and the very first frame — costs one extra frame and nothing after that.
* **Fitting a column shapes a handful of strings, not five hundred.** One pass over the 500-row sample — no allocation, nothing shaped — keeps every value within three characters of the longest, capped at sixteen; only those and the heading go to the text system. The character count narrows the field and deliberately does not pick the winner, because on a proportional face two values of the same length are not the same width. That is what makes an *exact* fit affordable, and exactness is the point: a width guessed from character counts is short by however far the font disagrees with the guess, and short is an ellipsis. Capped at 480 px, so one `TEXT` column cannot push the rest off the screen. This is the one piece of work here that scales with the result rather than with the window, and it is the reason it is bounded at 500 rows and done once per batch rather than once per frame.

The budget this buys is what `row_status`, `cell_dirty` and `render_cell` must respect: a source that answered any of them by walking a million rows would undo the virtualisation on its own. Keep staged changes in a map keyed by row so the answer is a lookup, and keep whatever a custom cell needs — the largest value in the column, a lookup table — beside the result rather than recomputing it per cell.

## Testing without a window

Half the crate needs no window at all. `selection.rs`, `copy.rs` and `source.rs` are pure, and their tests are plain `#[test]` functions over a twenty-line source — a struct of `Vec<Vec<Option<&'static str>>>` where `None` is a null and `Some("")` is the empty string:

```rust
struct Fixture {
    headings: Vec<(&'static str, GridColumnKind)>,
    rows: Vec<Vec<Option<String>>>,
}

impl GridSource for Fixture {
    fn column_count(&self) -> usize { self.headings.len() }
    fn row_count(&self) -> usize { self.rows.len() }

    fn column(&self, index: usize) -> GridColumn<'_> {
        let (name, kind) = self.headings[index];
        GridColumn::new(name, kind)
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        match self.rows[row][column].as_deref() {
            Some(text) => GridCell::Text(text),
            None => GridCell::Null,
        }
    }
}
```

For editing and staging, the tests add the three defaulted methods over plain fields — a `Vec<RowStatus>`, a `Vec<(usize, usize)>` of dirty cells and a `Vec<usize>` of editable columns — which is the shape of the overlay a host wraps a real result in.

The widget's own tests do need a window, and use gpui's `TestAppContext`. The pattern in `grid.rs` is worth copying: a `Harness` view that does nothing but `div().size_full().child(self.grid.clone())`, an `Rc<RefCell<Vec<GridEvent>>>` filled by a `cx.subscribe` so a test can drain what the grid announced, and helpers that `cx.update(…)` then `cx.run_until_parked()`. Mouse gestures are simulated with `cx.simulate_event(MouseDownEvent { … })` at coordinates worked out from the row height and the column widths, so a click on a given cell is a one-line helper — which is why the shared `open` helper there builds its grid with `.fixed_widths()`: a grid that sized its own columns would slide the target out from under that arithmetic. The tests that are about fitting open theirs the way a host gets one.

To count how often a source is touched, a source can note each call on a shared probe — which is how "does it still only touch the visible rows?" becomes an assertion rather than something to eyeball.
