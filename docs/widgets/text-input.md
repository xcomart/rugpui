# TextInput

A focusable text field: one line by default, several rows on request. Reach for it for any value the user types — a host name, a search box, a short SQL fragment. It is not a code editor: there is no undo, no highlighting and no gutter, and `rugpui-editor` is what a host wants when it needs a real one.

Source: [text_input.rs](../../crates/rugpui/src/text_input.rs). Re-exported from the crate root as `rugpui::{TextInput, InputMenuLabels}`.

## An entity, not an element

Every other widget in this crate is a value you build inside `render` and throw away. `TextInput` is not: it holds the caret, the selection, the IME's marked range and the scroll offset, so it is a gpui `Entity` that the host creates once and keeps.

```rust
use gpui::{Context, Entity, prelude::*};
use rugpui::TextInput;

struct Form {
    host: Entity<TextInput>,
}

impl Form {
    fn new(cx: &mut Context<Self>) -> Self {
        let host = cx.new(|cx| {
            let mut input = TextInput::new(cx).placeholder("host:port");
            input.set_content("db.internal:5432", cx);
            input
        });
        Self { host }
    }
}
```

Rendering it is just passing the entity as a child, exactly as the gallery does:

```rust
div().child(self.host.clone())
```

The builder methods (`placeholder`, `masked`, `multiline`, `disabled`, `tab_index`, `on_submit`, `context_menu`) take `self` by value, so they run inside the `cx.new` closure. Anything that has to change later has a setter — `set_content`, `set_placeholder`, `set_invalid`, `set_context_menu`, `clear` — because once the closure has returned there is no builder left to reach.

Before the first window opens, call [`rugpui::init`](../getting-started.md) (which calls `TextInput::init`) so the field's key bindings exist.

## Builder options and methods

| method | argument | default | effect |
| --- | --- | --- | --- |
| `TextInput::new` | `&mut Context<Self>` | — | empty single-line field, not focused |
| `TextInput::init` | `&mut App` | — | registers the key bindings; called for you by `rugpui::init` |
| `.placeholder` | `impl Into<SharedString>` | empty | text drawn in `text_muted` while the content is empty |
| `.masked` | `bool` | `false` | renders every grapheme as `•`; copy and cut are refused while masked |
| `.multiline` | `usize` rows | single line | field is `rows` rows tall, `Enter` breaks the line instead of submitting |
| `.disabled` | `bool` | `false` | read-only, muted text, arrow cursor, no actions and no menu wired at all |
| `.tab_index` | `isize` | not a tab stop | joins the window's tab ring at that index |
| `.on_submit` | `Fn(&str, &mut Window, &mut App)` | none | invoked on `Enter` in a single-line field, with the current content |
| `.context_menu` | `Fn(&App) -> InputMenuLabels` | none | gives the field a right-click cut/copy/paste/select-all menu |
| `.content()` | — | — | the current value as `&str` |
| `.current_placeholder()` | — | — | the placeholder currently set |
| `.is_multiline()` | — | — | whether `Enter` breaks the line |
| `.set_content` | `impl Into<SharedString>`, `cx` | — | replaces the value, collapsing the caret to its end |
| `.clear` | `cx` | — | `set_content("")` |
| `.set_placeholder` | `impl Into<SharedString>`, `cx` | — | for a field that must follow a language switch |
| `.set_invalid` | `bool`, `cx` | `false` | outlines the field in `danger`; a no-op when the flag is unchanged |
| `.set_context_menu` | labels closure, `cx` | — | the setter form of `.context_menu` |

`multiline(rows)` clamps to at least one row and sizes the field to what it *shows*: longer text scrolls inside the frame instead of pushing the form apart. A masked multiline field is a contradiction and the mask is ignored.

## Reading the value, and reacting to it

`TextInput` emits no events. It calls `cx.notify()` whenever the content, placeholder, invalid flag or caret changes, so the host observes it:

```rust
let host = self.host.clone();
cx.observe(&host, |form, input, cx| {
    let text = input.read(cx).content().to_owned();
    form.validate(&text, cx);
})
.detach();
```

A one-shot read — the moment a dialog is confirmed — needs no subscription at all:

```rust
let value = self.host.read(cx).content().to_owned();
```

Validation is the host's job, which is why `set_invalid` is a setter rather than a builder: whoever knows what a legal value looks like keeps the flag in step with the content. The danger outline deliberately wins over the focus ring, since the field being typed into is exactly the one whose refusal must stay visible.

`on_submit` is the other direction. In a single-line field `Enter` runs it with the content; in a multiline field `Enter` inserts a newline and the callback is never reached, so a form with a multiline field needs a real confirm button.

## Keyboard, mouse and IME

