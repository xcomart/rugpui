//! Two highlighters over one document: a base language with a second one
//! painted over it.
//!
//! Some documents are two languages at once. A template is a file of some other
//! language — Java, XML, PHP, SQL — with `${…}` statements sprinkled through
//! it, and the useful colouring of one is both at once: the Java is Java, and
//! the statements stand out of it. A shell script with an embedded here-doc,
//! a Markdown file with fenced code, a query with a host language's
//! placeholders in it: the shape recurs. That is what this composes.
//!
//! The base is any [`Highlighter`]. The layer on top is an [`Overlay`], which
//! is a highlighter that also says *where* it took charge, so that the two can
//! be told apart without either having to know the other's grammar. This crate
//! ships no overlay of its own — an overlay is a grammar, and which grammar is
//! painted over which base is the host's question, not this crate's.
//!
//! ```ignore
//! let java = Arc::new(JavaHighlighter);
//! let highlighter = Arc::new(CompositeHighlighter::new(java, Arc::new(MyStatements)));
//! editor.set_highlighter(Some(highlighter), cx);
//! ```
//!
//! # How the two are kept apart
//!
//! [`Overlay::regions`] answers with the byte ranges the overlay's own
//! constructs cover as well as with its spans. The base runs over the whole
//! line — so that its own state stays coherent — and its spans are then cut
//! wherever an overlay region stands. What is left is the base's opinion about
//! the text between them, and the overlay's about the regions themselves, with
//! no overlap and in order, which is the contract [`Highlighter::line`] owes its
//! caller.
//!
//! # The one thing a composable highlighter has to promise
//!
//! Both states have to fit in one [`LineState`], so each gets half of it:
//! [`LineState::pack`] puts the base's in the low sixteen bits and the
//! overlay's in the high sixteen. A highlighter that means to be composed must
//! keep its state inside [`LineState::COMPOSABLE_BITS`], which the lexers
//! shipped here do with room to spare — three bits for SQL.
//!
//! # What it does not do
//!
//! Picking the base from the file's extension is the *host*'s decision and not
//! this crate's: nothing here touches a file system.
//! [`highlighter_for_extension`](crate::lang::highlighter_for_extension) is the
//! table a host would look the base up in, and the composition itself is what
//! is settled here.
//!
//! The base sees the overlay's text as well as the text around it, so a `${`
//! inside what the base would call a string can still confuse the base's own
//! state. Cutting the regions out of the base's *input* instead would fix that
//! and break something worse — the base would lex `"a" + "b"` as two unrelated
//! fragments whenever a region stood between them — so the base reads the line
//! whole, and the cut happens to its output.

use std::ops::Range;
use std::sync::Arc;

use crate::highlight::{Highlighter, LineState, Span};

/// A language painted *over* a base one, which knows where it took charge.
///
/// An ordinary [`Highlighter`] answers only with spans, and spans are not
/// enough to compose: a span the overlay did not emit may still be text the
/// overlay owns — the space inside a statement, say — and the base's opinion
/// about it has to be thrown away all the same. [`Overlay::regions`] is what
/// says so.
pub trait Overlay: Send + Sync {
    /// Lexes one line, answering with the overlay's own spans, the byte ranges
    /// it takes charge of, and the state the line ends in.
    ///
    /// The regions must be sorted, non-overlapping and inside `text`, and every
    /// span must lie within one of them — the composition walks each list once
    /// and relies on both. The end state must fit in
    /// [`LineState::COMPOSABLE_BITS`].
    fn regions(&self, text: &str, state: LineState) -> Overlaid;
}

/// What one line of an [`Overlay`] came to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Overlaid {
    /// The overlay's own spans, sorted and non-overlapping.
    pub spans: Vec<Span>,
    /// The byte ranges the overlay takes charge of, sorted and
    /// non-overlapping. Whatever the base said about these is discarded.
    pub regions: Vec<Range<usize>>,
    /// The state the line ends in, within [`LineState::COMPOSABLE_BITS`].
    pub state: LineState,
}

/// A base-language highlighter with an [`Overlay`] painted over it.
pub struct CompositeHighlighter {
    /// The language the file is written in.
    base: Arc<dyn Highlighter>,
    /// The layer on top, which always wins where the two meet.
    overlay: Arc<dyn Overlay>,
}

impl CompositeHighlighter {
    /// Paints `overlay` over `base`.
    pub fn new(base: Arc<dyn Highlighter>, overlay: Arc<dyn Overlay>) -> Self {
        Self { base, overlay }
    }

    /// The base language, for a caller that wants to ask it something.
    pub fn base(&self) -> &Arc<dyn Highlighter> {
        &self.base
    }

    /// The layer on top, likewise.
    pub fn overlay(&self) -> &Arc<dyn Overlay> {
        &self.overlay
    }
}

impl Highlighter for CompositeHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        let (base_state, overlay_state) = state.unpack();
        let found = self.overlay.regions(text, overlay_state);
        let (base_spans, base_end) = self.base.line(text, base_state);

        let mut spans = Vec::with_capacity(base_spans.len() + found.spans.len());
        clip(base_spans, &found.regions, &mut spans);
        spans.extend(found.spans);
        spans.sort_by_key(|span| span.range.start);
        (spans, LineState::pack(base_end, found.state))
    }

    fn line_comment(&self) -> Option<&'static str> {
        // The base language's, because that is what the file is: commenting a
        // line out of a Java file with statements in it still writes `//`, and
        // an overlay rarely has a comment of its own to offer.
        self.base.line_comment()
    }
}

