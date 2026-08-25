//! JSON, which is the one configuration format here that fits on a line.
//!
//! A JSON string may not contain a raw newline and JSON has no comments, so
//! there is nothing for a line to leave open and nothing to carry: every line
//! is lexed from [`LineState::START`] and ends there.
//!
//! The one distinction worth making is the one JSON itself does not: a string
//! followed by a `:` is a member name, and a string anywhere else is a value.
//! That is what turns a wall of quotes into a document with a shape.
//!
//! # What is given up
//!
//! An unterminated string is coloured to the end of its line rather than being
//! reported. There is nowhere here to report it to, and a half-typed string
//! reading as a string is exactly right while it is being typed.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{
    Spans, char_step, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// The three bare words JSON allows.
const LITERALS: &[&str] = &["false", "null", "true"];

/// JSON.
///
/// Stateless: [`Highlighter::line`] answers [`LineState::START`] for every
/// line, whatever it was handed.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonHighlighter;

impl Highlighter for JsonHighlighter {
    fn line(&self, text: &str, _state: LineState) -> (Vec<Span>, LineState) {
        (lex_line(text), LineState::START)
    }

    /// None: JSON has no comment syntax at all, so there is nothing the toggle
    /// could write that a JSON reader would skip.
    fn line_comment(&self) -> Option<&'static str> {
        None
    }
}

/// The spans of one line of JSON.
fn lex_line(line: &str) -> Vec<Span> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();
    let mut at = 0;

    while at < len {
        let byte = bytes[at];
        match byte {
            b'"' => match quote_body(line, at + 1, b'"', true) {
                Some(end) => {
                    // A member name is a string with a colon after it, give or
                    // take the whitespace a formatter left in between.
                    let token = if bytes.get(skip_spaces(bytes, end)) == Some(&b':') {
                        Token::Key
                    } else {
                        Token::String
                    };
                    spans.push(token, at, end);
                    at = end;
                }
                None => {
                    spans.push(Token::String, at, len);
                    at = len;
                }
            },
            // A leading `-` belongs to the number only when a digit follows it,
            // so a stray one stays punctuation.
            _ if word_boundary(bytes, at)
                && (byte.is_ascii_digit()
                    || (byte == b'-'
                        && matches!(bytes.get(at + 1), Some(next) if next.is_ascii_digit()))) =>
            {
                let end = number(line, at);
                spans.push(Token::Number, at, end);
                at = end.max(at + 1);
            }
            _ if byte.is_ascii_alphabetic() && word_boundary(bytes, at) => {
                let end = word_end(bytes, at);
                if LITERALS.binary_search(&&line[at..end]).is_ok() {
                    spans.push(Token::Number, at, end);
                }
                at = end;
            }
            _ => at += char_step(line, at),
        }
    }

    spans.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line`, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&JsonHighlighter, line, LineState::START).0
    }

    #[test]
    fn the_literal_table_is_sorted() {
        assert!(LITERALS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_member_name_is_told_from_a_value() {
        assert_eq!(
            spans(r#"{"host": "web"}"#),
            [(r#""host""#, Token::Key), (r#""web""#, Token::String)]
        );
    }

    #[test]
    fn a_name_split_from_its_colon_is_still_a_name() {
        // What a formatter that aligns colons produces.
        assert_eq!(spans(r#""host"   : 1"#)[0].1, Token::Key);
    }

    #[test]
    fn escapes_do_not_end_a_string() {
        let line = r#"{"path": "c:\\x\"y", "n": 1}"#;
        let strings: Vec<_> = spans(line)
            .into_iter()
            .filter(|(_, token)| *token == Token::String)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(strings, [r#""c:\\x\"y""#]);
    }

    #[test]
    fn numbers_and_the_three_bare_words() {
        // The three bare words share the number colour: they are the literal
        // values of a format that has no others.
        assert_eq!(
            spans("[-1.5e3, true, null]"),
            [
                ("-1.5e3", Token::Number),
                ("true", Token::Number),
                ("null", Token::Number),
            ]
        );
    }

    #[test]
    fn an_unterminated_string_stops_at_the_line() {
        let line = r#"{"half: "#;
        let (found, state) = lex(&JsonHighlighter, line, LineState::START);
        assert_eq!(found.last(), Some(&(r#""half: "#, Token::String)));
        assert!(state.is_start(), "nothing crosses a line in JSON");
    }

    #[test]
    fn nothing_here_carries_and_nothing_here_panics() {
        for line in ["", "\"", "-", "{}", "[,]", r#"{"한글": "값"}"#, "🙂"] {
            // Every line from every state, since the cache may hand this one
            // whatever the line before it left behind.
            for state in [LineState::START, LineState(1), LineState(0xffff)] {
                let (_, end) = lex(&JsonHighlighter, line, state);
                assert!(end.is_start(), "{line:?}");
            }
        }
    }
}
