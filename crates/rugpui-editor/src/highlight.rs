//! What a highlighter is, and the per-line state cache that makes one
//! incremental.
//!
//! The editor holds no lexer of its own. It holds a [`Highlighter`] — a trait
//! with one method, "give me the coloured runs of this line, given the state
//! the line before it ended in" — and a table of implementations ships with the
//! crate: [`SqlHighlighter`](crate::sql_syntax::SqlHighlighter) and the base
//! languages of [`lang`](crate::lang). An editor with no highlighter at all is
//! a plain-text editor, and that is the default.
//!
//! # Why the state is an opaque integer
//!
//! The obvious signature carries the state as an associated type, so that a
//! highlighter can name its own. That makes the trait not object safe, and the
//! editor has to hold `Arc<dyn Highlighter>`: the highlighter is chosen when a
//! tab is opened, not when the widget is compiled. So the state is
//! [`LineState`], one opaque `u32` that only the highlighter that produced it
//! knows how to read. Every state a line lexer needs — "inside a block
//! comment", "inside a statement, past its colon, inside a single-quoted
//! value" — is a small number, and a `u32` holds all of them with room for a
//! bitfield.
//!
//! [`LineState::START`] is the state the first line of a buffer starts in, and
//! the state every highlighter has to treat as "nothing is open".
//!
//! # What is cached, and what is not
//!
//! Not the spans. Holding a `Vec<Span>` for every line of a large document
//! would cost more than the document does, and it would buy nothing:
//! [`Highlighter::line`] over one line is a few hundred nanoseconds, and the
//! renderer only ever needs the spans of the forty lines it is about to draw.
//! So the cache holds the four bytes per line that *cannot* be recomputed
//! locally — the state each line ends in — and the spans are produced on demand
//! from the state of the line before.
//!
//! After an edit on line *n*, [`SyntaxCache::edited`] re-lexes from *n*
//! downwards and stops at the first line whose new end state equals the one it
//! had. For an edit that opens no comment and no statement — which is nearly
//! every edit — that is one line, whatever the document's length. Typing `/*`
//! on line three of a hundred thousand walks down until the states stop
//! changing, which is either the line that closes the comment or the end of the
//! file; typing the `*/` that closes it walks back down again.
//!
//! [`SyntaxCache::lex_calls`] counts the calls, which is how the tests hold the
//! two claims above down: that drawing costs one call per visible line, and
//! that an ordinary edit costs one call.

use std::cell::Cell;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use gpui::{Font, Hsla, TextRun};
use rugpui::EditorTheme;

use crate::buffer::Buffer;

/// One of the token colours an editor palette hands out.
///
/// Every variant but [`Token::QuotedIdentifier`] names one of the fourteen
/// token slots of [`EditorTheme`], so that mapping a span
/// onto a colour is a total function with no fallback and no invented slot;
/// `QuotedIdentifier` shares [`Token::Identifier`]'s slot rather than adding a
/// fifteenth. Text a highlighter classifies as nothing at all gets no span,
/// and the renderer draws it in the palette's foreground colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    /// Reserved words: `select`, `from`, and a template's statement names.
    Keyword,
    /// Quoted literals.
    String,
    /// Numeric literals.
    Number,
    /// Line and block comments.
    Comment,
    /// Called functions, and a template's value processors.
    Function,
    /// Type names, and a template's option names.
    Type,
    /// `=`, `<>`, `,`, `:` — anything that combines two things.
    Operator,
    /// Table, column, alias and key names.
    Identifier,
    /// A quoted identifier — `"…"`, `` `…` `` or `[…]` in SQL. Painted like an
    /// identifier, but opaque to the statement splitter and the bracket
    /// matcher the way a string is: a `;` or a bracket inside it is part of
    /// the name.
    QuotedIdentifier,
    /// The left-hand side of a mapping, and a section header.
    ///
    /// What a configuration format spends half its screen on: the `port` of
    /// `port: 22`, the `[server]` of an `ini` file, the `PATH` of `PATH=/bin`,
    /// the `"host"` of a JSON member, a Markdown heading. The SQL and C-like
    /// lexers never emit it — there is no mapping in a `SELECT` — and the
    /// configuration lexers of [`lang`](crate::lang) emit little else.
    Key,
    /// A named reference to something defined elsewhere.
    ///
    /// A shell expansion (`$HOME`, `${TARGET:-x}`), a YAML anchor or alias
    /// (`&defaults`, `*defaults`), a Markdown link's text. Not a *declaration*
    /// of a name, which is [`Token::Key`]: the two are told apart by which side
    /// of the binding they are on, not by their spelling.
    Variable,
    /// Brackets, semicolons, dots and a template's `${`/`}`.
    Punctuation,
    /// The bracket under the caret and its partner.
    ///
    /// No highlighter ever emits this one: the bracket pair is found by
    /// [`crate::syntax::bracket_pair`] over the caret, not by a line lexer, and
    /// the element paints it as a quad under the text rather than as a colour
    /// on it. The variant exists so that the enum is exactly the palette's
    /// fourteen token slots.
    BracketMatch,
    /// Text that cannot be read at all: an unbalanced `}`, an unknown statement.
    Error,
    /// Text that reads, but not the way it was probably meant to: a misspelled
    /// processor, an `${ENDIF}` that the engine will look up as an item.
    Warning,
}

