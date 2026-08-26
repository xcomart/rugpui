# SchemeSelect

A dropdown that picks one colour scheme out of a list: the same trigger, deferred list, backdrop and arrow keys as [`Select`](./select.md), over entries that carry colours as well as a name. Reach for it in a settings dialog that has to offer a palette on one line.

Source: [scheme_select.rs](../../crates/rugpui/src/scheme_select.rs). Re-exported as `rugpui::{SchemeSelect, SchemeSwatch, SchemePreview}`.

A grid of preview cards shows more of each scheme at once, but it costs a form several rows of height per catalogue — and a dialog with two catalogues and a dozen other settings is better served by one line each, with the colours carried along on the right of every row. When a syntax palette is what is being chosen, use [`EditorThemePicker`](./editor-theme-picker.md) instead: syntax colours only mean anything in arrangement, and a pill of swatches cannot answer "can I still tell a keyword from a column name".

The widget knows nothing about what it is previewing. Callers hand it plain `Hsla` values, so the same dropdown fits anything with a background, a foreground and a handful of accents.

## Feeding it from `ThemeRegistry`

Entries are identified by **id**, not by label — a scheme's name is what the user reads, its id is what `settings.json` stores, and the two are not the same string. `ThemeRegistry::all(cx)` returns exactly the `ThemeEntry { id, name, dark, builtin }` rows to build from, and `ThemeRegistry::resolve(id, cx)` turns an id into the `Theme` whose colours go into the pill:

```rust
use rugpui::{SchemePreview, SchemeSelect, SchemeSwatch, Theme, ThemeRegistry};

fn scheme_swatches(cx: &App) -> Vec<SchemeSwatch> {
    ThemeRegistry::all(cx)
        .into_iter()
        .map(|entry| {
            let theme: Theme = ThemeRegistry::resolve(&entry.id, cx);
            SchemeSwatch::new(entry.id, entry.name).preview(SchemePreview {
                background: theme.background,
                foreground: theme.text,
                accents: vec![theme.accent, theme.success, theme.danger],
            })
        })
        .collect()
}
```

And the control itself, with the state the host keeps:

```rust
let this = cx.entity();

SchemeSelect::new("scheme")
    .options(scheme_swatches(cx))
    .selected(Some(self.scheme.clone()))
    .open(self.scheme_open)
    .scroll_handle(self.scheme_scroll.clone())
    // `on_select` takes `&str`, so `cx.listener` fits it directly.
    .on_select(cx.listener(|view, id: &str, _window, cx| {
        view.scheme = SharedString::from(id.to_owned());
        set_theme(ThemeRegistry::resolve(id, cx), cx);
        cx.notify();
    }))
    // `on_open_change` takes `bool` by value, which `cx.listener` cannot
    // produce — it always hands the event over by reference.
    .on_open_change(move |open, _window, cx| {
        this.update(cx, |view, cx| {
            view.scheme_open = open;
            cx.notify();
        });
    })
```

`ThemeRegistry::all` returns the six built-in themes in presentation order followed by the user's own, sorted by name, with any custom theme that shadows a built-in id left out. See [theming](../theming.md) for where those files come from.

## SchemeSwatch and SchemePreview

| method | argument | effect |
| --- | --- | --- |
| `SchemeSwatch::new` | id, name | an entry with **no** preview, drawn as a muted placeholder pill |
| `.preview` | `SchemePreview` | the colours the pill is painted with |
| `.placeholder_label` | `impl Into<SharedString>` | text on the placeholder pill (default `"inherits"`, English) |

`SchemePreview` is a plain struct with three public fields:

| field | type | drawn as |
| --- | --- | --- |
| `background` | `Hsla` | the pill's fill |
| `foreground` | `Hsla` | the colour of the sample `Aa` |
| `accents` | `Vec<Hsla>` | 8 px round chips beside the sample, in the order given |

An entry with no preview gets an *outlined* pill carrying its placeholder label, so that "inherit the other choice" reads as an absence of colour rather than as a scheme that happens to be transparent. That is how a picker offers "follow the app theme": one `SchemeSwatch::new(id, name)` with no `.preview(…)`, and a translated `.placeholder_label(…)`.

## Builder options

| method | argument | default | effect |
| --- | --- | --- | --- |
| `SchemeSelect::new` | `impl Into<ElementId>` | — | empty, closed, nothing selected |
| `.options` | `impl IntoIterator<Item = SchemeSwatch>` | empty | the entries, in display order |
| `.selected` | `Option<impl Into<SharedString>>` | `None` | the id of the picked entry |
| `.open` | `bool` | `false` | whether the list is showing |
| `.disabled` | `bool` | `false` | read-only line: muted, unfocusable, cannot open |
| `.width` | `Pixels` | trigger fills parent, list 320 px | width of trigger and list |
| `.tab_index` | `isize` | not a tab stop | joins the tab ring; ignored while disabled |
| `.scroll_handle` | `ScrollHandle` | none | so the parent can reveal the current entry |
| `.scrollbar` | `Scrollbar` | none | overlay indicator down the open list |
| `.on_select` | `Fn(&str, …)` | none | **the id** of the newly picked entry |
| `.on_open_change` | `Fn(bool, …)` | none | the open state the control would like |

`on_select` is never fired for the entry that is already selected — clicking it only puts the list away — which spares the parent an update that changes nothing.

`.selected` with an id no entry answers to still shows on the trigger, spelled as the id itself since there is no name to show, and highlights no row. A hand-edited `settings.json` naming a scheme that has since been deleted should say so rather than look like nothing was ever chosen.

`.disabled(true)` is for the case where something else has already made the choice — the editor theme while it follows the chrome theme — and the answer is still worth showing because it moves as the other choice does. Everything that answers a pointer or the keyboard hangs off one branch, so a disabled control is inert rather than merely grey, and it stays shut whatever the open flag says.

## Keyboard and mouse

Identical to `Select`. One tab stop; `Enter`/`Space` toggle the list; `Up`/`Down` while open move without wrapping, scroll the row into view through the handle, and fire `on_select` immediately. Modified keystrokes are ignored. `Escape` is the parent's to handle. A click outside closes through a full-window backdrop.

## Theme slots

The chrome the control is drawn in comes from the *current* `Theme`; the pill colours come from the `SchemePreview` you supplied.

| slot | where |
| --- | --- |
| `surface` | trigger background |
| `surface_hover` | trigger hover, hovered row |
| `surface_active` | background of the current row |
| `border` | trigger and list outline, and the placeholder pill's outline |
| `accent` | trigger border while focused, and the text of the current row |
| `background` | the list panel's fill |
| `text` | trigger label, normal rows |
| `text_muted` | disabled trigger label, placeholder pill text, the `▾` chevron |

The trigger is 32 px tall (matching `Select` and `TextInput`); rows are 30 px, taller than a plain dropdown's because they carry a pill.

## Pitfalls

- **Ids, not names.** `on_select` gives you the id. Store the id, resolve it to a palette, and keep the name only for display.
- **Nothing opens itself**, and a disabled control never opens at all, even if your flag says it should.
- **Set a width** unless the trigger genuinely fills its parent — the anchored list cannot measure it.
- **The placeholder label defaults to English.** Every localized caller should override it.
- **Rebuild the swatches when the registry changes.** `ThemeRegistry::all` is read at render time; after `theme_store::reload` writes new custom themes, the next render picks them up, but a vector you cached will not.
