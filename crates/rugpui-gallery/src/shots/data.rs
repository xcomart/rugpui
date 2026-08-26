//! The three widgets over data the host supplies: `TreeView`, `GridView` and
//! `EditorView`.
//!
//! Two of the sources here are the gallery's own, wrapped rather than copied:
//! [`Staged`] is [`Orders`](crate::data::Orders) with a staging layer's answers
//! over it, and [`Fetching`] is [`Catalog`](crate::data::Catalog) with one
//! branch still in flight. Both exist because a picture of an unchanged row and
//! a picture of a loaded branch says nothing about the two states the widgets
//! draw differently.

use gpui::{AnyElement, AnyView, App, Entity, Focusable, Window, prelude::*, px};
use rugpui::{ChildState, TreeRowInfo, TreeSource, TreeView};
use rugpui_editor::{EditorView, MarkKind, highlighter_for_extension};
use rugpui_grid::{GridCell, GridColumn, GridColumnKind, GridSource, GridView, RowStatus};

use super::{Motion, Shot, framed, panel};
use crate::{
    data::{Catalog, Orders},
    monospace,
};

/// Every shot on the tree, grid and editor pages.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "tree/expanded",
        width: 300.,
        height: 220.,
        per_theme: "",
        motion: Motion::Still,
        build: tree_expanded,
    },
    Shot {
        name: "tree/loading",
        width: 300.,
        height: 160.,
        per_theme: "",
        motion: Motion::Still,
        build: tree_loading,
    },
    Shot {
        name: "grid/default",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_default,
    },
    Shot {
        name: "grid/kinds",
        width: 640.,
        height: 172.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_kinds,
    },
    Shot {
        name: "grid/selection",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_selection,
    },
    Shot {
        name: "grid/staged",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_staged,
    },
    Shot {
        name: "grid/custom-cells",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_custom_cells,
    },
    Shot {
        name: "grid/choice-editor",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_choice_editor,
    },
    Shot {
        name: "grid/text-editor",
        width: 840.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: grid_text_editor,
    },
    Shot {
        name: "editor/sql",
        width: 560.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: editor_sql,
    },
    Shot {
        name: "editor/json",
        width: 560.,
        height: 260.,
        per_theme: "",
        motion: Motion::Still,
        build: editor_json,
    },
    Shot {
        name: "editor/find",
        width: 560.,
        height: 300.,
        per_theme: "",
        motion: Motion::Still,
        build: editor_find,
    },
    Shot {
        name: "editor/word-wrap",
        width: 560.,
        height: 200.,
        per_theme: "",
        motion: Motion::Still,
        build: editor_word_wrap,
    },
    Shot {
        name: "editor/theme",
        width: 560.,
        height: 200.,
        per_theme: "editor/theme-%s",
        motion: Motion::Still,
        build: editor_theme,
    },
];

// --- tree -------------------------------------------------------------------

/// Two levels open, a leaf selected, and both disclosure states on screen.
fn tree_expanded(_window: &mut Window, cx: &mut App) -> AnyView {
    let tree = cx.new(|cx| {
        let mut tree = TreeView::new(Catalog, cx);
        tree.expand(&"warehouse", cx);
        tree.expand(&"warehouse/public", cx);
        tree.set_selected(Some("warehouse/public/orders"), cx);
        tree
    });
    panel(cx, move |_window, cx| {
        framed(cx).flex_1().child(tree.clone()).into_any_element()
    })
}

/// A branch whose children are on their way: the placeholder row under it is
/// what [`ChildState::Loading`] draws.
fn tree_loading(_window: &mut Window, cx: &mut App) -> AnyView {
    let tree = cx.new(|cx| {
        let mut tree = TreeView::new(Fetching, cx);
        tree.expand(&"warehouse", cx);
        tree
    });
    panel(cx, move |_window, cx| {
        framed(cx).flex_1().child(tree.clone()).into_any_element()
    })
}

/// The gallery's catalogue with one branch still being fetched.
struct Fetching;

