//! YAML, as far as one line at a time can see it.
//!
//! YAML is context-sensitive in ways a line scanner cannot follow, so this
//! follows the two things that carry the meaning on screen: the key of a
//! mapping, and where a block scalar's body begins and ends. Everything else —
//! anchors, tags, flow collections — is scanned the way the shell lexer scans a
//! command line, one interesting thing at a time.
//!
//! # The state, in thirteen bits
//!
//! One thing crosses a line: a block scalar, and what has to be remembered
//! about it is the indentation of the line that introduced it. That is the low
//! bit as a flag and twelve bits of column above it, which fits the sixteen
//! [`LineState::COMPOSABLE_BITS`] allows with three to spare. A line indented
//! more than [`INDENT_LIMIT`] columns saturates there rather than wrapping,
//! which ends the scalar a little late in a file nobody has ever written.
//!
//! # What is given up
//!
//! * A block scalar's body is "every line indented further than the line that
//!   opened it, plus the blank lines between them". The specification says the
//!   indentation is fixed by the scalar's *first* line and may be given
//!   explicitly by a digit; the simplification differs only for a body whose
//!   first line is indented *less* than its introducer, which cannot happen in
//!   a valid document.
//! * A quoted scalar that spans lines is not carried over. Multi-line flow
//!   scalars are rare, and carrying them would mean guessing at the folding
//!   rules to know where they end.
//! * `- ` and `:` are structure, not tokens: colouring them is what makes a
//!   YAML file look like a punctuation exercise.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{
    Spans, char_step, indent_of, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// The deepest indentation a carried block scalar remembers.
///
/// Twelve bits, which is four thousand columns of leading space. Anything
/// deeper saturates here, so the scalar ends at the first line indented less
/// than this rather than at the first line indented less than its introducer.
const INDENT_LIMIT: usize = (1 << 12) - 1;

/// The scalars that are values rather than words.
///
/// The YAML 1.1 set, which is what most readers still implement: `yes`, `no`,
/// `on` and `off` are booleans there, and a file that relies on it reads
/// better when they are coloured as the booleans they will become.
const LITERALS: &[&str] = &[
    "FALSE", "False", "NO", "NULL", "Null", "OFF", "ON", "TRUE", "True", "YES", "false", "no",
    "null", "off", "on", "true", "yes",
];

/// YAML.
#[derive(Debug, Clone, Copy, Default)]
pub struct YamlHighlighter;

impl Highlighter for YamlHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        lex_line(text, state)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("#")
    }
}

/// The state a block scalar introduced at column `indent` leaves behind.
const fn block_scalar_state(indent: usize) -> LineState {
    let indent = if indent > INDENT_LIMIT {
        INDENT_LIMIT
    } else {
        indent
    };
    LineState(((indent as u32) << 1) | 1)
}

/// The indentation of the line that introduced the open block scalar, if one is
/// open.
const fn open_block_scalar(state: LineState) -> Option<usize> {
    if state.0 & 1 == 0 {
        None
    } else {
        Some((state.0 >> 1) as usize)
    }
}

