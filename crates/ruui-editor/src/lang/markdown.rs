//! Markdown, coloured the way a reader skims it rather than the way CommonMark
//! parses it.
//!
//! Markdown is the one format here whose *content* is prose, so the job is not
//! to tell code from comments but to make the structure findable: where a
//! section starts, where a code block starts and stops, what is a list and what
//! is quoted from somewhere else. That is a shape a line scanner can see almost
//! all of, because Markdown's block structure is written at the head of a line
//! on purpose — `#`, `>`, `-`, a fence — and only the inline spans need looking
//! along the line for.
//!
//! # The state, in two bits
//!
//! A fenced code block is open or it is not, and the only thing worth
//! remembering about an open one is which character its fence was drawn with,
//! so that a ``` inside a `~~~` block stays body. Two values —
//! `FENCED_BACKTICK` and `FENCED_TILDE` — which is two bits of the sixteen
//! [`LineState::COMPOSABLE_BITS`] allows.
//!
//! # What each thing is coloured as
//!
//! The tokens are shared with six configuration formats, so the mapping is by
//! role rather than by name:
//!
//! * A heading is [`Token::Key`] — the left-hand-side colour. A heading is a
//!   section header, which is exactly what `[section]` is in a `.conf`, and it
//!   takes the whole line because a heading *is* the line.
//! * A fenced code block's body is [`Token::String`]: it is literal text that
//!   the document is quoting rather than saying, which is what a string is in
//!   every other language here. The fence lines themselves are
//!   [`Token::Comment`], being markup about the block rather than part of it —
//!   including the info string, which is spelled the way a comment is read.
//! * A blockquote is [`Token::Comment`], the quiet colour: it is context
//!   carried in from elsewhere, and it should recede the way a comment does.
//! * A horizontal rule is [`Token::Comment`] for the same reason — it is a mark
//!   on the page with no text of its own.
//! * A list marker is [`Token::Keyword`] and *only* the marker, so that the
//!   item's text stays prose and the column of bullets stands out down the left.
//! * Inline code is [`Token::String`], to agree with the fenced kind.
//! * Strong is [`Token::Keyword`] and emphasis is [`Token::Number`]: two
//!   weights of "this word matters more than its neighbours", the louder colour
//!   on the louder markup. `Number` is where a palette puts the literal values
//!   of the configuration formats, and it is what these lexers spend on the
//!   quieter of two emphases.
//! * A link's text is [`Token::Variable`] — it names something that lives
//!   elsewhere, which is what the variable colour means everywhere else here —
//!   and its target is [`Token::String`], the target being a literal.
//! * An HTML comment is [`Token::Comment`], which needs no argument.
//!
//! # What is given up
//!
//! * **Indented code blocks.** Four spaces at the head of a line means code
//!   only when nothing else is open, and a line scanner cannot tell that from
//!   the second paragraph of a list item, which is indented exactly as far.
//!   Colouring both would put half the lists in a document in the string colour;
//!   colouring neither is wrong only where fences are not used, and fences are
//!   what people write.
//! * **YAML front matter.** A `---` on the first line of a file opens it, and
//!   the first line is the one thing this lexer cannot recognise: it is handed
//!   [`LineState::START`], and so is every line after a blank one. Front matter
//!   is therefore not expressible without a state the framework does not have,
//!   and the opening `---` reads as the horizontal rule it also is.
//! * **Setext headings** — a line underlined with `===` or `---` — for the same
//!   reason in reverse: the underline is seen a line too late to recolour the
//!   text above it, and the underline itself already reads as a rule.
//! * **Inline spans do not cross lines.** An emphasis or a code span left open
//!   at the end of a line stays plain rather than swallowing the paragraph, and
//!   an HTML comment left open colours its first line only. A fence is the one
//!   inline-looking thing that carries, and it carries because it is a block.
//! * **Double-backtick code spans** are read as two empty spans around plain
//!   text. `` `` `` is written to put a backtick *inside* code, which is rare
//!   enough not to be worth a second scanner.
//! * **Reference links** — `[text][label]` and the `[label]: url` line that
//!   defines them — are not coloured; only the inline `[text](url)` form is.
//!   The label form resolves against the whole document, and a line scanner
//!   would be guessing.
//! * A heading's line is one run: inline spans inside it are not looked for,
//!   because a heading already has the colour that says what it is.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{Spans, char_step, indent_of, skip_spaces, word_boundary};