/// A run of one line in one colour, in bytes from the start of that line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The run, relative to the start of the line it was lexed from.
    pub range: Range<usize>,
    /// What to colour it.
    pub token: Token,
}

impl Span {
    /// A span over `range`.
    pub const fn new(range: Range<usize>, token: Token) -> Self {
        Self { range, token }
    }

    /// How many bytes the span covers.
    pub const fn len(&self) -> usize {
        self.range.end.saturating_sub(self.range.start)
    }

    /// Whether the span covers nothing.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A run of `len` bytes in one colour, in `font`.
///
/// The plain case: no background, no underline, no strikethrough. The editor
/// element adds an underline to a copy of one of these when the IME is
/// composing over it.
pub(crate) fn plain_run(len: usize, color: Hsla, font: &Font) -> TextRun {
    TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// The coloured runs of one line, from the spans a [`Highlighter`] gave for it.
///
/// The runs have to tile `text`, because gpui shapes a line from a list of runs
/// whose lengths add up to its length -- but a highlighter's spans need not,
/// and a template's spans deliberately do not (the text between two statements
/// is nobody's token). So the gaps are filled here, in
/// [`EditorTheme::foreground`], and that is most of what this function does.
///
/// Spans are clamped rather than trusted: a highlighter is ordinary code, and
/// one span past the end of a line would otherwise be a panic inside gpui's
/// shaper rather than a wrong colour. A span that starts before the previous
/// one ended is pulled forward to where that one ended, so the result is
/// always sorted and non-overlapping whatever came in.
///
/// Public because the editor is not the only thing that draws code with this
/// crate's colours. A host's completion popup, a diff view, a log pane -- and
/// [`CodeSnippet`](crate::CodeSnippet), which is this crate's own use of it --
/// all want the same answer, and the alternative is every one of them getting
/// the gap-filling subtly wrong.
///
/// ```ignore
/// let (spans, next) = highlighter.line(line, state);
/// let runs = runs_for_spans(line, &spans, &palette, &font);
/// StyledText::new(line.to_string()).with_runs(runs)
/// ```
pub fn runs_for_spans(
    text: &str,
    spans: &[Span],
    palette: &EditorTheme,
    font: &Font,
) -> Vec<TextRun> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0;
    for span in spans {
        let from = span.range.start.clamp(at, text.len());
        let to = span.range.end.clamp(from, text.len());
        if from > at {
            runs.push(plain_run(from - at, palette.foreground, font));
        }
        if to > from {
            runs.push(plain_run(to - from, color_for(span.token, palette), font));
        }
        at = to;
    }
    if at < text.len() {
        runs.push(plain_run(text.len() - at, palette.foreground, font));
    }
    runs
}

/// The palette slot a token draws in.
///
/// One arm per variant and no fallback: every [`Token`] maps to one of the
/// palette's fourteen slots, which is what keeps a highlighter from inventing a
/// class no theme has a colour for. [`Token::QuotedIdentifier`] shares
/// [`Token::Identifier`]'s slot rather than needing one of its own.
///
/// Public for the same reason [`runs_for_spans`] is: a host drawing its own
/// popup over this crate's spans should ask the same question and get the same
/// answer.
pub const fn color_for(token: Token, palette: &EditorTheme) -> Hsla {
    match token {
        Token::Keyword => palette.keyword,
        Token::Type => palette.r#type,
        Token::Function => palette.function,
        Token::String => palette.string,
        Token::Number => palette.number,
        Token::Comment => palette.comment,
        Token::Operator => palette.operator,
        Token::Punctuation => palette.punctuation,
        Token::Identifier | Token::QuotedIdentifier => palette.identifier,
        Token::Key => palette.key,
        Token::Variable => palette.variable,
        Token::BracketMatch => palette.bracket_match,
        Token::Error => palette.error,
        Token::Warning => palette.warning,
    }
}

/// The state a line lexer is in between two lines, as the editor sees it.
///
/// Opaque on purpose: the editor compares two of them and stores them, and
/// only the [`Highlighter`] that produced one knows what it means.
/// [`LineState::START`] is the one value every highlighter agrees on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LineState(pub u32);

impl LineState {
    /// Nothing is open: the state the first line of a document starts in.
    pub const START: Self = Self(0);

