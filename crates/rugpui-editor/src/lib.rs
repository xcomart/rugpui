//! The code editor: a rope, a pluggable highlighter, an incremental syntax
//! cache, and a gpui element that draws only what fits on screen.
//!
//! `rugpui`'s [`TextInput`](rugpui::TextInput) is a form field — a single line by
//! default and a few rows at most — so the editor is a new widget rather than
//! an extension of it. What carries over is the discipline, not the code: byte
//! offsets everywhere, UTF-16 only at the platform boundary, grapheme clusters
//! for every caret step, and an `EntityInputHandler` that the IME can drive
//! without ever being handed an offset that is not on a character boundary.
//! [`mod@editor`] documents each departure and why it is one.
//!
//! # The boundary
//!
//! This crate knows `rugpui` and nothing else. It has no file system, no
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
//! Word wrap adds a fourth: **where each line breaks is measured once per
//! line**, not once per frame, so a line the viewport has never reached costs a
//! shaping pass when wrapping is switched on and nothing afterwards until it is
//! edited. [`mod@wrap`] holds the breaks and the row arithmetic built on them;
//! with wrapping off — the default — every function in it is the identity and a
//! row is a line.
//!
//! The things a whole-buffer `&str` would be needed for — "which statement is
//! the caret in", "which bracket matches this one" — are answered over a window
//! of the rope cut at statement boundaries, so they cost the length of a
//! statement rather than the length of the document. [`mod@syntax`].
//!
//! # Using it
//!
//! ```ignore
//! rugpui_editor::init(cx);            // once, after rugpui::init
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
//! language — the seven configuration formats a file panel reaches every day,
//! the three that need a lexer of their own, and a C-like table for the rest —
//! looked up by [`lang::highlighter_for_extension`] when a host has an
//! extension and by [`LanguageRegistry`] when it has a file name, a first line
//! and a picker to fill. A host that writes its languages in a file rather than
//! in code turns on the `custom-syntax` feature and registers a
//! `lang::custom::Definition`; it is off by default, because it costs a YAML
//! reader.
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
//! folding wanted a row-to-line map between the buffer and the renderer, and
//! word wrap has since built one ([`mod@wrap`]); what is still missing is the
//! fold model itself, which nothing here wants yet. A minimap needs a second,
//! coarser shaping pass, and is the least valuable of the three.
//!
//! The completion popup is the host's, not this crate's: what to offer comes
//! from a model this crate has never heard of. What is here is what the popup
//! needs from the document — [`EditorView::word_before_caret`],
//! [`EditorView::line_before_caret`], [`EditorView::caret_bounds`],
//! [`EditorView::replace_range`] — so that no caller ever has to work out a
//! byte offset into the rope for itself, plus the two functions that colour a
//! line the way the editor colours it: [`runs_for_spans`] and [`color_for`].
//!
//! # Code that is read rather than typed into
//!
//! [`mod@snippet`] is the editor's other half: [`CodeSnippet`] is a stateless
//! element that lexes a few lines and draws them, with no caret, no gutter and
//! no history — for a documentation box, a preview, a saved query in a list.
//! [`tooltip_code`] puts one in a [`rugpui`] tooltip, and
//! [`rugpui::Tooltip::element`] puts one beside a caption and a thumbnail.

#![warn(missing_docs)]

pub mod buffer;
pub mod composite;
pub mod editor;
pub mod element;
pub mod find;
pub mod highlight;
pub mod history;
pub mod lang;
pub mod snippet;
pub mod sql_syntax;
pub mod syntax;
pub mod wrap;

pub use buffer::Buffer;
pub use composite::{CompositeHighlighter, Overlaid, Overlay};
pub use editor::{EditorEvent, EditorView, MarkKind, NavKey, init};
pub use element::EditorElement;
pub use find::{FindState, find_all};
pub use highlight::{Highlighter, LineState, Span, SyntaxCache, Token, color_for, runs_for_spans};
pub use history::{Edit, EditKind, History, SelectionState, Transaction};
pub use lang::{FileMatch, LanguageEntry, LanguageRegistry, highlighter_for_extension};
pub use snippet::{CodeSnippet, tooltip_code};
pub use sql_syntax::SqlHighlighter;
pub use syntax::StatementSpan;

#[cfg(test)]
mod tests;
