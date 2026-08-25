//! Dockerfiles, which are one instruction per line until a `\` says otherwise.
//!
//! The instruction is the whole of the structure — everything else on the line
//! is an argument to it — so that is what this finds first, and it only looks
//! for one where a line can start. A line the previous one ended with a `\` is
//! a continuation, and its first word is an argument like any other; that is
//! the whole of what a [`LineState`] carries here.
//!
//! # The state, in one bit
//!
//! [`CONTINUED`] and nothing else, which leaves fifteen of the sixteen bits
//! [`LineState::COMPOSABLE_BITS`] allows spare — so a Dockerfile can be the
//! base language under an overlay
//! ([`CompositeHighlighter`](crate::composite::CompositeHighlighter)) with room
//! to spare on both sides.
//!
//! # What is given up
//!
//! The body of a `RUN` is shell and is not lexed as shell. Handing the rest of
//! the line to the shell lexer would mean carrying its quote and heredoc states
//! through this one, for a file whose shell fragments are usually a single
//! command; what is here instead is the part of shell that a Dockerfile
//! actually leans on — quoting and `$` expansion — inlined.

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{
    Spans, char_step, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// The state of a line the one before it ended with a `\`.
///
/// The only state this lexer has, so it is one bit and the low one.
const CONTINUED: LineState = LineState(1);

/// Everything the builder accepts at the head of a line.
///
/// Compared case-insensitively — a lower-case `from` is legal — but written the
/// way a Dockerfile writes it. Short enough that a scan beats a binary search
/// and, more to the point, beats upper-casing the word to look it up.
const INSTRUCTIONS: &[&str] = &[
    "ADD",
    "ARG",
    "CMD",
    "COPY",
    "ENTRYPOINT",
    "ENV",
    "EXPOSE",
    "FROM",
    "HEALTHCHECK",
    "LABEL",
    "MAINTAINER",
    "ONBUILD",
    "RUN",
    "SHELL",
    "STOPSIGNAL",
    "USER",
    "VOLUME",
    "WORKDIR",
];

/// The words that mean something inside an instruction rather than at the head
/// of one: the `AS` of a named build stage.
const MODIFIERS: &[&str] = &["AS"];

/// `Dockerfile`, `Dockerfile.*` and `Containerfile`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DockerfileHighlighter;

impl Highlighter for DockerfileHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        lex_line(text, state)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("#")
    }
}