    /// How many bits of a state survive being composed with another.
    ///
    /// [`CompositeHighlighter`](crate::composite::CompositeHighlighter) runs
    /// two line lexers over the same document and has one `u32` to keep both
    /// their states in, so each gets half of it. A highlighter that means to be
    /// composable — used as the base language under an overlay, or as the
    /// overlay over a base — has to keep its state inside this many bits. The
    /// widest lexer shipped here uses three.
    pub const COMPOSABLE_BITS: u32 = 16;

    /// Whether this is [`LineState::START`].
    pub const fn is_start(self) -> bool {
        self.0 == 0
    }

    /// Puts two composable states into one.
    ///
    /// `low` keeps the low [`LineState::COMPOSABLE_BITS`] and `high` the rest,
    /// so that two start states pack to [`LineState::START`] and a state that
    /// overflows its half is truncated rather than corrupting the other.
    pub const fn pack(low: Self, high: Self) -> Self {
        debug_assert!(
            low.0 >> Self::COMPOSABLE_BITS == 0 && high.0 >> Self::COMPOSABLE_BITS == 0,
            "a composed highlighter state has to fit in LineState::COMPOSABLE_BITS"
        );
        let mask = (1 << Self::COMPOSABLE_BITS) - 1;
        Self((low.0 & mask) | ((high.0 & mask) << Self::COMPOSABLE_BITS))
    }

    /// Takes a [`LineState::pack`]ed state apart again.
    pub const fn unpack(self) -> (Self, Self) {
        let mask = (1 << Self::COMPOSABLE_BITS) - 1;
        (Self(self.0 & mask), Self(self.0 >> Self::COMPOSABLE_BITS))
    }
}

/// A line lexer the editor can be given.
///
/// One method, and it has to be a pure function of its two arguments: the cache
/// calls it out of order, skips lines whose state did not change, and calls it
/// again for the same line on the next frame. A highlighter that remembers
/// anything between calls will disagree with the cache.
///
/// The spans it answers with must be sorted, must not overlap, and must lie
/// inside `text`. They need not tile it: the bytes no span covers are the ones
/// the highlighter had no opinion about, and the renderer draws them in the
/// palette's foreground colour. That is what keeps a highlighter for a language
/// that is mostly prose — a template — from having to invent a token for prose.
pub trait Highlighter: Send + Sync + 'static {
    /// The coloured runs of `text`, and the state the line after it starts in.
    ///
    /// `text` is one line with its terminator already stripped. `state` is what
    /// this method returned for the line before, or [`LineState::START`] for
    /// the first line of the document.
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState);

    /// What this language writes a line comment with, if it has one.
    ///
    /// `None` — the default — makes the editor's comment toggle do nothing,
    /// which is the right answer for a language that has no comments at all.
    fn line_comment(&self) -> Option<&'static str> {
        None
    }

    /// Whether this language is written as `;`-terminated statements, such
    /// that the editor should highlight the statement the caret sits in and
    /// let it be run on its own.
    ///
    /// `false` — the default — turns that behaviour off. Statement highlight
    /// and statement execution belong to the SQL editor; a language that
    /// happens to use `;` for something else (a Java template, say) would
    /// only get a misleading selection-like band drawn across unrelated
    /// lines.
    fn statements(&self) -> bool {
        false
    }
}

