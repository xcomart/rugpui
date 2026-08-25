# ruui

A gpui widget kit, and the two larger widgets built on it: a virtualised data
grid and a code editor. Extracted from three desktop applications that had been
carrying byte-identical copies of the same code, so that a fix made once is a
fix everywhere.

Nothing here knows what an application does. There are no database concepts, no
log concepts and no documents — only colours, text entry, tabs, menus, trees,
scroll indicators, a table over rows it never fetches, and an editor over a rope
it never loads. Every user-facing string comes from the host, which is what lets
a localized application stay localized without this repository having heard of a
locale.

## The crates

| crate | what it is |
|---|---|
| [`ruui`](crates/ruui) | The widget kit: theme and editor-theme palettes and their file store, text input, buttons, checkboxes, segmented controls, tabs, dropdown menus, selects, palette pickers, tooltips, modals, overlay scrollbars, lazily filled trees, and the caption buttons of a self-drawn title bar. |
| [`ruui-grid`](crates/ruui-grid) | A virtualised result grid. A million rows scroll without a stutter, null is not the empty string, and the rows arrive through a `GridSource` the host implements — so the grid can be pointed at a query result, a `DESCRIBE`, a plan or a diff. |
| [`ruui-editor`](crates/ruui-editor) | A code editor: a rope, a pluggable per-line highlighter with an incremental cache, and an element that shapes only the visible lines. Ships base lexers for eighteen languages — SQL, Java, XML/HTML, PHP, the seven configuration formats a file panel reaches every day, and a C-like table for the rest — with a registry that picks one from a file name, a `#!` line or a language the host defined in a YAML file (`custom-syntax`). Composes any second grammar the host supplies over one of them. |

`ruui-grid` and `ruui-editor` both depend on `ruui`; neither depends on the
other.

## Using it

Take all three from git at one revision, and — this is the part that is easy to
get wrong — point your own `[patch."https://github.com/zed-industries/zed"]`
table at *this* repository at that same revision:

```toml
[workspace.dependencies]
ruui = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
ruui-grid = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
ruui-editor = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }

# The gpui these widgets are written against, named by revision because a git
# dependency is identified by URL *and* revision.
gpui = { git = "https://github.com/zed-industries/zed", rev = "fd82517a115d97a07835b52f0512b22b38e38ccf" }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "fd82517a115d97a07835b52f0512b22b38e38ccf", features = [
    "font-kit",
    "wayland",
    "x11",
] }

[patch."https://github.com/zed-industries/zed"]
gpui = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
gpui_linux = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
gpui_macos = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
gpui_windows = { git = "https://github.com/xcomart/ruui", rev = "<sha>" }
```

The patch table is not optional. Without it your workspace resolves gpui from
Zed's monorepo while `ruui` resolves it from the patched copy, and two gpui
crates end up in one binary: the `Global`s the widgets install would be invisible
to the application, and nothing would draw. Keep the `rev` of the patch table and
of the `ruui` dependencies the same, for the same reason.

Then, once during start-up:

```rust
ruui::init(cx);          // key bindings, the two registries, the default palettes
ruui_grid::init(cx);     // if you use the grid
ruui_editor::init(cx);   // if you use the editor
```

and, if you let users drop theme files into a directory of your choosing:

```rust
let dirs = ruui::ThemeDirs {
    ui_themes: config_dir.join("themes"),
    // `None` for an application with no code editor and so no second palette.
    editor_themes: Some(config_dir.join("editor-themes")),
};
ruui::theme_store::reload(&dirs, cx);
```

Where those directories are is the application's decision; this repository never
guesses at a configuration directory.

## The vendored gpui

`vendor/` holds four crates of Zed's monorepo at revision
`fd82517a115d97a07835b52f0512b22b38e38ccf`, each the upstream directory with its
manifest flattened and every change to the code marked `RULOGMAN PATCH` — so
diffing a vendored crate against upstream at that revision shows the whole of
what we carry. They are patched back over the git source by the root manifest's
patch table, and they exist because gpui has no answer of its own for four
things: moving an open window between the platform's caption and the
application's own (`set_titlebar_transparent`), the X11 backend running its
close callbacks with the client `RefCell` borrowed, X11's `is_transparent`
ignoring client-side decorations, and X11 having no counterpart to Wayland's
`org_kde_kwin_blur`. The root
[`Cargo.toml`](Cargo.toml) tells the whole story, hunk by hunk.

`vendor/unicode-width` is there for a different reason: nothing in this
workspace uses it, and it is neither patched nor built here. It narrows the
ranges where Unicode 16 and the terminals disagree about a symbol's width, and
it lives here so an application that needs it can take it through
`[patch.crates-io]` at the same revision as everything else rather than carrying
a vendored tree of its own.

Building this workspace prints three `patch ... was not used in the crate graph`
warnings, for `gpui_linux`, `gpui_macos` and `gpui_windows`. That is expected: a
widget links the platform-independent core and never a backend, so only a
consuming application — which does name `gpui_platform` — pulls those three in.
The entries are kept because the table is what consumers copy.

Moving the revision forward means re-flattening the manifests and replaying the
marked hunks. Delete a hunk, and then the vendored crate once it holds none,
whenever upstream grows its own answer.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The first build compiles gpui from source and takes a while. The tests need no
window and no display server: they run on gpui's headless test platform.

## Licence

MIT. See [LICENSE](LICENSE).
