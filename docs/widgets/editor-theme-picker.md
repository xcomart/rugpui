# EditorThemePicker

A grid of selectable cards, each previewing one editor theme by rendering a miniature SQL statement in it — gutter, current-line band, selection and caret included. Reach for it wherever the user chooses a *syntax* palette.

Source: [editor_theme_picker.rs](../../crates/rugpui/src/editor_theme_picker.rs). Re-exported as `rugpui::{EditorThemePicker, EditorThemeSwatch}`.

## Why cards and not swatches

A chrome theme can be previewed with a strip of coloured blocks, because it *is* a set of flat surfaces and a strip is an honest picture of one. A syntax palette cannot: its colours only mean anything in arrangement. What a reader is choosing between is not "is this purple nice" but "can I tell a keyword from a column name at a glance, and is the comment still legible". Swatches answer neither question, so each card renders a statement instead, painted entirely out of the theme it is offering.

The snippet is hardcoded, spans and all — there is no lexer in this crate and the preview does not need one. The card previews line 1's selection deliberately, so the current-line band, the selection and the caret are all visible at once and can be seen not to swallow one another, which is what most often goes wrong in a palette nobody checked.

For the one-line form control over the same kind of data, see [`SchemeSelect`](./scheme-select.md).

## Feeding it from `EditorThemeRegistry`

`EditorThemeRegistry::all(cx)` returns `EditorThemeEntry { id, name, dark, builtin }` rows — the six built-in palettes in presentation order, then the user's own sorted by name — and `EditorThemeRegistry::resolve(id, cx)` turns an id into the `EditorTheme` the card is painted with.

```rust
use rugpui::{EditorThemePicker, EditorThemeRegistry, EditorThemeSwatch};

fn editor_swatches(cx: &App) -> Vec<EditorThemeSwatch> {
    // The leading card offers "follow the app theme": no preview, so it draws
    // an outlined placeholder rather than a palette of its own.
    let follow = EditorThemeSwatch::new("", "Follow the app theme")
        .placeholder_label("follows the app theme");

    std::iter::once(follow)
        .chain(EditorThemeRegistry::all(cx).into_iter().map(|entry| {
            let preview = EditorThemeRegistry::resolve(&entry.id, cx);
            EditorThemeSwatch::new(entry.id, entry.name).preview(preview)
        }))
        .collect()
}
```

```rust
EditorThemePicker::new("editor-theme")
    .options(editor_swatches(cx))
    .selected(Some(self.editor_theme.clone()))
    .font_family(self.editor_font.clone())
    .columns(2)
    .on_select(cx.listener(|view, id: &str, _window, cx| {
        view.editor_theme = SharedString::from(id.to_owned());
        set_editor_theme(EditorThemeRegistry::resolve(id, cx), cx);
        cx.notify();
    }))
```

The picker is stateless: the parent owns the selected id, passes it in on every render, and stores what `on_select` hands back. There is no open flag here — the grid is always shown.

## EditorThemeSwatch

| method | argument | effect |
| --- | --- | --- |
| `EditorThemeSwatch::new` | id, name | an entry with **no** preview, drawn as a muted placeholder card |
| `.preview` | `EditorTheme` | the palette the card's snippet is painted with |
| `.placeholder_label` | `impl Into<SharedString>` | text on the placeholder card (default `"follows the app theme"`, English) |

The id is what `on_select` reports and what settings store; the name is drawn under the card and truncated if it does not fit.

## EditorThemePicker options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `EditorThemePicker::new` | `impl Into<ElementId>` | — | empty grid |
| `.options` | `impl IntoIterator<Item = EditorThemeSwatch>` | empty | the entries, in display order |
| `.selected` | `Option<impl Into<SharedString>>` | `None` | id of the highlighted card; an unknown id highlights nothing |
| `.columns` | `usize` | `2` | cards per row; zero is treated as one |
| `.font_family` | `impl Into<SharedString>` | inherited | family the snippet is drawn in |
| `.tab_index` | `isize` | not a tab stop | joins the window's tab ring |
| `.on_select` | `Fn(&str, …)` | none | **the id** of the picked entry |

Two columns rather than the three a swatch grid takes: a card has a statement in it, and a statement needs the width. The last row is padded with empty flex boxes so its cards keep the width of a full row instead of stretching.

`on_select` is never fired for the card that is already selected, and the selected card carries no click handler and no hover at all.

`.font_family` is passed in rather than resolved here: which monospace family is installed, and which one the user has pointed the editor at, are both questions this crate has no business answering. A caller that says nothing gets the surrounding font, which previews the colours correctly and only the metrics wrongly. The gallery's `monospace(cx)` helper shows how a host picks one.

## Keyboard

The grid takes a single tab stop when `.tab_index(…)` is set. While focused, the arrow keys move the selection within the grid, without wrapping — how a grid of radio buttons behaves everywhere else:

| key | effect |
| --- | --- |
| `Left` / `Right` | one card back / forward |
| `Up` / `Down` | one row back / forward (by `columns`) |

Modified keystrokes are ignored, and nothing happens while no card is selected — there is no cursor to move. Each step fires `on_select` immediately. This module registers no `KeyBinding`: the handlers are plain `on_key_down` listeners on the focused grid.

## Theme slots

The card *bodies* are painted entirely from the `EditorTheme` being previewed — `foreground`, `keyword`, `identifier`, `number`, `string`, `comment`, `operator`, `punctuation`, `key`, `variable`, plus `background`, `gutter`, `gutter_active`, `line_highlight`, `selection` and `cursor` for the frame. `error` and `warning` are deliberately left out: a card advertising a theme should not be drawing a broken statement.

The card *frames* come from the chrome `Theme`:

| slot | where |
| --- | --- |
| `surface` | a resting card's background |
| `surface_hover` | a hovered, unselected card |
| `surface_active` | the selected card's background |
| `border` | card outline, and the placeholder card's outline |
| `accent` | the selected card's outline, and the grid's focus ring |
| `text` | the selected card's name |
| `text_muted` | unselected card names, and the placeholder label |

## Pitfalls

- **The card must not look like the dialog.** Every colour inside the snippet comes from the previewed palette and none from the surrounding chrome; if you wrap the picker in something that overrides text colour, you are undoing the point of it.
- **Ids, not names.** Store what `on_select` gives you.
- **The placeholder label defaults to English.** Override it in a localized app.
- **`selected` must be `Some(id)` for the arrow keys to do anything** — with nothing selected there is no cursor to move from.
- **Cards are not cheap to draw.** Each is a small grid of coloured runs; a picker over a hundred user themes will cost more than a one-line [`SchemeSelect`](./scheme-select.md) would.