/// The spans of one line of YAML, and the state it leaves behind.
fn lex_line(line: &str, state: LineState) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let len = bytes.len();

    if let Some(introduced_at) = open_block_scalar(state) {
        // A blank line inside a block scalar belongs to it however far it is
        // indented, which is to say not at all.
        if line.trim().is_empty() || indent_of(line) > introduced_at {
            let mut spans = Spans::new();
            spans.push(Token::String, 0, len);
            return (spans.finish(), state);
        }
        // Otherwise the scalar ended, and this line is a document line again.
    }

    let mut spans = Spans::new();
    let indent = indent_of(line);
    let mut at = indent;

    // The document markers, which are the whole line when they are there.
    let trimmed = line.trim_end();
    if trimmed == "---" || trimmed == "..." {
        spans.push(Token::Keyword, at, trimmed.len());
        return (spans.finish(), LineState::START);
    }

    if bytes.get(at) == Some(&b'#') {
        spans.push(Token::Comment, at, len);
        return (spans.finish(), LineState::START);
    }

    // Sequence indicators, however many of them are nested on this line.
    while bytes.get(at) == Some(&b'-') && matches!(bytes.get(at + 1), None | Some(b' ')) {
        at = skip_spaces(bytes, at + 1);
    }

    if let Some(colon) = key_end(line, at) {
        spans.push(Token::Key, at, colon);
        at = skip_spaces(bytes, colon + 1);
    }

    // A block scalar is introduced where a value would go: after the colon of a
    // mapping entry, or after the dash of a sequence one.
    if let Some(end) = block_scalar(line, at) {
        spans.push(Token::Keyword, at, end);
        let rest = skip_spaces(bytes, end);
        if bytes.get(rest) == Some(&b'#') {
            spans.push(Token::Comment, rest, len);
        }
        return (spans.finish(), block_scalar_state(indent));
    }

    while at < len {
        let byte = bytes[at];
        match byte {
            // YAML wants whitespace before an inline `#`, which is what stops a
            // URL's fragment from turning the rest of the line grey.
            b'#' if at == 0 || matches!(bytes.get(at - 1), Some(b' ' | b'\t')) => {
                spans.push(Token::Comment, at, len);
                at = len;
            }
            b'\'' | b'"' => match quote_body(line, at + 1, byte, byte == b'"') {
                Some(end) => {
                    spans.push(Token::String, at, end);
                    at = end;
                }
                None => {
                    // A flow scalar left open is not carried to the next line;
                    // see the module documentation.
                    spans.push(Token::String, at, len);
                    at = len;
                }
            },
            // An anchor, an alias, or a tag: all three name something elsewhere
            // in the document, which is what the variable colour is for.
            b'&' | b'*' | b'!' if word_boundary(bytes, at) => {
                let end = word_end(bytes, at + 1);
                if end > at + 1 {
                    spans.push(Token::Variable, at, end);
                    at = end;
                } else {
                    at += 1;
                }
            }
            b'0'..=b'9' if word_boundary(bytes, at) => {
                let end = number(line, at);
                spans.push(Token::Number, at, end);
                at = end;
            }
            _ if (byte.is_ascii_alphabetic() || byte == b'_') && word_boundary(bytes, at) => {
                let end = word_end(bytes, at);
                if LITERALS.binary_search(&&line[at..end]).is_ok() {
                    spans.push(Token::Number, at, end);
                }
                at = end;
            }
            _ => at += char_step(line, at),
        }
    }

    (spans.finish(), LineState::START)
}

/// The offset of the `:` that ends the key starting at `at`, if this line is a
/// mapping entry.
///
/// A key ends at a `:` that is followed by a space or by the end of the line —
/// which is what keeps a `http://host` in a value from being read as one — and
/// a `#` comment or a quote that runs to the end of the line says there is no
/// key here at all.
fn key_end(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut scan = at;
    while scan < bytes.len() {
        match bytes[scan] {
            b'#' if scan > at && matches!(bytes.get(scan - 1), Some(b' ' | b'\t')) => return None,
            quote @ (b'\'' | b'"') => scan = quote_body(line, scan + 1, quote, quote == b'"')?,
            b':' if matches!(bytes.get(scan + 1), None | Some(b' ' | b'\t')) => {
                return (scan > at).then_some(scan);
            }
            _ => scan += char_step(line, scan),
        }
    }
    None
}

