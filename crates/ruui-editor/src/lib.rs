//! The code editor: a rope, a pluggable highlighter, an incremental syntax
//! cache, and a gpui element that draws only what fits on screen.
//!
//! `ruui`'s [`TextInput`](ruui::TextInput) is a form field — a single line by
//! default and a few rows at most — so the editor is a new widget rather than
//! an extension of it. What carries over is the discipline, not the code: byte
//! offsets everywhere, UTF-16 only at the platform boundary, grapheme clusters
//! for every caret step, and an `EntityInputHandler` that the IME can drive
//! without ever being handed an offset that is not on a character boundary.
//! [`mod@editor`] documents each departure and why it is one.
//!
//! # The boundary
//!
//! This crate knows `ruui` and nothing else. It has no file system, no
//! language server and no parser: a highlighter is a line lexer the host hands
//! it, and the whole-document verdict a real parser would give — parse errors,
//! unknown fields — comes from the host as gutter marks.
//!
//! # The three things that make it hold at 100MB
//!
//! * **The buffer is a rope.** An insert is O(log n), and so are
//!   `byte <-> line` and `byte <-> UTF-16 code unit`. [`mod@buffer`].
//! * **The syntax cache is one [`LineState`] per line**, and an edit re-lexes
//!   from the edited line down to the first line whose end state is unchanged —
//!   which for an ordinary keystroke is the line itself. [`mod@highlight`].
//! * **Only the visible lines are shaped.** The element works out the row range
//!   from the scroll offset and the line height, and shapes those and no
//!   others. [`mod@element`].
//!
//! The things a whole-buffer `&str` would be needed for — "which statement is
//! the caret in", "which bracket matches this one" — are answered over a window
//! of the rope cut at statement boundaries, so they cost the length of a
//! statement rather than the length of the document. [`mod@syntax`].
//!
//! # Using it
//!
//! ```ignore
//! ruui_editor::init(cx);            // once, after ruui::init
//!
//! let editor = cx.new(|cx| {
//!     EditorView::new(cx).highlighter(Arc::new(SqlHighlighter))
//! });
//! cx.subscribe(&editor, |_, editor, event: &EditorEvent, cx| {
//!     if let EditorEvent::Changed = event {
//!         let text = editor.read(cx).text();
//!         // re-render the preview
//!     }
//! })
//! .detach();
//! ```
//!
//! An editor with no highlighter is a plain-text editor, and that is what
//! [`EditorView::new`] makes. [`mod@lang`] is a base [`Highlighter`] per
//! extension, looked up by [`lang::highlighter_for_extension`].
//! [`CompositeHighlighter`] is how a document that is *two* languages at once —
//! a template written in Java, a query with a host language's placeholders in
//! it — gets both: the base language underneath, and an [`Overlay`] the host
//! supplies painted over it. No overlay ships here, because an overlay is a
//! grammar and whose grammar it is, is the host's question.
//!
//! # Out of scope, deliberately
//!
//! Multiple cursors would change the shape of every command in [`mod@editor`],
//! so they go in as a list of selections in one piece or not at all. Code
//! folding needs a row-to-line map between the buffer and the renderer, which
//! nothing else wants yet. A minimap needs a second, coarser shaping pass, and
//! is the least valuable of the three.
//!
//! The completion popup is the host's, not this crate's: what to offer comes
//! from a model this crate has never heard of. What is here is what the popup
//! needs from the document — [`EditorView::word_before_caret`],
//! [`EditorView::line_before_caret`], [`EditorView::caret_bounds`],
//! [`EditorView::replace_range`] — so that no caller ever has to work out a
//! byte offset into the rope for itself.

#![warn(missing_docs)]

pub mod buffer;
pub mod composite;
pub mod editor;
pub mod element;
pub mod find;
pub mod highlight;
pub mod history;
pub mod lang;
pub mod sql_syntax;
pub mod syntax;

pub use buffer::Buffer;
pub use composite::{CompositeHighlighter, Overlaid, Overlay};
pub use editor::{EditorEvent, EditorView, MarkKind, NavKey, init};
pub use element::EditorElement;
pub use find::{FindState, find_all};
pub use highlight::{Highlighter, LineState, Span, SyntaxCache, Token};
pub use history::{Edit, EditKind, History, SelectionState, Transaction};
pub use lang::highlighter_for_extension;
pub use sql_syntax::SqlHighlighter;
pub use syntax::StatementSpan;

#[cfg(test)]
mod tests;