`TextInput::init` binds actions in the `rugpui_input` namespace, all scoped to the `TextInput` key context so they never leak into the rest of the app. The clipboard and select-all chords follow the platform: `cmd` on macOS, `ctrl` everywhere else.

| keys | action | effect |
| --- | --- | --- |
| `backspace` / `delete` | `Backspace` / `Delete` | delete the grapheme either side of the caret, or the selection |
| `left` / `right` | `Left` / `Right` | move one grapheme; collapse a selection to that end |
| `shift-left` / `shift-right` | `SelectLeft` / `SelectRight` | extend the selection one grapheme |
| `up` / `down` | `Up` / `Down` | move a row (multiline; single-line rows are degenerate) |
| `shift-up` / `shift-down` | `SelectUp` / `SelectDown` | extend the selection a row |
| `home` / `end` | `Home` / `End` | caret to the start / end of the field |
| `shift-home` / `shift-end` | `SelectHome` / `SelectEnd` | extend the selection to the start / end |
| `enter` | `Submit` | run `on_submit`, or break the line in a multiline field |
| `cmd/ctrl-a` | `SelectAll` | select everything |
| `cmd/ctrl-c` | `Copy` | copy the selection; refused while masked |
| `cmd/ctrl-x` | `Cut` | copy and delete; refused while masked |
| `cmd/ctrl-v` | `Paste` | insert the clipboard at the caret |
| `ctrl-cmd-space` (macOS only) | `ShowCharacterPalette` | opens the system emoji / character palette |

Mouse: left press places the caret and starts a drag selection, moving extends it, release ends it (including a release outside the field). Right press focuses the field and opens the edit menu, if it has one. A multiline field scrolls with the wheel, and the caret is kept in view by the element asking gpui to scroll to it after the frame that moved it.

IME works because `TextInput` implements `EntityInputHandler` and installs an `ElementInputHandler` over its painted bounds. Composition text is held in a marked range and drawn with a one-pixel underline until it is committed, so Hangul, kana and pinyin all compose in place. Every offset the field stores is a *byte* offset into the real content; when the field is masked a `DisplayMap` translates those to and from the bullet string, which is what keeps the caret correct for multi-byte text.

## The edit menu

A right-click opens cut / copy / paste / select-all — but only for a field the host has worded, because those four strings are user-facing sentences and this crate holds none of its own.

```rust
use rugpui::{InputMenuLabels, TextInput};

let input = cx.new(|cx| {
    TextInput::new(cx).context_menu(|_cx| InputMenuLabels {
        cut: "Cut".into(),
        copy: "Copy".into(),
        paste: "Paste".into(),
        select_all: "Select all".into(),
    })
});
```

The closure is asked for its wording *every time the menu opens*, not once when the field is built, so an application that switches language while a window is open shows the new words on the next click without rebuilding a single input. In a real host that closure reads the current locale out of the `App`.

The menu is built out of [`MenuEntry`](./menu.md) rows and shown through a `ContextMenu` positioned at the click. Each row runs the same handler its key binding runs, so the menu is a second way in rather than a second implementation. Rows are dropped when they cannot apply: cut and copy are absent with no selection or on a masked field, select-all is absent on empty content, and a separator only appears between two non-empty groups. Shortcut hints name `Cmd` or `Ctrl` from the same `cfg` the bindings were made with, so a hint can never advertise a chord the field does not answer to. A field that has outlived its focus drops its open menu on the next frame.

## Theme slots

| slot | where |
| --- | --- |
| `surface` | field background; `surface.opacity(0.6)` while disabled |
| `border` | resting outline |
| `accent` | focus outline, the two-pixel caret, and `accent.opacity(0.3)` for the selection fill |
| `danger` | outline while `set_invalid(true)` |
| `text_muted` | placeholder text, and the content of a disabled field |
| `text` | inherited from the surrounding text style for normal content |

The frame is 32 px tall for a single line (a 20 px line box with 6 px either side); a multiline field keeps the padding and grows by whole rows, so one two-row field and two one-row fields stack to the same height.

## Pitfalls

- **Builders run once.** `input.placeholder("…")` on an existing entity will not compile the way you want — use `set_placeholder`. The same goes for the content: only `set_content` and `clear` exist after construction.
- **A disabled field is inert, not merely grey.** None of the actions, mouse handlers or the right-click menu are wired at all while `disabled(true)`, so there is no row a read-only field could honour.
- **No menu without labels.** A field built without `.context_menu(...)` has no right-click menu; this is the default, not an oversight.
- **`Enter` means two different things.** Check `is_multiline()` before assuming `on_submit` will ever fire.
- **Masked fields still hold the real string.** Only the rendering is masked; `content()` returns the plaintext, and copy/cut are refused so it cannot leak to the clipboard.
- **Paste is asynchronous.** The clipboard is read on a spawned task, so the content changes a moment after the keystroke, and a field torn down in between simply drops the paste.