/// Per-line syntax state, kept in step with a [`Buffer`].
pub struct SyntaxCache {
    /// The lexer in force, if any. `None` is a plain-text document.
    highlighter: Option<Arc<dyn Highlighter>>,
    /// `ends[i]` is the state line `i` ends in. Always as long as the buffer
    /// has lines.
    ends: Vec<LineState>,
    /// How many times [`Highlighter::line`] has been called through this cache.
    ///
    /// A [`Cell`] so that [`SyntaxCache::spans`] can stay `&self` and be called
    /// from an element's prepaint, which holds the view by shared reference.
    lex_calls: Cell<usize>,
}

impl fmt::Debug for SyntaxCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyntaxCache")
            .field("highlighter", &self.highlighter.is_some())
            .field("lines", &self.ends.len())
            .field("lex_calls", &self.lex_calls.get())
            .finish()
    }
}

impl SyntaxCache {
    /// Builds the cache for `buffer`, lexing all of it once.
    ///
    /// The one linear pass in this module. It happens when a document is opened
    /// or [`SyntaxCache::reset`] is called, and never on an edit. With no
    /// highlighter it is a `resize`.
    pub fn new(buffer: &Buffer, highlighter: Option<Arc<dyn Highlighter>>) -> Self {
        let mut this = Self {
            highlighter,
            ends: Vec::new(),
            lex_calls: Cell::new(0),
        };
        this.reset(buffer);
        this
    }

    /// The lexer in force, if there is one.
    pub fn highlighter(&self) -> Option<&Arc<dyn Highlighter>> {
        self.highlighter.as_ref()
    }

    /// Swaps the lexer and re-lexes the buffer.
    ///
    /// A new highlighter moves what every state means, so there is no
    /// incremental path here and no need for one: it happens when a tab is
    /// opened, not while anyone is typing.
    pub fn set_highlighter(&mut self, highlighter: Option<Arc<dyn Highlighter>>, buffer: &Buffer) {
        self.highlighter = highlighter;
        self.reset(buffer);
    }