impl TreeSource for Fetching {
    type Id = &'static str;

    fn children(&self, parent: Option<&Self::Id>) -> ChildState<Self::Id> {
        match parent {
            Some(&"warehouse") => ChildState::Loading,
            other => Catalog.children(other),
        }
    }

    fn render_row(
        &self,
        id: &Self::Id,
        info: TreeRowInfo,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        Catalog.render_row(id, info, window, cx)
    }
}

// --- grid -------------------------------------------------------------------

/// The grid in its bordered box, which is how every grid shot is framed.
fn grid_panel<S: GridSource>(cx: &mut App, grid: Entity<GridView<S>>) -> AnyView {
    panel(cx, move |_window, cx| {
        framed(cx).flex_1().child(grid.clone()).into_any_element()
    })
}

/// The result as it arrives: autofitted columns, a key column in its own
/// colour, nulls drawn as `NULL` rather than as nothing.
fn grid_default(_window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| GridView::new(Plain, cx));
    grid_panel(cx, grid)
}

/// One column per [`GridColumnKind`] and one row per [`GridCell`] variant: the
/// alignment each kind chooses and the three ways a cell can hold no ordinary
/// text.
fn grid_kinds(_window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| GridView::new(Shapes, cx));
    grid_panel(cx, grid)
}

/// A rectangle of cells picked, with the cursor at the corner the selection
/// started from.
fn grid_selection(_window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| {
        let mut grid = GridView::new(Plain, cx);
        grid.select_cell(2, 1, cx);
        grid.extend_selection(4, 3, cx);
        grid
    });
    grid_panel(cx, grid)
}

/// What a staging layer's answers look like: a modified row, an inserted one, a
/// deleted one, and the tint on the individual cells that carry the change.
fn grid_staged(_window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| GridView::new(Staged, cx));
    grid_panel(cx, grid)
}

/// The two columns the gallery's source draws itself: a badge on `channel` and
/// a bar under `total`.
fn grid_custom_cells(_window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| {
        let mut grid = GridView::new(Orders, cx);
        grid.select_cell(1, 4, cx);
        grid
    });
    grid_panel(cx, grid)
}

/// The dropdown a `CellEditor::Choice` opens, over the cell it belongs to.
///
/// Its first row is the `NULL` the column is nullable for — the gesture that
/// clears a cell rather than emptying it — and the highlighted row is the value
/// the cell already holds.
fn grid_choice_editor(window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| {
        let mut grid = GridView::new(Orders, cx);
        // High up the result on purpose: the list hangs below the cell, and a
        // cell near the bottom would hang past the frame.
        grid.begin_edit(3, 4, window, cx);
        grid
    });
    grid_panel(cx, grid)
}

/// The field a `CellEditor::Text` opens, seeded with what the cell held.
fn grid_text_editor(window: &mut Window, cx: &mut App) -> AnyView {
    let grid = cx.new(|cx| {
        let mut grid = GridView::new(Orders, cx);
        grid.begin_edit(6, 5, window, cx);
        grid
    });
    grid_panel(cx, grid)
}

/// The gallery's result with the two hand-drawn columns left off.
///
/// [`Orders`] draws `channel` as a badge and `total` with a bar, which is
/// exactly what `grid/custom-cells` is for and exactly what gets in the way of
/// a picture of the *default* drawing.
struct Plain;

impl GridSource for Plain {
    fn column_count(&self) -> usize {
        Orders.column_count()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        Orders.column(index)
    }

    fn row_count(&self) -> usize {
        Orders.row_count()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        Orders.cell(row, column)
    }
}

/// The same result with a staging layer's answers over it.
struct Staged;

/// Which rows [`Staged`] reports as changed, and how.
const STAGED_ROWS: &[(usize, RowStatus)] = &[
    (1, RowStatus::Modified),
    (3, RowStatus::Inserted),
    (5, RowStatus::Deleted),
];

