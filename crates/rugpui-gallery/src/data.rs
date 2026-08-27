//! The sample data the larger widgets are pointed at.
//!
//! The traits are the whole of what a host has to write: the tree asks a
//! [`TreeSource`] for children and for a row, the list asks a [`ListSource`]
//! for a count, an id and a row, and the grid asks a [`GridSource`] for columns
//! and cells. None of the three fetches anything, which is why a fixture like
//! this is twenty lines rather than a database.

use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
    svg,
};
use rugpui::{ChildState, ListRowInfo, ListSource, TreeRowInfo, TreeSource, theme};
use rugpui_grid::{CellEditor, CellInfo, GridCell, GridColumn, GridColumnKind, GridSource};

use crate::{FILE, FOLDER, icon_tint};

// --- the tree ---------------------------------------------------------------

/// One node: its id, the label drawn for it, and the ids of its children.
///
/// A node with no children is a leaf and gets no disclosure arrow. Ids are
/// paths because a tree's ids have to be stable across a reload — everything
/// the widget remembers, which node is open and which is selected, is keyed by
/// them.
type Node = (&'static str, &'static str, &'static [&'static str]);

/// The outermost level.
const ROOTS: &[&str] = &["warehouse", "reporting"];

/// Every node of the sample catalogue.
const NODES: &[Node] = &[
    (
        "warehouse",
        "warehouse",
        &["warehouse/public", "warehouse/staging"],
    ),
    (
        "warehouse/public",
        "public",
        &[
            "warehouse/public/orders",
            "warehouse/public/customers",
            "warehouse/public/line_items",
        ],
    ),
    ("warehouse/public/orders", "orders", &[]),
    ("warehouse/public/customers", "customers", &[]),
    ("warehouse/public/line_items", "line_items", &[]),
    (
        "warehouse/staging",
        "staging",
        &["warehouse/staging/import"],
    ),
    ("warehouse/staging/import", "import", &[]),
    ("reporting", "reporting", &["reporting/daily"]),
    ("reporting/daily", "daily", &[]),
];

/// A read-only catalogue: everything is already in memory, so no node is ever
/// [`ChildState::NotLoaded`] and the tree never has to ask for a fetch.
pub struct Catalog;

impl Catalog {
    /// The row of `NODES` for `id`.
    fn node(id: &str) -> Option<&'static Node> {
        NODES.iter().find(|(node, _, _)| *node == id)
    }
}

impl TreeSource for Catalog {
    type Id = &'static str;

    fn children(&self, parent: Option<&Self::Id>) -> ChildState<Self::Id> {
        match parent {
            None => ChildState::Loaded(ROOTS.to_vec()),
            Some(id) => match Self::node(id) {
                Some((_, _, [])) | None => ChildState::Leaf,
                Some((_, _, children)) => ChildState::Loaded(children.to_vec()),
            },
        }
    }

    fn render_row(
        &self,
        id: &Self::Id,
        info: TreeRowInfo,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let palette = theme(cx);
        let label = Self::node(id).map_or(*id, |(_, label, _)| *label);
        let icon = if info.has_children { FOLDER } else { FILE };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(
                svg()
                    .size(px(14.))
                    .flex_none()
                    .path(icon)
                    .text_color(icon_tint(info.selected, &palette)),
            )
            .child(label)
            .into_any_element()
    }
}

// --- the list ---------------------------------------------------------------

/// One row of the sample list: its id, the name on the first line, the note on
/// the second, and the word in the pill at the end of that line.
type Contact = (&'static str, &'static str, &'static str, &'static str);

/// Seven people, which is more than fits in the gallery's 180 px box — the
/// list has to be scrollable to be worth a picture.
const CONTACTS: &[Contact] = &[
    ("ada", "Ada Lovelace", "Analytics · London", "owner"),
    ("grace", "Grace Hopper", "Platform · Arlington", "admin"),
    ("alan", "Alan Turing", "Research · Wilmslow", "admin"),
    (
        "katherine",
        "Katherine Johnson",
        "Reporting · Hampton",
        "write",
    ),
    (
        "edsger",
        "Edsger Dijkstra",
        "Query planner · Austin",
        "read",
    ),
    ("barbara", "Barbara Liskov", "Storage · Cambridge", "read"),
    ("radia", "Radia Perlman", "Networking · Seattle", "read"),
];

/// A directory of people, drawn as two-line cards.
///
/// Everything the list knows how to draw is the row's height, its padding and
/// its background; the two lines, the weights, the pill and the way the second
/// line justifies against the row's width are all here, which is the whole
/// point of [`ListSource::render_item`]. The rows are 44 px tall because the
/// gallery asks for that height — the list virtualises on one height for every
/// row, so a card row is a taller *list*, not a taller row.
pub struct Contacts;

impl Contacts {
    /// The row of [`CONTACTS`] for `id`.
    fn contact(id: &str) -> Option<&'static Contact> {
        CONTACTS.iter().find(|(contact, _, _, _)| *contact == id)
    }
}