/// The spans of one line of a Dockerfile, and the state it leaves behind.
fn lex_line(line: &str, state: LineState) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();
    let head = skip_spaces(bytes, 0);

    // A `# syntax=` directive is a comment to everything except the builder,
    // and colouring it as one says the right thing about what it does to the
    // image: nothing.
    if bytes.get(head) == Some(&b'#') {
        spans.push(Token::Comment, head, len);
        return (spans.finish(), LineState::START);
    }

    let mut at = head;
    if state != CONTINUED {
        let end = word_end(bytes, at);
        if end > at
            && INSTRUCTIONS
                .iter()
                .any(|word| word.eq_ignore_ascii_case(&line[at..end]))
        {
            spans.push(Token::Keyword, at, end);
            at = end;
        }
    }

    while at < len {
        let byte = bytes[at];
        match byte {
            b'"' | b'\'' => {
                let end = quote_body(line, at + 1, byte, byte == b'"').unwrap_or(len);
                spans.push(Token::String, at, end);
                at = end;
            }
            b'$' => {
                let end = if bytes.get(at + 1) == Some(&b'{') {
                    let mut scan = at + 2;
                    while scan < len && bytes[scan] != b'}' {
                        scan += char_step(line, scan);
                    }
                    if scan < len { scan + 1 } else { len }
                } else {
                    word_end(bytes, at + 1)
                };
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
                let word = &line[at..end];
                if MODIFIERS
                    .iter()
                    .any(|other| other.eq_ignore_ascii_case(word))
                {
                    spans.push(Token::Keyword, at, end);
                } else if bytes.get(end) == Some(&b'=') {
                    // `ENV k=v`, `ARG k=v`, `LABEL k=v`: the name being bound
                    // is the key of a mapping wherever it appears.
                    spans.push(Token::Key, at, end);
                }
                at = end;
            }
            _ => at += char_step(line, at),
        }
    }

    // A trailing `\` joins this line to the next, so the next one must not go
    // looking for an instruction at its head.
    let state = if line.trim_end().ends_with('\\') {
        CONTINUED
    } else {
        LineState::START
    };
    (spans.finish(), state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line` from a clean state, as `(text, token)` pairs.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&DockerfileHighlighter, line, LineState::START).0
    }

    #[test]
    fn the_only_state_fits_the_composable_budget() {
        assert_eq!(CONTINUED.0 >> LineState::COMPOSABLE_BITS, 0);
        assert!(!CONTINUED.is_start());
    }

    #[test]
    fn every_instruction_is_recognised_in_either_case() {
        for instruction in INSTRUCTIONS {
            let line = format!("{instruction} x");
            assert_eq!(spans(&line)[0].1, Token::Keyword, "{instruction}");
            let lowered = format!("{} x", instruction.to_ascii_lowercase());
            assert_eq!(spans(&lowered)[0].1, Token::Keyword, "{lowered}");
        }
        // Kept in alphabetical order so that a reader can find one.
        assert!(INSTRUCTIONS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn an_instruction_leads_the_line() {
        assert_eq!(
            spans("FROM debian:12 AS build"),
            [
                ("FROM", Token::Keyword),
                ("12", Token::Number),
                ("AS", Token::Keyword),
            ]
        );
    }

    #[test]
    fn a_lower_case_instruction_is_still_one() {
        assert_eq!(spans("from debian")[0].1, Token::Keyword);
    }

    #[test]
    fn a_word_that_only_looks_like_an_instruction_is_not_one() {
        assert!(
            !spans("  RUNNER x")
                .iter()
                .any(|(_, token)| *token == Token::Keyword)
        );
    }

    #[test]
    fn a_continuation_has_no_instruction_of_its_own() {
        let (_, after) = lex(
            &DockerfileHighlighter,
            "RUN apt-get update \\",
            LineState::START,
        );
        assert_eq!(after, CONTINUED);

        // `run_it` here is an argument to the line above, not a new instruction.
        let (found, closed) = lex(&DockerfileHighlighter, "  && run_it", after);
        assert!(!found.iter().any(|(_, token)| *token == Token::Keyword));
        assert!(closed.is_start());
    }

    #[test]
    fn a_binding_names_a_key() {
        assert_eq!(
            spans("ENV PATH=/usr/bin"),
            [("ENV", Token::Keyword), ("PATH", Token::Key)]
        );
    }

    #[test]
    fn expansions_and_strings_survive() {
        let line = r#"RUN echo "$HOME" ${TARGET:-x}"#;
        let found = spans(line);
        assert!(found.iter().any(|(_, token)| *token == Token::String));
        assert_eq!(
            found
                .iter()
                .filter(|(_, token)| *token == Token::Variable)
                .map(|(text, _)| *text)
                .collect::<Vec<_>>(),
            ["${TARGET:-x}"],
            "the one inside the quotes stays part of the string"
        );
    }

    #[test]
    fn a_directive_is_a_comment() {
        assert_eq!(spans("# syntax=docker/dockerfile:1")[0].1, Token::Comment);
        // And a comment does not continue, whatever it ends with.
        assert!(
            lex(&DockerfileHighlighter, "# a \\", LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        for line in ["", "\\", "$", "${", "\"", "RUN", "ENV 키=값", "🙂"] {
            for state in [LineState::START, CONTINUED, LineState(0xffff)] {
                lex(&DockerfileHighlighter, line, state);
            }
        }
    }
}