    /// What the language in force writes a line comment with, if it has one.
    pub fn line_comment(&self) -> Option<&'static str> {
        self.highlighter.as_ref()?.line_comment()
    }

    /// Discards the cache and rebuilds it from `buffer`.
    pub fn reset(&mut self, buffer: &Buffer) {
        let lines = buffer.line_count();
        self.ends.clear();
        self.ends.reserve(lines);
        if self.highlighter.is_none() {
            self.ends.resize(lines, LineState::START);
            return;
        }
        let mut state = LineState::START;
        for line in 0..lines {
            state = self.lex_end_state(buffer, line, state);
            self.ends.push(state);
        }
    }

    /// The state line `line` starts in.
    pub fn start_state(&self, line: usize) -> LineState {
        match line.checked_sub(1) {
            None => LineState::START,
            Some(previous) => self.ends.get(previous).copied().unwrap_or_default(),
        }
    }

    /// The state line `line` ends in.
    pub fn end_state(&self, line: usize) -> LineState {
        self.ends.get(line).copied().unwrap_or_default()
    }

    /// The spans of `line`, lexed from the cached start state.
    ///
    /// Offsets are relative to the start of the line. Cheap enough to call once
    /// per visible line per frame, which is exactly how the renderer uses it.
    /// With no highlighter the answer is always empty, and the renderer draws
    /// the whole line in the foreground colour.
    pub fn spans(&self, buffer: &Buffer, line: usize) -> Vec<Span> {
        let Some(highlighter) = self.highlighter.as_ref() else {
            return Vec::new();
        };
        let text = buffer.line_text(line);
        self.lex_calls.set(self.lex_calls.get() + 1);
        highlighter.line(&text, self.start_state(line)).0
    }

    /// Brings the cache back into step after `buffer` changed.
    ///
    /// `first` is the first line the edit touched and `added` the number of
    /// lines the replacement spans, both after the edit; `removed` is how many
    /// lines the replaced text spanned before it. Returns the number of lines
    /// that had to be re-lexed, which is what the performance tests read.
    pub fn edited(&mut self, buffer: &Buffer, first: usize, removed: usize, added: usize) -> usize {
        // Make the vector as long as the buffer again. When the edit changed no
        // line count -- typing inside a line, the common case -- this is a
        // no-op rather than a memmove.
        if removed != added {
            let at = (first + 1).min(self.ends.len());
            let old_end = (at + removed).min(self.ends.len());
            self.ends
                .splice(at..old_end, std::iter::repeat_n(LineState::START, added));
        }
        debug_assert_eq!(self.ends.len(), buffer.line_count());
        if self.highlighter.is_none() {
            return 0;
        }

        // Re-lex downwards. Every line inside the edited region has to be
        // redone whatever its end state comes out as; below the region, an
        // unchanged end state means every line under it is unchanged too.
        let lines = buffer.line_count();
        let last_dirty = first + added;
        let mut state = self.start_state(first);
        let mut relexed = 0;
        for line in first..lines {
            state = self.lex_end_state(buffer, line, state);
            relexed += 1;
            let settled = line > last_dirty && self.ends[line] == state;
            self.ends[line] = state;
            if settled {
                break;
            }
        }
        relexed
    }

    /// How many times a line has been lexed through this cache.
    ///
    /// For tests and for profiling; the number is meaningless on its own and
    /// only differences between two reads of it mean anything.
    pub fn lex_calls(&self) -> usize {
        self.lex_calls.get()
    }

    /// The state `line` ends in, given the state it starts in.
    fn lex_end_state(&self, buffer: &Buffer, line: usize, start: LineState) -> LineState {
        let Some(highlighter) = self.highlighter.as_ref() else {
            return LineState::START;
        };
        let text = buffer.line_text(line);
        self.lex_calls.set(self.lex_calls.get() + 1);
        highlighter.line(&text, start).1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_syntax::SqlHighlighter;

    /// A font to hang runs on. Never shaped in these tests -- only the lengths
    /// and the colours are read -- so the family need not exist.
    fn test_font() -> Font {
        gpui::font("test")
    }

    /// The lengths of `runs`, which is what "tiles the text" is asserted on.
    fn lengths(runs: &[TextRun]) -> Vec<usize> {
        runs.iter().map(|run| run.len).collect()
    }

    /// A cache over `text`, with the SQL highlighter.
    fn cache(text: &str) -> (Buffer, SyntaxCache) {
        let buffer = Buffer::new(text);
        let cache = SyntaxCache::new(&buffer, Some(Arc::new(SqlHighlighter)));
        (buffer, cache)
    }

    /// Replaces `range` in both the buffer and the cache, the way the editor
    /// does, and answers with the number of lines re-lexed.
    fn edit(
        buffer: &mut Buffer,
        cache: &mut SyntaxCache,
        range: std::ops::Range<usize>,
        text: &str,
    ) -> usize {
        let first = buffer.line_of(range.start);
        let removed = buffer.line_of(range.end) - first;
        let added = text.bytes().filter(|b| *b == b'\n').count();
        buffer.replace(range, text);
        cache.edited(buffer, first, removed, added)
    }

    /// The token covering `column`, if any.
    fn token_at(spans: &[Span], column: usize) -> Option<Token> {
        spans
            .iter()
            .find(|span| span.range.contains(&column))
            .map(|span| span.token)
    }

    #[test]
    fn a_cache_with_no_highlighter_is_plain_text() {
        let buffer = Buffer::new("select 1;\nselect 2;\n");
        let mut cache = SyntaxCache::new(&buffer, None);
        assert!(cache.spans(&buffer, 0).is_empty());
        assert!(cache.end_state(0).is_start());
        assert_eq!(cache.lex_calls(), 0);
        assert_eq!(cache.edited(&buffer, 0, 0, 0), 0);
    }

    #[test]
    fn an_ordinary_edit_relexes_one_line() {
        let mut text = String::new();
        for i in 0..2000 {
            text.push_str(&format!("select {i} from t;\n"));
        }
        let (mut buffer, mut cache) = cache(&text);

        // An edit on the third line settles on the fourth: the third's end
        // state is unchanged, and the loop stops the moment it sees that.
        let at = buffer.line_start(2) + 6;
        assert_eq!(edit(&mut buffer, &mut cache, at..at, "ion"), 2);
    }

    #[test]
    fn opening_a_block_comment_propagates_and_closing_it_stops() {
        // Two hundred statements with a stray `*/` on the eleventh line: the
        // buffer is long enough that "walked to the end" and "stopped where the
        // states settled" are different numbers.
        let mut text = String::new();
        for line in 0..200 {
            if line == 10 {
                text.push_str("*/\n");
            } else {
                text.push_str(&format!("select {line};\n"));
            }
        }
        let (mut buffer, mut cache) = cache(&text);
        assert!(cache.end_state(0).is_start());

        // Open a block comment on the first line: every line down to the `*/`
        // is now inside it, and the walk stops there rather than at line 200.
        let relexed = edit(&mut buffer, &mut cache, 0..0, "/*");
        assert_eq!(relexed, 11);
        assert!(!cache.end_state(0).is_start());
        assert!(!cache.end_state(9).is_start());
        assert!(cache.end_state(10).is_start());
        assert_eq!(
            token_at(&cache.spans(&buffer, 1), 0),
            Some(Token::Comment),
            "a line inside the comment lexes as comment throughout"
        );

        // Close it again on the first line and the states walk back, no
        // further than they came.
        let relexed = edit(&mut buffer, &mut cache, 2..2, "*/");
        assert_eq!(relexed, 11);
        assert!(cache.end_state(0).is_start());
        assert!(cache.end_state(9).is_start());
        assert_eq!(token_at(&cache.spans(&buffer, 1), 0), Some(Token::Keyword));
    }

    #[test]
    fn splitting_and_joining_lines_keeps_the_cache_the_right_length() {
        let (mut buffer, mut cache) = cache("select 1 from t;\n");
        edit(&mut buffer, &mut cache, 8..9, "\n");
        assert_eq!(buffer.line_count(), 3);
        assert_eq!(buffer.line_text(1), "from t;");

        edit(&mut buffer, &mut cache, 8..9, "");
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line_text(0), "select 1from t;");
    }

    #[test]
    fn an_incremental_cache_agrees_with_a_fresh_one() {
        let (mut buffer, mut cache) = cache("select 'a'\n, 'b' from t;\n-- tail\nselect 2;\n");

        // A quote opened mid-buffer, then closed again, then a whole line
        // pasted in: after each the cache has to match a rebuild.
        for (range, text) in [
            (7..7, "'"),
            (0..0, "/* x\n"),
            (0..5, ""),
            (10..10, "\ninsert into u values ('$$');"),
        ] {
            edit(&mut buffer, &mut cache, range, text);
            let fresh = SyntaxCache::new(&buffer, Some(Arc::new(SqlHighlighter)));
            let incremental: Vec<_> = (0..buffer.line_count())
                .map(|l| cache.end_state(l))
                .collect();
            let rebuilt: Vec<_> = (0..buffer.line_count())
                .map(|l| fresh.end_state(l))
                .collect();
            assert_eq!(incremental, rebuilt, "after {text:?}");
        }
    }

    #[test]
    fn drawing_costs_one_lex_per_visible_line() {
        let mut text = String::new();
        for i in 0..5000 {
            text.push_str(&format!("select {i};\n"));
        }
        let (buffer, cache) = cache(&text);

        let before = cache.lex_calls();
        for line in 100..140 {
            cache.spans(&buffer, line);
        }
        assert_eq!(cache.lex_calls() - before, 40);
    }

    #[test]
    fn swapping_the_highlighter_relexes_everything() {
        let (buffer, mut cache) = cache("class name;\n{}\n");
        assert_eq!(cache.line_comment(), Some("--"));
        assert_eq!(
            token_at(&cache.spans(&buffer, 0), 2),
            Some(Token::Identifier),
            "`class` is a plain identifier to the SQL highlighter"
        );

        cache.set_highlighter(Some(Arc::new(crate::lang::java::JavaHighlighter)), &buffer);
        assert_eq!(cache.line_comment(), Some("//"));
        assert_eq!(
            token_at(&cache.spans(&buffer, 0), 2),
            Some(Token::Keyword),
            "`class` is a keyword to the Java highlighter"
        );
    }

    #[test]
    fn runs_tile_the_text_across_a_gap() {
        let palette = EditorTheme::default();
        let font = test_font();
        // `select 1` with only `select` claimed: the space and the `1` are
        // nobody's token and have to come back as foreground runs.
        let spans = [Span::new(0..6, Token::Keyword)];
        let runs = runs_for_spans("select 1", &spans, &palette, &font);

        assert_eq!(lengths(&runs), vec![6, 2]);
        assert_eq!(runs[0].color, palette.keyword);
        assert_eq!(runs[1].color, palette.foreground);
        assert_eq!(
            runs.iter().map(|run| run.len).sum::<usize>(),
            "select 1".len()
        );
    }

    #[test]
    fn a_leading_gap_is_filled_too() {
        let palette = EditorTheme::default();
        let runs = runs_for_spans(
            "  select",
            &[Span::new(2..8, Token::Keyword)],
            &palette,
            &test_font(),
        );
        assert_eq!(lengths(&runs), vec![2, 6]);
        assert_eq!(runs[0].color, palette.foreground);
    }

    #[test]
    fn spans_past_the_end_and_over_each_other_are_clamped() {
        let palette = EditorTheme::default();
        let font = test_font();
        // A highlighter that has miscounted: one span runs past the line, the
        // next starts inside the one before it. Both are ordinary bugs, and
        // neither may produce runs that do not tile -- gpui panics on those.
        let spans = [
            Span::new(0..99, Token::Keyword),
            Span::new(2..4, Token::String),
            Span::new(50..60, Token::Number),
        ];
        let runs = runs_for_spans("abcd", &spans, &palette, &font);

        assert_eq!(lengths(&runs), vec![4]);
        assert_eq!(runs[0].color, palette.keyword);
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), 4);
    }

    #[test]
    fn empty_text_has_no_runs() {
        let palette = EditorTheme::default();
        assert!(runs_for_spans("", &[], &palette, &test_font()).is_empty());
        // Even with a span over it, which is what a highlighter that answered
        // for the wrong line would hand in.
        assert!(
            runs_for_spans(
                "",
                &[Span::new(0..3, Token::Keyword)],
                &palette,
                &test_font()
            )
            .is_empty()
        );
    }

    #[test]
    fn text_with_no_spans_is_one_foreground_run() {
        let palette = EditorTheme::default();
        let runs = runs_for_spans("plain prose", &[], &palette, &test_font());
        assert_eq!(lengths(&runs), vec!["plain prose".len()]);
        assert_eq!(runs[0].color, palette.foreground);
    }

    #[test]
    fn every_token_maps_to_its_palette_slot() {
        let palette = EditorTheme::default();
        // Written out rather than derived, so that moving a token to another
        // slot has to be a deliberate edit here as well as there.
        let pairs = [
            (Token::Keyword, palette.keyword),
            (Token::String, palette.string),
            (Token::Number, palette.number),
            (Token::Comment, palette.comment),
            (Token::Function, palette.function),
            (Token::Type, palette.r#type),
            (Token::Operator, palette.operator),
            (Token::Identifier, palette.identifier),
            (Token::QuotedIdentifier, palette.identifier),
            (Token::Key, palette.key),
            (Token::Variable, palette.variable),
            (Token::Punctuation, palette.punctuation),
            (Token::BracketMatch, palette.bracket_match),
            (Token::Error, palette.error),
            (Token::Warning, palette.warning),
        ];
        for (token, slot) in pairs {
            assert_eq!(color_for(token, &palette), slot, "{token:?}");
        }
    }
}