/// Which cells of those rows hold a value the server has not seen.
const STAGED_CELLS: &[(usize, usize)] = &[(1, 1), (1, 4), (3, 5)];

impl GridSource for Staged {
    fn column_count(&self) -> usize {
        Orders.column_count()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        Orders.column(index)
    }

    fn row_count(&self) -> usize {
        Orders.row_count()
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        // The inserted row has no value for `note` yet; the server will supply
        // one, which is `Default` and emphatically not `Null`.
        if row == 3 && column == 5 {
            return GridCell::Default;
        }
        Orders.cell(row, column)
    }

    fn row_status(&self, row: usize) -> RowStatus {
        STAGED_ROWS
            .iter()
            .find(|(staged, _)| *staged == row)
            .map_or(RowStatus::Unchanged, |(_, status)| *status)
    }

    fn cell_dirty(&self, row: usize, column: usize) -> bool {
        STAGED_CELLS.contains(&(row, column))
    }
}

/// A result whose whole point is its column kinds and its cell shapes.
struct Shapes;

/// The five kinds, one column each.
const SHAPE_COLUMNS: &[(&str, GridColumnKind)] = &[
    ("id", GridColumnKind::Number),
    ("name", GridColumnKind::Text),
    ("active", GridColumnKind::Boolean),
    ("created_at", GridColumnKind::Temporal),
    ("payload", GridColumnKind::Binary),
];

/// Four rows: ordinary values, a null, the empty string, and a `DEFAULT`.
const SHAPE_ROWS: usize = 4;

impl GridSource for Shapes {
    fn column_count(&self) -> usize {
        SHAPE_COLUMNS.len()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        let (name, kind) = SHAPE_COLUMNS[index];
        GridColumn::new(name, kind).primary_key(index == 0)
    }

    fn row_count(&self) -> usize {
        SHAPE_ROWS
    }

    fn cell(&self, row: usize, column: usize) -> GridCell<'_> {
        // The last column is a large object in every row, since its size is the
        // whole of what the grid has of it.
        if column == 4 {
            return GridCell::Lob {
                size: Some(4096 * (row as u64 + 1)),
            };
        }
        match (row, column) {
            // A null: no value at all.
            (1, 1..=3) => GridCell::Null,
            // The empty string, which is a value and is not a null.
            (2, 1) => GridCell::Text(""),
            // A column left out of a staged insert; the server will fill it in.
            (3, 2 | 3) => GridCell::Default,
            (_, 0) => GridCell::Text(["1", "2", "3", "4"][row]),
            (_, 1) => GridCell::Text(["Northwind Traders", "", "", "Calder Marine"][row]),
            (_, 2) => GridCell::Text("true"),
            (_, 3) => GridCell::Text("2026-02-03 09:14:22"),
            _ => GridCell::Null,
        }
    }
}

// --- editor -----------------------------------------------------------------

/// The editor in its bordered box, drawn in the fixed-pitch family the host
/// picked — the font goes on the container, not on the view.
fn editor_panel(cx: &mut App, editor: Entity<EditorView>) -> AnyView {
    let mono = monospace(cx);
    panel(cx, move |_window, cx| {
        framed(cx)
            .flex_1()
            .font_family(mono.clone())
            .text_size(px(12.5))
            .child(editor.clone())
            .into_any_element()
    })
}

/// A statement with a warning in the gutter on line 9 — the whole-document
/// verdict a host's parser reached, handed to an editor that has never heard of
/// one.
fn editor_sql(_window: &mut Window, cx: &mut App) -> AnyView {
    let editor = cx.new(|cx| {
        let mut editor =
            EditorView::new(cx).highlighter(highlighter_for_extension("sql").expect("sql"));
        editor.set_text(crate::data::SQL, cx);
        editor.set_marks(vec![(8, MarkKind::Warning)], cx);
        editor
    });
    editor_panel(cx, editor)
}