/// A ``` fenced block left open on the line before.
const FENCED_BACKTICK: LineState = LineState(1);
/// A `~~~` fenced block left open on the line before.
const FENCED_TILDE: LineState = LineState(2);

/// Markdown.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownHighlighter;

impl Highlighter for MarkdownHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        lex_line(text, state)
    }

    /// None: Markdown has no comment syntax, and its `#` means something else
    /// entirely. A toggle that turned a paragraph into a row of headings would
    /// be worse than no toggle at all.
    fn line_comment(&self) -> Option<&'static str> {
        None
    }
}

/// The character an open fence was drawn with, if one is open.
const fn open_fence(state: LineState) -> Option<u8> {
    match state.0 {
        1 => Some(b'`'),
        2 => Some(b'~'),
        _ => None,
    }
}

/// The state a fence drawn with `fence` leaves behind.
const fn fence_state(fence: u8) -> LineState {
    if fence == b'`' {
        FENCED_BACKTICK
    } else {
        FENCED_TILDE
    }
}

/// The spans of one line of Markdown, and the state it leaves behind.
fn lex_line(line: &str, state: LineState) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();

    // Inside a fenced block nothing else is markup, which is the whole point of
    // one: the body is what the author wanted shown verbatim.
    if let Some(fence) = open_fence(state) {
        if closes_fence(bytes, fence) {
            spans.push(Token::Comment, indent_of(line), len);
            return (spans.finish(), LineState::START);
        }
        spans.push(Token::String, 0, len);
        return (spans.finish(), state);
    }

    let at = indent_of(line);

    // A fence first, because everything after this point would read the block's
    // first line as prose.
    for fence in *b"`~" {
        if fence_run(bytes, fence).is_some() {
            spans.push(Token::Comment, at, len);
            return (spans.finish(), fence_state(fence));
        }
    }

    if heading(bytes, at) {
        spans.push(Token::Key, at, len);
        return (spans.finish(), LineState::START);
    }

    // The rule is tried before the list marker, because `* * *` and `- - -` are
    // both, and what they mean is the rule.
    if horizontal_rule(line) || bytes.get(at) == Some(&b'>') {
        spans.push(Token::Comment, at, len);
        return (spans.finish(), LineState::START);
    }

    let mut from = at;
    if let Some(end) = list_marker(bytes, at) {
        spans.push(Token::Keyword, at, end);
        from = end;
    }

    inline(&mut spans, line, from);
    (spans.finish(), LineState::START)
}

/// The end of a run of three or more `fence` bytes at the head of `line`, past
/// whatever indentation it has.
///
/// Three is the minimum a fence may be and there is no maximum, so this counts
/// rather than compares.
fn fence_run(bytes: &[u8], fence: u8) -> Option<usize> {
    let at = skip_spaces(bytes, 0);
    let mut end = at;
    while bytes.get(end) == Some(&fence) {
        end += 1;
    }
    (end - at >= 3).then_some(end)
}

/// Whether `bytes` is the line that closes a `fence` block.
///
/// A closing fence is a run of the opening character with nothing after it. The
/// specification also asks that it be no shorter than the opening run; that is
/// not tracked, because the case it separates — a four-backtick block closed by
/// three — is vanishingly rare beside the case it costs, which is remembering a
/// length in a state that has to stay small.
fn closes_fence(bytes: &[u8], fence: u8) -> bool {
    fence_run(bytes, fence).is_some_and(|end| skip_spaces(bytes, end) >= bytes.len())
}

