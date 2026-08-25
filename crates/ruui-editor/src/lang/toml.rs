//! TOML: tables, keys, and the one construct that crosses a line.
//!
//! TOML is regular enough that a line scanner gets most of it right. The three
//! things worth being careful about are the ones a reader looks for: the
//! `[table]` header that says where you are, the key on the left of an `=`, and
//! the `"""` string that runs on until it is closed.
//!
//! # The state, in two bits
//!
//! A multi-line string is open or it is not, and the only thing that has to be
//! remembered about an open one is which quote byte opened it —
//! [`MULTILINE_DOUBLE`] or [`MULTILINE_SINGLE`]. Two values, so the whole state
//! is two bits of the sixteen [`LineState::COMPOSABLE_BITS`] allows.
//!
//! # What is given up
//!
//! * A key is a word with an `=` after it, whatever the nesting. That gets
//!   `a = 1`, `a.b = 1` and `{ x = 1 }` right and would call the `x` in a
//!   comparison a key too, except that TOML has no comparisons.
//! * A multi-line string's closing delimiter is found by searching for three
//!   quotes, without regard for a backslash before them. `\"""` inside a
//!   `"""` string ends it early, in colour only.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{Spans, char_step, number, quote_body, skip_spaces, word_boundary};

/// A `"""` string left open on the line before.
const MULTILINE_DOUBLE: LineState = LineState(1);
/// A `'''` string left open on the line before.
const MULTILINE_SINGLE: LineState = LineState(2);

/// The two bare words TOML allows as values.
const LITERALS: &[&str] = &["false", "true"];

/// TOML.
#[derive(Debug, Clone, Copy, Default)]
pub struct TomlHighlighter;

impl Highlighter for TomlHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        lex_line(text, state)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("#")
    }
}

/// The quote byte an open multi-line string was opened with, if one is open.
const fn open_quote(state: LineState) -> Option<u8> {
    match state.0 {
        1 => Some(b'"'),
        2 => Some(b'\''),
        _ => None,
    }
}

/// The state a multi-line string opened with `quote` leaves behind.
const fn carry(quote: u8) -> LineState {
    if quote == b'"' {
        MULTILINE_DOUBLE
    } else {
        MULTILINE_SINGLE
    }
}

/// The spans of one line of TOML, and the state it leaves behind.
fn lex_line(line: &str, state: LineState) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();
    let mut at = 0;

    if let Some(quote) = open_quote(state) {
        match triple_end(line, 0, quote) {
            Some(end) => {
                spans.push(Token::String, 0, end);
                at = end;
            }
            None => {
                spans.push(Token::String, 0, len);
                return (spans.finish(), state);
            }
        }
    }

    // A `[table]` or `[[array]]` header, which is only a header at the head of
    // a line: a `[` anywhere else opens an array.
    let head = skip_spaces(bytes, at);
    if at == 0 && bytes.get(head) == Some(&b'[') {
        let end = header_end(line, head);
        spans.push(Token::Key, head, end);
        at = end;
    }

    while at < len {
        let byte = bytes[at];
        match byte {
            b'#' => {
                spans.push(Token::Comment, at, len);
                at = len;
            }
            b'"' | b'\''
                if bytes.get(at + 1) == Some(&byte) && bytes.get(at + 2) == Some(&byte) =>
            {
                match triple_end(line, at + 3, byte) {
                    Some(end) => {
                        spans.push(Token::String, at, end);
                        at = end;
                    }
                    None => {
                        spans.push(Token::String, at, len);
                        return (spans.finish(), carry(byte));
                    }
                }
            }
            b'"' | b'\'' => {
                let end = quote_body(line, at + 1, byte, byte == b'"').unwrap_or(len);
                // A quoted key is still a key.
                let token = if bytes.get(skip_spaces(bytes, end)) == Some(&b'=') {
                    Token::Key
                } else {
                    Token::String
                };
                spans.push(token, at, end);
                at = end;
            }
            _ if word_boundary(bytes, at)
                && (byte.is_ascii_digit()
                    || (matches!(byte, b'-' | b'+')
                        && matches!(bytes.get(at + 1), Some(next) if next.is_ascii_digit()))) =>
            {
                let end = number(line, at);
                spans.push(Token::Number, at, end);
                at = end.max(at + 1);
            }
            _ if (byte.is_ascii_alphabetic() || byte == b'_') && word_boundary(bytes, at) => {
                let end = bare_key_end(bytes, at);
                let word = &line[at..end];
                if LITERALS.binary_search(&word).is_ok() {
                    spans.push(Token::Number, at, end);
                } else if matches!(bytes.get(skip_spaces(bytes, end)), Some(b'=' | b'.')) {
                    // The `.` is what makes both halves of a dotted key read as
                    // one thing rather than as a key with a word in front of it.
                    spans.push(Token::Key, at, end);
                }
                at = end;
            }
            _ => at += char_step(line, at),
        }
    }

    (spans.finish(), LineState::START)
}