/// Writes the parts of `spans` that no region covers into `out`.
///
/// `regions` is sorted and its members do not overlap, which is what
/// [`Overlay::regions`] promises, so one walk over each is enough.
fn clip(spans: Vec<Span>, regions: &[Range<usize>], out: &mut Vec<Span>) {
    if regions.is_empty() {
        out.extend(spans);
        return;
    }
    for span in spans {
        let mut at = span.range.start;
        for region in regions {
            if region.end <= at {
                continue;
            }
            if region.start >= span.range.end {
                break;
            }
            if region.start > at {
                out.push(Span::new(at..region.start, span.token));
            }
            at = region.end;
        }
        if at < span.range.end {
            out.push(Span::new(at..span.range.end, span.token));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::Token;
    use crate::sql_syntax::SqlHighlighter;

    /// A minimal overlay, standing in for whatever grammar a host paints over
    /// a base: everything between `${` and the first `}` is its region, the
    /// braces are punctuation and the name between them is a keyword. Nine
    /// bits of state would be a real template language; one is enough to show
    /// that a region carries over a line break.
    struct Braces;

    /// The one state `Braces` has: the previous line left a `${` open.
    const INSIDE: LineState = LineState(1);

    impl Overlay for Braces {
        fn regions(&self, text: &str, state: LineState) -> Overlaid {
            let mut out = Overlaid::default();
            let bytes = text.as_bytes();
            // `(where the region started, where its name starts)`. A region
            // carried over from the line before starts at 0 and its name with
            // it; one opened here starts at the `$` and its name after the `{`.
            let mut open = (state == INSIDE).then_some((0usize, 0usize));
            let mut at = 0;
            while at < bytes.len() {
                match open {
                    None => {
                        if bytes[at..].starts_with(b"${") {
                            out.spans.push(Span::new(at..at + 2, Token::Punctuation));
                            open = Some((at, at + 2));
                            at += 2;
                        } else {
                            at += 1;
                        }
                    }
                    Some((region, name)) if bytes[at] == b'}' => {
                        if name < at {
                            out.spans.push(Span::new(name..at, Token::Keyword));
                        }
                        out.spans.push(Span::new(at..at + 1, Token::Punctuation));
                        out.regions.push(region..at + 1);
                        open = None;
                        at += 1;
                    }
                    Some(_) => at += 1,
                }
            }
            if let Some((region, name)) = open {
                if name < text.len() {
                    out.spans.push(Span::new(name..text.len(), Token::Keyword));
                }
                out.regions.push(region..text.len());
                out.state = INSIDE;
            }
            out.spans.sort_by_key(|span| span.range.start);
            out
        }
    }

    /// `(text, token)` for every span of `line`, lexed from `state`.
    fn lex<'a>(
        highlighter: &CompositeHighlighter,
        line: &'a str,
        state: LineState,
    ) -> (Vec<(&'a str, Token)>, LineState) {
        let (spans, end) = highlighter.line(line, state);
        let mut last = 0;
        for span in &spans {
            assert!(
                span.range.start >= last,
                "spans overlap or are unsorted in {line:?}: {spans:?}"
            );
            last = span.range.end;
        }
        (
            spans
                .iter()
                .map(|span| (&line[span.range.clone()], span.token))
                .collect(),
            end,
        )
    }

    fn composite() -> CompositeHighlighter {
        CompositeHighlighter::new(Arc::new(SqlHighlighter), Arc::new(Braces))
    }

    #[test]
    fn the_base_paints_the_text_and_the_overlay_paints_its_regions() {
        let highlighter = composite();
        let (spans, state) = lex(&highlighter, "select ${name} from t", LineState::START);
        assert_eq!(
            spans,
            vec![
                ("select", Token::Keyword),
                ("${", Token::Punctuation),
                ("name", Token::Keyword),
                ("}", Token::Punctuation),
                ("from", Token::Keyword),
                ("t", Token::Identifier),
            ],
            "the SQL either side of the region is still SQL, and the \
             placeholder the base would have called a type is the overlay's"
        );
        assert!(state.is_start());
        assert_eq!(highlighter.line_comment(), Some("--"));
    }

    #[test]
    fn both_states_survive_one_line_state() {
        let highlighter = composite();
        // A block comment the base opens and a region the overlay opens, both
        // left hanging on the same line: neither may overwrite the other.
        let (_, state) = lex(&highlighter, "/* a ${key", LineState::START);
        let (base, overlay) = state.unpack();
        assert!(!base.is_start(), "the base is inside its block comment");
        assert!(!overlay.is_start(), "the overlay is inside its region");

        let (spans, state) = lex(&highlighter, "more} still */ select", state);
        assert_eq!(
            spans,
            vec![
                ("more", Token::Keyword),
                ("}", Token::Punctuation),
                (" still */", Token::Comment),
                ("select", Token::Keyword),
            ],
            "the region closes on the overlay's state and the comment on the \
             base's"
        );
        assert!(state.is_start());
    }

    #[test]
    fn clipping_cuts_a_base_span_a_region_stands_inside() {
        let spans = vec![Span::new(0..20, Token::Comment)];
        let mut out = Vec::new();
        clip(spans, &[5..8, 12..14], &mut out);
        assert_eq!(
            out,
            vec![
                Span::new(0..5, Token::Comment),
                Span::new(8..12, Token::Comment),
                Span::new(14..20, Token::Comment),
            ]
        );
    }
}