impl ListSource for Contacts {
    type Id = &'static str;

    fn len(&self) -> usize {
        CONTACTS.len()
    }

    fn id(&self, index: usize) -> Self::Id {
        CONTACTS[index].0
    }

    fn render_item(
        &self,
        id: &Self::Id,
        _info: ListRowInfo,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let palette = theme(cx);
        let Some((_, name, note, badge)) = Self::contact(id) else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .justify_center()
            .size_full()
            .gap(px(1.))
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(*name),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .text_size(px(10.5))
                    .text_color(palette.text_muted)
                    .child(div().truncate().child(*note))
                    .child(
                        // Outlined rather than filled: the row's own background
                        // changes under it when it is selected or hovered, and
                        // a pill painted in `surface_active` would disappear
                        // into the first of those.
                        div()
                            .flex_none()
                            .px(px(5.))
                            .rounded_full()
                            .border_1()
                            .border_color(palette.border)
                            .text_size(px(9.5))
                            .child(*badge),
                    ),
            )
            .into_any_element()
    }
}

// --- the grid ---------------------------------------------------------------

/// The headings, which are also what the "Scrollbar" list is filled with.
pub const COLUMN_NAMES: &[&str] = &[
    "order_id",
    "customer",
    "placed_at",
    "total",
    "channel",
    "shipped_at",
    "carrier",
    "tracking_no",
    "discount_code",
    "note",
];

/// The six columns of the sample result, with the shape of their values.
///
/// The kind is a hint and not a type: it decides which way a column lines up
/// and whether a value is quoted in generated SQL, and nothing else.
const COLUMNS: &[(&str, GridColumnKind)] = &[
    ("order_id", GridColumnKind::Number),
    ("customer", GridColumnKind::Text),
    ("placed_at", GridColumnKind::Temporal),
    ("total", GridColumnKind::Number),
    ("channel", GridColumnKind::Text),
    ("note", GridColumnKind::Text),
];

/// The `total` column, which is drawn with a bar under the number.
const TOTAL: usize = 3;

/// The `channel` column, which is drawn as a badge and edited with a dropdown.
const CHANNEL: usize = 4;

/// The `note` column, the one plain field left in the result.
const NOTE: usize = 5;

/// The three values `channel` is allowed to take.
///
/// The list a real host would have read out of a `CHECK` constraint or an
/// enum type; here it is written down, which is the same thing as far as
/// `cell_editor` is concerned.
const CHANNELS: [&str; 3] = ["web", "store", "phone"];

/// The largest `total` in [`ROWS`], which is what the bars are drawn as a
/// fraction of.
///
/// A constant rather than a scan: `render_cell` is called once per visible cell
/// per frame, so it may not go looking through the result to draw one of them.
const MAX_TOTAL: f32 = 5410.20;

/// The padding a cell the host draws has to supply for itself, matching what
/// the grid puts around its own text so a badge lines up with the values above
/// and below it.
const CELL_PADDING: f32 = 6.;

/// Twelve rows, four of which hold a null — which is drawn as `NULL` in the
/// null colour rather than as the empty string, the distinction the grid exists
/// to keep. The null in `channel` is there to be seen twice: the badge falls
/// back to the grid's own marker, and the dropdown over it opens on its `NULL`
/// row.
const ROWS: &[[Option<&str>; 6]] = &[
    [
        Some("10241"),
        Some("Northwind Traders"),
        Some("2026-02-03 09:14:22"),
        Some("1284.50"),
        Some("web"),
        Some("expedited"),
    ],
    [
        Some("10242"),
        Some("Blue Ridge Supply"),
        Some("2026-02-03 11:02:07"),
        Some("312.00"),
        Some("store"),
        None,
    ],
    [
        Some("10243"),
        Some("Harbour & Cole"),
        Some("2026-02-04 08:41:55"),
        Some("87.25"),
        Some("phone"),
        Some("gift wrap"),
    ],
    [
        Some("10244"),
        Some("Meridian Foods"),
        Some("2026-02-04 15:20:11"),
        Some("2940.00"),
        Some("web"),
        Some(""),
    ],
    [
        Some("10245"),
        Some("Kestrel Logistics"),
        Some("2026-02-05 07:03:48"),
        Some("15.99"),
        None,
        None,
    ],
    [
        Some("10246"),
        Some("Ashgrove Dairy"),
        Some("2026-02-05 13:37:02"),
        Some("448.10"),
        Some("store"),
        Some("split shipment"),
    ],
    [
        Some("10247"),
        Some("Pinewood Hardware"),
        Some("2026-02-06 10:11:39"),
        Some("76.40"),
        Some("phone"),
        Some("call before delivery"),
    ],
    [
        Some("10248"),
        Some("Selkirk Brewing"),
        Some("2026-02-06 16:55:20"),
        Some("1099.95"),
        Some("web"),
        None,
    ],
    [
        Some("10249"),
        Some("Tamarind Imports"),
        Some("2026-02-07 09:28:14"),
        Some("623.75"),
        Some("store"),
        Some("customs paperwork attached"),
    ],
    [
        Some("10250"),
        Some("Wrenfield Press"),
        Some("2026-02-07 12:04:59"),
        Some("208.00"),
        Some("web"),
        Some("invoice by email"),
    ],
    [
        Some("10251"),
        Some("Calder Marine"),
        Some("2026-02-08 08:16:33"),
        Some("5410.20"),
        Some("phone"),
        Some("pallet"),
    ],
    [
        Some("10252"),
        Some("Orchard Lane Foods"),
        Some("2026-02-08 14:49:06"),
        Some("94.60"),
        Some("store"),
        Some(""),
    ],
];