/// The end of a table header whose `[` is at `at`.
///
/// The last `]` before a comment, so that `[[a.b]]` comes out whole; a header
/// that never closes takes the rest of the line, which is what it looks like
/// while it is being typed.
fn header_end(line: &str, at: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = at;
    let mut close = None;
    while end < bytes.len() {
        match bytes[end] {
            b'#' => break,
            b']' => {
                close = Some(end + 1);
                end += 1;
            }
            b'"' | b'\'' => {
                end =
                    quote_body(line, end + 1, bytes[end], bytes[end] == b'"').unwrap_or(bytes.len())
            }
            _ => end += char_step(line, end),
        }
    }
    close.unwrap_or(bytes.len())
}

/// The end of a multi-line string body starting at `at`, delimited by three
/// `quote` bytes.
fn triple_end(line: &str, at: usize, quote: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut end = at;
    while end + 3 <= bytes.len() {
        if bytes[end] == quote && bytes[end + 1] == quote && bytes[end + 2] == quote {
            return Some(end + 3);
        }
        end += char_step(line, end);
    }
    None
}

/// The end of the bare key starting at `at`: TOML allows `-` in one, unlike
/// most of the formats here.
fn bare_key_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    while matches!(bytes.get(end), Some(byte) if byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
    {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line` from a clean state, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&TomlHighlighter, line, LineState::START).0
    }

    #[test]
    fn every_state_round_trips_inside_the_composable_budget() {
        for quote in *b"\"'" {
            let state = carry(quote);
            assert_eq!(state.0 >> LineState::COMPOSABLE_BITS, 0);
            assert_eq!(open_quote(state), Some(quote));
        }
        assert_eq!(open_quote(LineState::START), None);
        assert_ne!(MULTILINE_DOUBLE, MULTILINE_SINGLE);
    }

    #[test]
    fn a_table_header_is_one_key() {
        assert_eq!(spans("[server.http]"), [("[server.http]", Token::Key)]);
        assert_eq!(
            spans("[[hosts]] # many"),
            [("[[hosts]]", Token::Key), ("# many", Token::Comment)]
        );
    }

    #[test]
    fn a_key_is_the_word_before_the_equals() {
        assert_eq!(
            spans("keep-alive = 30"),
            [("keep-alive", Token::Key), ("30", Token::Number)]
        );
    }

    #[test]
    fn both_halves_of_a_dotted_key_are_keys() {
        let keys: Vec<_> = spans("a.b = 1")
            .into_iter()
            .filter(|(_, token)| *token == Token::Key)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(keys, ["a", "b"]);
    }

    #[test]
    fn an_inline_table_keeps_its_keys() {
        assert_eq!(
            spans("point = { x = 1, y = 2 }")
                .iter()
                .filter(|(_, token)| *token == Token::Key)
                .count(),
            3
        );
    }

    #[test]
    fn a_quoted_key_is_a_key_and_a_quoted_value_is_a_string() {
        assert_eq!(
            spans(r#""a b" = "c""#),
            [(r#""a b""#, Token::Key), (r#""c""#, Token::String)]
        );
    }

    #[test]
    fn a_multiline_string_carries_to_where_it_closes() {
        let (opened, after) = lex(&TomlHighlighter, r#"text = """first"#, LineState::START);
        assert_eq!(opened[0].1, Token::Key);
        assert_eq!(after, MULTILINE_DOUBLE);

        let (middle, still) = lex(&TomlHighlighter, "second # not a comment", after);
        assert_eq!(middle[0].1, Token::String);
        assert_eq!(still, after);

        let (last, closed) = lex(&TomlHighlighter, r#"third""" # a comment"#, after);
        assert!(closed.is_start());
        assert_eq!(last[0].1, Token::String);
        assert_eq!(last.last().expect("spans").1, Token::Comment);
    }

    #[test]
    fn a_triple_quote_that_opens_and_closes_on_one_line_carries_nothing() {
        assert!(
            lex(&TomlHighlighter, r#"a = """one""""#, LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn a_date_reads_as_one_number() {
        assert_eq!(
            spans("when = 2026-08-08"),
            [("when", Token::Key), ("2026-08-08", Token::Number)]
        );
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        for line in [
            "",
            "[",
            "[[",
            "\"\"\"",
            "'''",
            "=",
            "a =",
            "키 = \"값\"",
            "🙂 = 🙂",
        ] {
            for state in [
                LineState::START,
                MULTILINE_DOUBLE,
                MULTILINE_SINGLE,
                LineState(0xffff),
            ] {
                lex(&TomlHighlighter, line, state);
            }
        }
    }
}