/// Whether `line` is a horizontal rule: three or more of `-`, `*` or `_`, alone
/// on the line apart from the spaces that may be sprinkled between them.
fn horizontal_rule(line: &str) -> bool {
    let mut marks = line
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\r'));
    let Some(first) = marks.next() else {
        return false;
    };
    if !matches!(first, b'-' | b'*' | b'_') {
        return false;
    }
    let mut count = 1;
    for byte in marks {
        if byte != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

/// Whether an ATX heading starts at `at`.
///
/// One to six `#`, and then a space or the end of the line. The space is what
/// keeps a `#tag` in prose — and a shell comment in a file somebody mis-set the
/// language of — from turning into a heading.
fn heading(bytes: &[u8], at: usize) -> bool {
    let mut end = at;
    while bytes.get(end) == Some(&b'#') {
        end += 1;
    }
    (1..=6).contains(&(end - at)) && matches!(bytes.get(end), None | Some(b' ' | b'\t'))
}

/// The end of the list marker at `at`, when there is one.
///
/// A bullet is `-`, `*` or `+`; an ordered marker is digits and then `.` or `)`.
/// Either way a space must follow, which is what stops a `-1` in prose and a
/// `1.5` at the head of a line from becoming bullets.
fn list_marker(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at)? {
        b'-' | b'*' | b'+' => {
            matches!(bytes.get(at + 1), None | Some(b' ' | b'\t')).then_some(at + 1)
        }
        byte if byte.is_ascii_digit() => {
            let mut end = at;
            while matches!(bytes.get(end), Some(digit) if digit.is_ascii_digit()) {
                end += 1;
            }
            if !matches!(bytes.get(end), Some(b'.' | b')')) {
                return None;
            }
            end += 1;
            matches!(bytes.get(end), None | Some(b' ' | b'\t')).then_some(end)
        }
        _ => None,
    }
}

/// Scans the prose of a line for the spans that are not prose.
///
/// Everything here closes on the same line or does not count, so the loop can
/// look forward freely and fall back to stepping over one character. All four
/// delimiters are ASCII, so an index found by comparing bytes is always a
/// character boundary.
fn inline(spans: &mut Spans, line: &str, from: usize) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut at = from;

    while at < len {
        let byte = bytes[at];
        match byte {
            b'`' => match find(bytes, at + 1, b'`') {
                Some(end) => {
                    spans.push(Token::String, at, end + 1);
                    at = end + 1;
                }
                None => at += 1,
            },
            b'*' | b'_' => {
                let width = if bytes.get(at + 1) == Some(&byte) {
                    2
                } else {
                    1
                };
                match emphasis(bytes, at, byte, width) {
                    Some(end) => {
                        // Two markers, two weights: `**` is the louder of them
                        // and gets the louder colour.
                        let token = if width == 2 {
                            Token::Keyword
                        } else {
                            Token::Number
                        };
                        spans.push(token, at, end);
                        at = end;
                    }
                    None => at += 1,
                }
            }
            b'[' => match link(bytes, at) {
                Some((text, target)) => {
                    spans.push(Token::Variable, at, text);
                    spans.push(Token::String, text, target);
                    at = target;
                }
                None => at += 1,
            },
            b'<' if bytes[at..].starts_with(b"<!--") => {
                // Unclosed, it colours this line and no more; see the module
                // documentation for why nothing is carried.
                let end = html_comment_end(bytes, at + 4).unwrap_or(len);
                spans.push(Token::Comment, at, end);
                at = end;
            }
            _ => at += char_step(line, at),
        }
    }
}

/// The end of the emphasis span opened by `width` copies of `byte` at `at`, when
/// there is one on this line.
///
/// Two of the flanking rules are worth keeping because each of them stops a
/// common false positive: a marker followed by a space is not an opener, so
/// arithmetic and a bullet in the middle of a sentence stay plain; and an `_`
/// inside a word is not an opener, so `snake_case_names` do not go italic from
/// the middle. The rest of the flanking rules are dropped — they decide cases
/// that are ambiguous to a person reading the source too.
fn emphasis(bytes: &[u8], at: usize, byte: u8, width: usize) -> Option<usize> {
    if !matches!(bytes.get(at + width), Some(next) if !matches!(next, b' ' | b'\t')) {
        return None;
    }
    if byte == b'_' && !word_boundary(bytes, at) {
        return None;
    }
    let mut scan = at + width;
    while scan < bytes.len() {
        if bytes[scan] == byte {
            if width == 1 {
                return Some(scan + 1);
            }
            if bytes.get(scan + 1) == Some(&byte) {
                return Some(scan + 2);
            }
        }
        scan += 1;
    }
    None
}