/// A finished result set: every row is here, so the grid never asks for more.
pub struct Orders;

impl GridSource for Orders {
    fn column_count(&self) -> usize {
        COLUMNS.len()
    }

    fn column(&self, index: usize) -> GridColumn<'_> {
        let (name, kind) = COLUMNS[index];
        // The key column is drawn in its own colour, and is half of what makes
        // a cell editable at all.
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

    /// Two of the six columns are drawn rather than written out.
    ///
    /// `channel` is one of three words, which reads better as a badge than as a
    /// lower-case word floating in a cell; `total` is a number whose *size*
    /// carries as much as its digits do, so it gets a bar along the bottom of
    /// the cell showing how it compares with the largest in the result. Every
    /// other column returns `None` and the grid draws its text, which is what
    /// the hook is for: it is an exception, not a replacement.
    ///
    /// Note what is *not* here. There is no selection background, no dirty tint
    /// and no cursor outline: the grid paints all three around whatever this
    /// returns. And the null `channel` returns `None` too, so that cell falls
    /// back to the grid's own `NULL` marker rather than a badge reading
    /// "NULL".
    fn render_cell(
        &self,
        row: usize,
        column: usize,
        info: &CellInfo<'_>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        let GridCell::Text(text) = self.cell(row, column) else {
            return None;
        };

        match column {
            CHANNEL => Some(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    // A custom cell is handed the bare box, so the padding that
                    // lines it up with the columns either side is its own.
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
            TOTAL => {
                let share = (text.parse::<f32>().unwrap_or_default() / MAX_TOTAL).clamp(0., 1.);
                Some(
                    div()
                        .relative()
                        .size_full()
                        .flex()
                        .items_center()
                        // Right-aligned by hand: the grid does not align a cell
                        // it did not draw, so the alignment the column's kind
                        // would have chosen is applied here.
                        .justify_end()
                        .px(px(CELL_PADDING))
                        .child(SharedString::from(text.to_owned()))
                        // Absolute, and therefore outside the padding: the bar
                        // is a measure of the whole cell and reads as one only
                        // if it starts at the edge.
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
        }
    }

    /// Two columns take an edit: the constrained one and the free-text one.
    ///
    /// `order_id` is the key an `UPDATE` would be aimed at and the other three
    /// are derived, so none of them is the gallery's to change.
    fn cell_editable(&self, _row: usize, column: usize) -> bool {
        matches!(column, CHANNEL | NOTE)
    }

    /// And they are edited differently, which is the whole point of the hook.
    ///
    /// A column with three legal values is a dropdown over those three — plus
    /// the `NULL` row, because the column really does hold a null in one of its
    /// rows and there has to be a way back to it. A free-text note is the field
    /// the grid has always opened.
    fn cell_editor(&self, _row: usize, column: usize) -> CellEditor {
        match column {
            CHANNEL => CellEditor::Choice {
                options: CHANNELS.iter().map(|value| (*value).into()).collect(),
                nullable: true,
            },
            _ => CellEditor::Text,
        }
    }
}

// --- the editors ------------------------------------------------------------

/// What the SQL editor holds.
pub const SQL: &str = "\
-- Orders that have not shipped, by value.
SELECT o.order_id,
       c.name AS customer,
       o.placed_at,
       sum(l.quantity * l.unit_price) AS total
  FROM public.orders AS o
  JOIN public.customers AS c ON c.id = o.customer_id
  LEFT JOIN public.line_items AS l ON l.order_id = o.order_id
 WHERE o.shipped_at IS NULL
   AND o.placed_at >= now() - interval '30 days'
 GROUP BY o.order_id, c.name, o.placed_at
 ORDER BY total DESC
 LIMIT 100;
";

/// What the JSON editor holds.
pub const JSON: &str = "\
{
  \"id\": \"solarized-dark\",
  \"name\": \"Solarized Dark\",
  \"dark\": true,
  \"colors\": {
    \"background\": \"#002b36\",
    \"accent\": \"#268bd2\",
    \"danger\": \"#dc322f\"
  },
  \"fallback\": null
}
";
