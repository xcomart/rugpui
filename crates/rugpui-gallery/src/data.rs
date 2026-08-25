//! The sample data the three larger widgets are pointed at.
//!
//! Both traits are the whole of what a host has to write: the tree asks a
//! [`TreeSource`] for children and for a row, and the grid asks a
//! [`GridSource`] for columns and cells. Neither widget fetches anything, which
//! is why a fixture like this is twenty lines rather than a database.

use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, Window, div, px, svg};
use rugpui::{ChildState, TreeRowInfo, TreeSource, theme};
use rugpui_grid::{GridCell, GridColumn, GridColumnKind, GridSource};

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

/// The five columns of the sample result, with the shape of their values.
///
/// The kind is a hint and not a type: it decides which way a column lines up
/// and whether a value is quoted in generated SQL, and nothing else.
const COLUMNS: &[(&str, GridColumnKind)] = &[
    ("order_id", GridColumnKind::Number),
    ("customer", GridColumnKind::Text),
    ("placed_at", GridColumnKind::Temporal),
    ("total", GridColumnKind::Number),
    ("note", GridColumnKind::Text),
];

/// Twelve rows, three of which hold a null — which is drawn as `NULL` in the
/// null colour rather than as the empty string, the distinction the grid exists
/// to keep.
const ROWS: &[[Option<&str>; 5]] = &[
    [
        Some("10241"),
        Some("Northwind Traders"),
        Some("2026-02-03 09:14:22"),
        Some("1284.50"),
        Some("expedited"),
    ],
    [
        Some("10242"),
        Some("Blue Ridge Supply"),
        Some("2026-02-03 11:02:07"),
        Some("312.00"),
        None,
    ],
    [
        Some("10243"),
        Some("Harbour & Cole"),
        Some("2026-02-04 08:41:55"),
        Some("87.25"),
        Some("gift wrap"),
    ],
    [
        Some("10244"),
        Some("Meridian Foods"),
        Some("2026-02-04 15:20:11"),
        Some("2940.00"),
        Some(""),
    ],
    [
        Some("10245"),
        Some("Kestrel Logistics"),
        Some("2026-02-05 07:03:48"),
        Some("15.99"),
        None,
    ],
    [
        Some("10246"),
        Some("Ashgrove Dairy"),
        Some("2026-02-05 13:37:02"),
        Some("448.10"),
        Some("split shipment"),
    ],
    [
        Some("10247"),
        Some("Pinewood Hardware"),
        Some("2026-02-06 10:11:39"),
        Some("76.40"),
        Some("call before delivery"),
    ],
    [
        Some("10248"),
        Some("Selkirk Brewing"),
        Some("2026-02-06 16:55:20"),
        Some("1099.95"),
        None,
    ],
    [
        Some("10249"),
        Some("Tamarind Imports"),
        Some("2026-02-07 09:28:14"),
        Some("623.75"),
        Some("customs paperwork attached"),
    ],
    [
        Some("10250"),
        Some("Wrenfield Press"),
        Some("2026-02-07 12:04:59"),
        Some("208.00"),
        Some("invoice by email"),
    ],
    [
        Some("10251"),
        Some("Calder Marine"),
        Some("2026-02-08 08:16:33"),
        Some("5410.20"),
        Some("pallet"),
    ],
    [
        Some("10252"),
        Some("Orchard Lane Foods"),
        Some("2026-02-08 14:49:06"),
        Some("94.60"),
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