/// Where the text and the target of an inline link starting at `at` end, when
/// `at` opens one.
///
/// The `]` must be followed immediately by a `(`, which is what tells a link
/// from the `[label]` of a reference and from a `[WARN]` in a pasted log. The
/// target ends at the first `)`, so a URL containing one is cut short — the
/// escape that would fix it is rarer than the URLs it would break.
fn link(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let close = find(bytes, at + 1, b']')?;
    if bytes.get(close + 1) != Some(&b'(') {
        return None;
    }
    let paren = find(bytes, close + 2, b')')?;
    Some((close + 1, paren + 1))
}

/// The end of an HTML comment whose body starts at `from`, past its `-->`.
fn html_comment_end(bytes: &[u8], from: usize) -> Option<usize> {
    let last = bytes.len().checked_sub(2)?;
    (from..last)
        .find(|at| &bytes[*at..*at + 3] == b"-->")
        .map(|at| at + 3)
}

/// The offset of the first `byte` at or after `from`.
fn find(bytes: &[u8], from: usize, byte: u8) -> Option<usize> {
    (from..bytes.len()).find(|at| bytes[*at] == byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line` from a clean state, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&MarkdownHighlighter, line, LineState::START).0
    }

    #[test]
    fn every_state_round_trips_inside_the_composable_budget() {
        for fence in *b"`~" {
            let state = fence_state(fence);
            assert_eq!(state.0 >> LineState::COMPOSABLE_BITS, 0);
            assert_eq!(open_fence(state), Some(fence));
        }
        assert_eq!(open_fence(LineState::START), None);
        assert_ne!(FENCED_BACKTICK, FENCED_TILDE);
    }

    #[test]
    fn a_heading_is_the_whole_line() {
        for line in ["# Title", "###### Deep", "  ## Indented"] {
            assert_eq!(
                spans(line).last().map(|(_, token)| *token),
                Some(Token::Key),
                "{line:?}"
            );
        }
        // Seven is too many, and a `#` with no space is a tag rather than a
        // heading.
        assert!(
            !spans("####### Nope")
                .iter()
                .any(|(_, token)| *token == Token::Key)
        );
        assert!(!spans("#tag").iter().any(|(_, token)| *token == Token::Key));
    }

    #[test]
    fn a_fence_carries_its_body_until_it_closes() {
        let (open, after) = lex(&MarkdownHighlighter, "```rust", LineState::START);
        assert_eq!(open[0].1, Token::Comment);
        assert_eq!(after, FENCED_BACKTICK);

        // Everything inside is literal, markup included.
        let (body, still) = lex(&MarkdownHighlighter, "# not a heading", after);
        assert_eq!(body[0].1, Token::String);
        assert_eq!(still, after);
        assert_eq!(lex(&MarkdownHighlighter, "", after).1, after);

        let (close, closed) = lex(&MarkdownHighlighter, "```", after);
        assert_eq!(close[0].1, Token::Comment);
        assert!(closed.is_start());
    }

    #[test]
    fn a_fence_is_closed_only_by_its_own_character() {
        let (_, after) = lex(&MarkdownHighlighter, "~~~", LineState::START);
        assert_eq!(after, FENCED_TILDE);
        // A backtick fence inside a tilde block is body.
        let (body, still) = lex(&MarkdownHighlighter, "```", after);
        assert_eq!(body[0].1, Token::String);
        assert_eq!(still, after);
        assert!(lex(&MarkdownHighlighter, "~~~~", after).1.is_start());
    }

    #[test]
    fn a_fence_that_never_closes_keeps_carrying() {
        let mut state = lex(&MarkdownHighlighter, "```", LineState::START).1;
        for line in ["one", "", "  two", "## three"] {
            let (_, next) = lex(&MarkdownHighlighter, line, state);
            assert_eq!(next, state, "{line:?}");
            state = next;
        }
        assert!(!state.is_start());
    }

    #[test]
    fn a_closing_fence_may_not_carry_an_info_string() {
        let after = lex(&MarkdownHighlighter, "```", LineState::START).1;
        // Text after the run means this is body, not the end of the block.
        assert_eq!(lex(&MarkdownHighlighter, "``` js", after).1, after);
        // Trailing spaces are forgiven.
        assert!(lex(&MarkdownHighlighter, "```   ", after).1.is_start());
    }

    #[test]
    fn a_list_marker_is_coloured_and_its_text_is_not() {
        assert_eq!(spans("- item"), [("-", Token::Keyword)]);
        assert_eq!(spans("  1. first"), [("1.", Token::Keyword)]);
        assert_eq!(spans("12) twelfth")[0].1, Token::Keyword);
        // A marker needs the space after it.
        assert!(
            !spans("-1 degree")
                .iter()
                .any(|(_, token)| *token == Token::Keyword)
        );
        assert!(
            !spans("1.5 units")
                .iter()
                .any(|(_, token)| *token == Token::Keyword)
        );
    }

    #[test]
    fn the_text_of_a_list_item_is_still_lexed() {
        assert_eq!(
            spans("- see `run.sh`"),
            [("-", Token::Keyword), ("`run.sh`", Token::String)]
        );
    }

    #[test]
    fn a_blockquote_recedes() {
        assert_eq!(spans("> quoted")[0].1, Token::Comment);
        assert_eq!(spans("  > > deep"), [("> > deep", Token::Comment)]);
    }

    #[test]
    fn a_rule_is_a_rule_before_it_is_a_bullet() {
        for line in ["---", "***", "___", "- - -", "*****"] {
            assert_eq!(
                spans(line).last().map(|(_, token)| *token),
                Some(Token::Comment),
                "{line:?}"
            );
        }
        // Two is not enough, and a mixture is not one at all.
        assert!(
            !spans("--")
                .iter()
                .any(|(_, token)| *token == Token::Comment)
        );
        assert!(
            !spans("-*-")
                .iter()
                .any(|(_, token)| *token == Token::Comment)
        );
    }

    #[test]
    fn inline_code_keeps_its_backticks() {
        assert_eq!(spans("run `ls -l` now"), [("`ls -l`", Token::String)]);
    }

    #[test]
    fn strong_and_emphasis_are_two_weights() {
        let found = spans("a **bold** and *thin* word");
        assert!(found.contains(&("**bold**", Token::Keyword)));
        assert!(found.contains(&("*thin*", Token::Number)));
        assert!(spans("__x__").contains(&("__x__", Token::Keyword)));
        assert!(spans("_x_").contains(&("_x_", Token::Number)));
    }

    #[test]
    fn an_unclosed_marker_stays_plain() {
        // No span at all, so the whole line is drawn in the foreground colour.
        for line in ["a * b", "half *open", "2 * 3 = 6", "snake_case_name"] {
            assert_eq!(spans(line), [], "{line:?}");
        }
    }

    #[test]
    fn a_link_names_a_target() {
        assert_eq!(
            spans("see [the guide](docs/x.md) first"),
            [
                ("[the guide]", Token::Variable),
                ("(docs/x.md)", Token::String),
            ]
        );
        // A bracket with no target after it is prose.
        assert_eq!(spans("[WARN] hi"), []);
        assert_eq!(spans("[a][b]"), []);
    }

    #[test]
    fn an_html_comment_is_a_comment() {
        assert!(spans("text <!-- hidden --> more").contains(&("<!-- hidden -->", Token::Comment)));
        // Left open it takes this line and stops there.
        let (found, state) = lex(&MarkdownHighlighter, "<!-- open", LineState::START);
        assert_eq!(found[0].1, Token::Comment);
        assert!(state.is_start());
    }

    #[test]
    fn a_heading_is_not_scanned_for_spans() {
        // One run, so that a heading reads as a heading rather than as a line
        // with holes in it.
        let line = "# A `code` heading";
        assert_eq!(spans(line), [(line, Token::Key)]);
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        for line in [
            "",
            "   ",
            "#",
            "`",
            "*",
            "**",
            "___",
            "[",
            "[]",
            "[](",
            "<!--",
            "-",
            "1.",
            "~~~",
            "제목 `코드` **굵게**",
            "🙂 *🙂* [🙂](🙂)",
        ] {
            for state in [
                LineState::START,
                FENCED_BACKTICK,
                FENCED_TILDE,
                LineState(0xffff),
            ] {
                lex(&MarkdownHighlighter, line, state);
            }
        }
    }
}
