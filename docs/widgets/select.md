# Select

A dropdown that picks one string out of a list: a trigger the height of a [`TextInput`](./text-input.md), and a deferred list that hangs beneath it. Reach for it for a one-of-many choice whose options are plain text — a driver name, a font family, an export format.

Source: [select.rs](../../crates/rugpui/src/select.rs). Re-exported as `rugpui::Select`.

Options are plain strings, and the text of an option is also its identity. That is what keeps the widget usable for lists the caller discovers at runtime — installed fonts, for one — without inventing ids for them. When the options carry colours instead, reach for [`SchemeSelect`](./scheme-select.md), which is the same control keyed by id.

## The state the host keeps

Three things, all owned by the parent view and passed back in on every render: the selected value, the open flag, and (optionally) the list's `ScrollHandle`. This is the gallery's dropdown, complete:

```rust
use gpui::SharedString;
use rugpui::Select;

Select::new("driver")
    .options(
        ["PostgreSQL", "MySQL", "Oracle", "SQLite", "SQL Server"]
            .map(SharedString::new_static),
    )
    .selected(Some(self.choice.clone()))
    .placeholder("Pick a driver")
    .open(self.select_open)
    .width(px(180.))
    .on_select({
        let this = cx.entity();
        move |_index, text, _window, cx| {
            let text = SharedString::from(text.to_owned());
            this.update(cx, |gallery, cx| {
                gallery.choice = text;
                cx.notify();
            });
        }
    })
    .on_open_change({
        let this = cx.entity();
        move |open, _window, cx| {
            this.update(cx, |gallery, cx| {
                gallery.select_open = open;
                cx.notify();
            });
        }
    })
```

The gallery keeps exactly two fields for it:

```rust
struct Gallery {
    choice: SharedString,
    select_open: bool,
    // …
}
```

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `Select::new` | `impl Into<ElementId>` | — | empty, closed, nothing selected |
| `.options` | `impl IntoIterator<Item = SharedString>` | empty | the options, in display order |
| `.selected` | `Option<impl Into<SharedString>>` | `None` | the picked value |
| `.placeholder` | `impl Into<SharedString>` | empty | muted text on the trigger while nothing is selected |
| `.open` | `bool` | `false` | whether the list is showing |
| `.width` | `Pixels` | trigger fills parent, list 320 px | width of both trigger and list |
| `.tab_index` | `isize` | not a tab stop | joins the window's tab ring |
| `.scroll_handle` | `ScrollHandle` | none | tracks the list's scroll, so the parent can reveal a row |
| `.scrollbar` | `Scrollbar` | none | overlay indicator down the open list |
| `.on_select` | `Fn(usize, &str, …)` | none | index and text of the option picked |
| `.on_open_change` | `Fn(bool, …)` | none | the open state the control would like |

`on_select` hands over **both** the zero-based index and the text. Key off the index when the list has a fixed shape — a leading "no choice" row, say — because the text is translated and comparing against it would break in every language but one. Key off the text for a list discovered at runtime.

`on_open_change` fires with `true` when the trigger is activated while closed, and with `false` when it is activated again, when a row is clicked, or when the pointer goes down anywhere outside the list.

## Selection, placeholder and the "no choice" row

A value the option list does not contain still shows on the trigger; it just highlights no row. With nothing selected, the row whose text *equals* the placeholder counts as the current one — so a list that offers an explicit "no choice" entry should spell that row exactly like the placeholder, and the open list will always show where the user stands.

## Keyboard and mouse

The control takes a single tab stop when `.tab_index(…)` is set. `Enter` and `Space` toggle the list, as they do for any focusable element in gpui.

| keys | effect |
| --- | --- |
| `Up` / `Down` (while open) | move the selection by one, **without wrapping**, scroll the row into view, and fire `on_select` |
| `Enter` / `Space` | toggle the list (gpui's default activation) |
| `Escape` | **not handled** — see below |

Arrow keys are ignored when any modifier is down, or when there are no options. They select immediately rather than deferring to a confirm key, so the parent sees each step as the user walks the list.

Closing on `Escape` is left to the parent on purpose, so a dialog can decide whether the key belongs to the dropdown or to itself.

Mouse: a click on the trigger toggles; a click on a row fires `on_select` and then `on_open_change(false)`; a press anywhere outside the list closes it through a full-window backdrop.

## Layout, and why the list needs a width

The list is drawn with `deferred` + `anchored` rather than inline, for the same reason a [`MenuButton`](./menu.md) dropdown is: a trigger inside a scrolling form would otherwise have its list clipped by that form. But an `anchored` element is absolutely positioned and therefore cannot inherit the trigger's width — which is why `.width(px)` sets *both*, and why leaving it out gives a trigger that fills its parent and a list that falls back to 320 px.

The trigger is 32 px tall, matching `TextInput`, so a form that mixes the two lines up. The list starts scrolling at 260 px and rows are 26 px. To put an overlay bar down it, build a [`Scrollbar`](./scrollbar.md) from the same handle you passed to `.scroll_handle(…)` and hand it to `.scrollbar(…)`; the owner answers the drag, because the id the bar was built with is what tells that drag from any other.

## Theme slots

| slot | where |
| --- | --- |
| `surface` | trigger background |
| `surface_hover` | trigger hover, and a hovered row |
| `surface_active` | background of the current row |
| `border` | trigger outline and list outline |
| `accent` | trigger border while focused, and the text of the current row |
| `background` | the list panel's fill |
| `text` | selected value, and a normal row |
| `text_muted` | the placeholder, and the `▾` chevron |

## Pitfalls

- **Nothing opens itself.** Store `open` and pass it back, or the list will never appear.
- **Set a width if the trigger is not full-width.** Otherwise the list is 320 px whatever the trigger is.
- **Arrow keys fire `on_select` as they move.** If your handler is expensive, debounce it — the widget will not.
- **`Escape` is yours to handle.**
- **`.selected` is a value, not an index.** Passing a string no option matches is legal and shows on the trigger; that is how a stale setting stays visible instead of looking like nothing was chosen.
