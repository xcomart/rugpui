//! The flat configuration formats: `.ini`, `.conf`, `.cfg`, `.properties`,
//! `.env`, and the `sshd_config` family.
//!
//! One lexer for all of them because they are one format with several
//! punctuations. A line is a comment, a `[section]`, or a mapping — and the
//! mapping is spelled `key = value`, `key: value` or `key value` depending on
//! whose parser reads it. Nothing crosses a line, so every line is lexed from
//! [`LineState::START`] and ends there.
//!
//! # What is given up
//!
//! * `key value` is only read as a mapping when the key is a bare word, so a
//!   line of an `/etc/hosts`-shaped file is not turned into a key by accident.
//! * A trailing `#` is only a comment when whitespace precedes it, because a
//!   `#` is a legal character in a password and half of these files hold one.
//! * `.properties` continuation lines — a value ending in `\` — are not
//!   followed. The next line reads as another mapping, which is what it looks
//!   like.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{
    Spans, char_step, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// The words a value can be instead of a string or a number.
///
/// The union of what these formats spell a boolean with, since no single one of
/// them agrees with the others and a file is never ambiguous about which it
/// meant.
const LITERALS: &[&str] = &[
    "FALSE", "False", "NO", "OFF", "ON", "TRUE", "True", "YES", "false", "no", "none", "null",
    "off", "on", "true", "yes",
];

/// The one word that can stand in front of a key, in a `.env` file meant to be
/// sourced as well as read.
const EXPORT: &str = "export";

/// The flat configuration formats.
///
/// Stateless: [`Highlighter::line`] answers [`LineState::START`] for every
/// line, whatever it was handed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfHighlighter;

impl Highlighter for ConfHighlighter {
    fn line(&self, text: &str, _state: LineState) -> (Vec<Span>, LineState) {
        (lex_line(text), LineState::START)
    }

    /// `#`, which every one of these formats accepts even where it also has a
    /// spelling of its own.
    fn line_comment(&self) -> Option<&'static str> {
        Some("#")
    }
}

/// The spans of one line of a flat configuration file.
fn lex_line(line: &str) -> Vec<Span> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();
    let head = skip_spaces(bytes, 0);

    // `;` is the ini spelling, `!` the `.properties` one, and `#` everyone's.
    if matches!(bytes.get(head), Some(b'#' | b';' | b'!')) {
        spans.push(Token::Comment, head, len);
        return spans.finish();
    }

    if bytes.get(head) == Some(&b'[') {
        // To the last `]` on the line, so `[a.b]` and a stray one both come out
        // as the header they are being typed towards.
        let end = line.rfind(']').map_or(len, |at| at + 1);
        spans.push(Token::Key, head, end);
        return value(spans, line, end);
    }

    let mut at = head;
    if line[at..].starts_with(EXPORT) && matches!(bytes.get(at + EXPORT.len()), Some(b' ' | b'\t'))
    {
        spans.push(Token::Keyword, at, at + EXPORT.len());
        at = skip_spaces(bytes, at + EXPORT.len());
    }

    let key = word_end(bytes, at);
    if key > at {
        // The separator decides whether this was a mapping at all. An `=` or a
        // `:` says so outright; whitespace says so only when something follows
        // it, which is how `sshd_config` writes one and how a bare word alone
        // on a line stays a bare word.
        match bytes.get(key) {
            Some(b'=' | b':') => {
                spans.push(Token::Key, at, key);
                at = key + 1;
            }
            Some(b' ' | b'\t') if skip_spaces(bytes, key) < len => {
                spans.push(Token::Key, at, key);
                at = key;
            }
            _ => {}
        }
    }

    value(spans, line, at)
}

/// Scans the value side of a line — everything a mapping's key does not cover.
fn value(mut spans: Spans, line: &str, from: usize) -> Vec<Span> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut at = from;

    while at < len {
        let byte = bytes[at];
        match byte {
            b'#' | b';' if at > 0 && matches!(bytes.get(at - 1), Some(b' ' | b'\t')) => {
                spans.push(Token::Comment, at, len);
                at = len;
            }
            b'"' | b'\'' => {
                let end = quote_body(line, at + 1, byte, byte == b'"').unwrap_or(len);
                spans.push(Token::String, at, end);
                at = end;
            }
            // A `$VAR` in a `.env` file, and in every `.conf` that is read by a
            // shell before it is read by anything else.
            b'$' if matches!(bytes.get(at + 1), Some(b'{')) => {
                let mut end = at + 2;
                while end < len && bytes[end] != b'}' {
                    end += char_step(line, end);
                }
                let end = if end < len { end + 1 } else { len };
                spans.push(Token::Variable, at, end);
                at = end;
            }
            b'$' => {
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
                at = end.max(at + 1);
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

    spans.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line`, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&ConfHighlighter, line, LineState::START).0
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
    fn all_three_comment_marks_work() {
        for line in ["# a", "; a", "! a", "   # a"] {
            assert!(has(line, Token::Comment), "{line:?} was not a comment");
        }
    }

    #[test]
    fn a_section_header_is_one_key() {
        assert_eq!(spans("[server]"), [("[server]", Token::Key)]);
    }

    #[test]
    fn the_three_spellings_of_a_mapping() {
        for line in ["Port = 22", "Port: 22", "Port 22"] {
            assert_eq!(spans(line)[0], ("Port", Token::Key), "{line:?}");
            assert!(has(line, Token::Number), "{line:?}");
        }
    }

    #[test]
    fn a_bare_word_on_its_own_is_not_a_key() {
        // Nothing follows it, so nothing was mapped to anything -- and with no
        // span at all the word is drawn in the foreground colour.
        assert_eq!(spans("standalone"), []);
    }

    #[test]
    fn an_exported_variable_keeps_its_key() {
        assert_eq!(
            spans("export TOKEN=abc"),
            [("export", Token::Keyword), ("TOKEN", Token::Key)]
        );
    }

    #[test]
    fn a_hash_inside_a_value_is_part_of_it() {
        // The password case, which is why the rule asks for whitespace.
        assert!(!has("pass = a#b", Token::Comment));
        assert!(has("pass = a # b", Token::Comment));
    }

    #[test]
    fn a_value_can_be_quoted_expanded_or_a_literal() {
        let line = r#"url = "http://$HOST/${path}" # see below"#;
        let found = spans(line);
        assert_eq!(found[0], ("url", Token::Key));
        assert!(found.iter().any(|(_, token)| *token == Token::String));
        assert!(found.iter().any(|(_, token)| *token == Token::Comment));
        // `true` shares the number colour: the literal values of a format with
        // no others.
        assert!(has("debug = true", Token::Number));
        assert!(has("home = $HOME", Token::Variable));
    }

    #[test]
    fn nothing_here_carries_and_nothing_here_panics() {
        for line in [
            "",
            "[",
            "]",
            "=",
            "$",
            "${",
            "\"",
            "키 = 값 # 주석",
            "🙂=🙂",
        ] {
            for state in [LineState::START, LineState(1), LineState(0xffff)] {
                let (_, end) = lex(&ConfHighlighter, line, state);
                assert!(end.is_start(), "{line:?}");
            }
        }
    }
}