/// The end of a block scalar header — `|`, `>`, and the chomping and indentation
/// indicators after it — when `at` is one, and nothing when it is not.
///
/// The header has to be the last thing on the line apart from a comment;
/// anything else means the `|` was a plain scalar that happens to start with a
/// pipe.
fn block_scalar(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if !matches!(bytes.get(at), Some(b'|' | b'>')) {
        return None;
    }
    let mut end = at + 1;
    while matches!(bytes.get(end), Some(byte) if *byte == b'+' || *byte == b'-' || byte.is_ascii_digit())
    {
        end += 1;
    }
    let rest = skip_spaces(bytes, end);
    if rest >= bytes.len() || bytes[rest] == b'#' {
        Some(end)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line` from a clean state, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&YamlHighlighter, line, LineState::START).0
    }

    /// Whether any span of `line` came out as `token`.
    fn has(line: &str, token: Token) -> bool {
        spans(line).iter().any(|(_, found)| *found == token)
    }

    #[test]
    fn the_literal_table_is_sorted() {
        assert!(LITERALS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn every_state_round_trips_inside_the_composable_budget() {
        for indent in [0, 1, 2, 7, 4094, INDENT_LIMIT] {
            let state = block_scalar_state(indent);
            assert_eq!(
                state.0 >> LineState::COMPOSABLE_BITS,
                0,
                "{indent} overflowed the budget"
            );
            assert_eq!(open_block_scalar(state), Some(indent));
        }
        // Deeper than the field holds saturates rather than wrapping, so a
        // scalar ends late rather than at the wrong place entirely.
        assert_eq!(
            open_block_scalar(block_scalar_state(INDENT_LIMIT + 9)),
            Some(INDENT_LIMIT)
        );
        assert_eq!(open_block_scalar(LineState::START), None);
    }

    #[test]
    fn a_mapping_entry_splits_into_key_and_value() {
        assert_eq!(
            spans("  port: 22"),
            [("port", Token::Key), ("22", Token::Number)]
        );
    }

    #[test]
    fn a_sequence_entry_still_has_a_key() {
        assert_eq!(spans("  - name: web"), [("name", Token::Key)]);
    }

    #[test]
    fn a_colon_inside_a_value_does_not_end_a_key() {
        // The regression this rule exists for: `url: http://host/x` has one key
        // and not two.
        let keys: Vec<_> = spans("url: http://host/x")
            .into_iter()
            .filter(|(_, token)| *token == Token::Key)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(keys, ["url"]);
    }

    #[test]
    fn a_quoted_key_keeps_its_quotes() {
        assert_eq!(spans(r#""a: b": c"#), [(r#""a: b""#, Token::Key)]);
    }

    #[test]
    fn booleans_and_nulls_share_the_number_colour() {
        assert!(has("enabled: true", Token::Number));
        assert!(has("x: null", Token::Number));
        assert!(has("x: yes", Token::Number));
        // A word that merely contains one is not one, and gets no span at all.
        assert_eq!(spans("x: trueish"), [("x", Token::Key)]);
    }

    #[test]
    fn a_comment_needs_whitespace_before_it() {
        assert!(has("a: b # why", Token::Comment));
        assert!(!has("a: b#c", Token::Comment));
        assert_eq!(spans("# whole line")[0].1, Token::Comment);
    }

    #[test]
    fn a_block_scalar_takes_everything_indented_under_it() {
        let (opened, after) = lex(&YamlHighlighter, "  script: |", LineState::START);
        assert_eq!(opened[0].1, Token::Key);
        assert_eq!(after, block_scalar_state(2));

        let (body, still) = lex(&YamlHighlighter, "    echo hi: not a key", after);
        assert_eq!(body[0].1, Token::String);
        assert_eq!(still, after);

        // A blank line is part of the body.
        assert_eq!(lex(&YamlHighlighter, "", after).1, after);

        // And a line back at the introducer's indentation closes it.
        let (next, closed) = lex(&YamlHighlighter, "  other: 1", after);
        assert!(closed.is_start());
        assert_eq!(next[0].1, Token::Key);
    }

    #[test]
    fn a_chomping_indicator_is_part_of_the_header() {
        assert_eq!(
            lex(&YamlHighlighter, "a: >-", LineState::START).1,
            block_scalar_state(0)
        );
        // A pipe with something after it is a scalar that starts with a pipe.
        assert!(
            lex(&YamlHighlighter, "a: | b", LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn anchors_and_aliases_point_somewhere() {
        assert!(has("base: &defaults", Token::Variable));
        assert!(has("x: *defaults", Token::Variable));
    }

    #[test]
    fn document_markers_stand_alone() {
        assert_eq!(spans("---")[0].1, Token::Keyword);
        assert_eq!(spans("...")[0].1, Token::Keyword);
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        for line in [
            "",
            ":",
            "-",
            "- ",
            "&",
            "*",
            "|",
            "'",
            "\"",
            "한글: 값 # 주석",
            "🙂: 🙂",
        ] {
            for state in [
                LineState::START,
                block_scalar_state(0),
                block_scalar_state(INDENT_LIMIT),
                LineState(0xffff),
            ] {
                lex(&YamlHighlighter, line, state);
            }
        }
    }
}