/// The same editor over a second grammar.
fn editor_json(_window: &mut Window, cx: &mut App) -> AnyView {
    let editor = cx.new(|cx| {
        let mut editor =
            EditorView::new(cx).highlighter(highlighter_for_extension("json").expect("json"));
        editor.set_text(crate::data::JSON, cx);
        editor
    });
    editor_panel(cx, editor)
}

/// The find bar down, with a query in it and every match marked.
///
/// The bar is opened the way a user opens it — by the `Find` action reaching a
/// focused editor — because there is no host-facing method that opens it, and
/// the action can only be dispatched once there is a rendered frame to dispatch
/// into.
fn editor_find(window: &mut Window, cx: &mut App) -> AnyView {
    let editor: Entity<EditorView> = cx.new(|cx| {
        let mut editor =
            EditorView::new(cx).highlighter(highlighter_for_extension("sql").expect("sql"));
        editor.set_text(crate::data::SQL, cx);
        editor.find_labels("Find", "Replace", cx);
        editor
    });

    let handle = editor.read(cx).focus_handle(cx);
    window.focus(&handle, cx);

    let armed = editor.clone();
    window.on_next_frame(move |window, cx| {
        window.dispatch_action(Box::new(rugpui_editor::editor::Find), cx);
        // After the bar exists, so the query lands in a field that is on screen.
        window.on_next_frame(move |_window, cx| {
            armed.update(cx, |editor, cx| editor.set_find_query("orders", cx));
        });
    });

    editor_panel(cx, editor)
}

/// A short listing, taken once per palette: the same code in each of the six
/// editor themes.
fn editor_theme(_window: &mut Window, cx: &mut App) -> AnyView {
    let editor = cx.new(|cx| {
        let mut editor =
            EditorView::new(cx).highlighter(highlighter_for_extension("sql").expect("sql"));
        editor.set_text(THEME_SQL, cx);
        editor.select_range(THEME_SELECTION, cx);
        editor
    });
    editor_panel(cx, editor)
}

/// One statement written on one long line, broken at the width of the text
/// area rather than run off to the right of it.
///
/// Focused, because the caret is half of what the picture is about: it sits on
/// the second row of the first line, which is a place there is no way to put it
/// while a line is a row.
fn editor_word_wrap(window: &mut Window, cx: &mut App) -> AnyView {
    let editor: Entity<EditorView> = cx.new(|cx| {
        let mut editor = EditorView::new(cx)
            .highlighter(highlighter_for_extension("sql").expect("sql"))
            .word_wrap(true);
        editor.set_text(WRAP_SQL, cx);
        editor.move_to(WRAP_CARET, cx);
        editor
    });

    let handle = editor.read(cx).focus_handle(cx);
    window.focus(&handle, cx);

    editor_panel(cx, editor)
}

/// The listing the word-wrap shot draws: a select list too long to fit, and two
/// short lines under it to show that a wrapped line is still one line.
const WRAP_SQL: &str = "\
SELECT o.order_id, o.placed_at, c.name, c.email, sum(l.quantity * l.unit_price) AS total FROM public.orders AS o
  JOIN public.customers AS c ON c.customer_id = o.customer_id
 WHERE o.channel = 'web';
";

/// Where the caret sits in it — inside `sum(`, which the break above puts on
/// the second row.
const WRAP_CARET: usize = 60;

/// The listing the per-palette editor shot draws.
///
/// Short, and chosen to reach as many token colours as it can: a comment, a
/// keyword, an identifier, a string, a number, an operator and a function.
const THEME_SQL: &str = "\
-- Orders that have not shipped, by value.
SELECT o.order_id,
       sum(l.quantity * l.unit_price) AS total
  FROM public.orders AS o
 WHERE o.channel = 'web'
   AND o.total > 250
 LIMIT 100;
";

/// What the per-palette editor shot leaves selected, so the selection colour is
/// in the picture beside the current-line band and the caret.
///
/// `o.order_id` on the second line: an identifier rather than a word of the
/// comment above it, so the selection is drawn over a colour the palette
/// actually chose.
const THEME_SELECTION: std::ops::Range<usize> = 49..59;
