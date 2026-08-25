//! The byte-scanning helpers the configuration-format lexers share, and the
//! span builder that keeps them to the contract.
//!
//! # Terminal-level highlighting, not a parser
//!
//! Every lexer built on this is a hand-written state machine over bytes, and
//! none of them builds a tree. What an editor needs is what a good `cat` would
//! give you: a comment is grey, a string is green, the left-hand side of a
//! mapping stands out from the right. A `.yml` that is invalid YAML still has
//! to be readable *while it is being fixed*, which is the argument against a
//! real parser as much as the size of one is — a parser tells you the document
//! is wrong, and a scanner just keeps colouring. So the rule every lexer here
//! is held to is that it never panics and never refuses: a line of random bytes
//! comes out as no spans at all, not as an error.
//!
//! # Spans need not tile the line
//!
//! This is the one place these lexers depart from the ones they were grown in.
//! [`Highlighter::line`](crate::highlight::Highlighter::line) hands the
//! renderer the runs it has an opinion about, and the bytes between them are
//! drawn in the palette's foreground colour by the element. So [`Spans`] emits
//! nothing for the plain stretches rather than filling them in, which is both
//! less work per line and less to compare in a test: the interesting spans of a
//! line of prose are what a reader is checking, and a list of them interleaved
//! with the spaces in between says the same thing twice.

use crate::highlight::{Span, Token};

/// Spans under construction, in the order they were found.
///
/// A lexer built on this cannot produce an overlap or an empty span: it says
/// where the interesting runs are, moving forwards, and anything that would
/// step back onto what it has already claimed is clamped rather than allowed
/// through. That is the contract [`Highlighter::line`] owes its caller — sorted,
/// non-overlapping, inside the line, on character boundaries, never empty — and
/// it is worth taking out of the hands of eight separate loops.
///
/// [`Highlighter::line`]: crate::highlight::Highlighter::line
pub(crate) struct Spans {
    /// What has been decided.
    spans: Vec<Span>,
    /// The end of the last span pushed; nothing may start before it.
    at: usize,
}

impl Spans {
    /// An empty line's worth.
    pub(crate) const fn new() -> Self {
        Self {
            spans: Vec::new(),
            at: 0,
        }
    }

    /// Records `at..end` as `token`.
    ///
    /// `at` must not be behind a span already pushed; a lexer that scans
    /// forwards cannot do otherwise, and one that tried is clamped rather than
    /// allowed to produce an overlap. An empty run is dropped, since a span
    /// covering nothing is not one.
    pub(crate) fn push(&mut self, token: Token, at: usize, end: usize) {
        let at = at.max(self.at);
        let end = end.max(at);
        if end > at {
            self.spans.push(Span::new(at..end, token));
            self.at = end;
        }
    }

    /// The spans, in the order they were found.
    pub(crate) fn finish(self) -> Vec<Span> {
        self.spans
    }
}

/// How many bytes the character at `at` takes.
///
/// Read off the lead byte rather than by slicing, so that a caller that has
/// somehow landed off a boundary advances by one byte instead of panicking. A
/// lexer here only ever lands on boundaries — it splits on ASCII and steps by
/// this — but "never panics" is the promise these modules are built on.
pub(crate) fn char_step(line: &str, at: usize) -> usize {
    match line.as_bytes().get(at) {
        None => 1,
        Some(0xc0..=0xdf) => 2,
        Some(0xe0..=0xef) => 3,
        Some(0xf0..=0xf7) => 4,
        Some(_) => 1,
    }
}

/// Whether `at` begins a word rather than continuing one.
pub(crate) fn word_boundary(bytes: &[u8], at: usize) -> bool {
    match at.checked_sub(1).and_then(|before| bytes.get(before)) {
        None => true,
        Some(byte) => !byte.is_ascii_alphanumeric() && *byte != b'_',
    }
}

/// The end of the `[A-Za-z0-9_]` word starting at `at`.
pub(crate) fn word_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    end
}

/// The first byte at or after `at` that is not a space or a tab.
pub(crate) fn skip_spaces(bytes: &[u8], at: usize) -> usize {
    let mut at = at;
    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    at
}

/// How many bytes of leading space and tab `line` has.
pub(crate) fn indent_of(line: &str) -> usize {
    skip_spaces(line.as_bytes(), 0)
}

/// The end of a quote body — the byte after the closing `quote` — starting at
/// `at`, which is the first byte *inside* the quote.
///
/// `None` when the line ends before the quote closes, which is the caller's cue
/// to colour the rest of the line and, if its language allows it, carry the
/// quote to the next line.
pub(crate) fn quote_body(line: &str, at: usize, quote: u8, escapes: bool) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut at = at;
    while at < bytes.len() {
        let byte = bytes[at];
        if escapes && byte == b'\\' {
            // A trailing backslash escapes the line break itself, so the string
            // is still open whatever comes next.
            if at + 1 >= bytes.len() {
                return None;
            }
            at += 1 + char_step(line, at + 1);
        } else if byte == quote {
            return Some(at + 1);
        } else {
            at += char_step(line, at);
        }
    }
    None
}

/// The end of the number starting at `at`.
///
/// Deliberately greedy across `.`, `:` and `-` when a digit follows, so that a
/// version, an IPv4 address and a TOML timestamp each come out as one number
/// rather than as three with punctuation between them. That is a lie about the
/// grammar and the truth about how they read.
pub(crate) fn number(line: &str, at: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = at;
    if matches!(bytes.get(end), Some(b'-' | b'+')) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'0')
        && matches!(
            bytes.get(end + 1).map(|byte| byte | 32),
            Some(b'x' | b'b' | b'o')
        )
    {
        end += 2;
        while matches!(bytes.get(end), Some(byte) if byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            end += 1;
        }
        return end;
    }
    let digits = |bytes: &[u8], from: usize| {
        let mut at = from;
        while matches!(bytes.get(at), Some(byte) if byte.is_ascii_digit() || *byte == b'_') {
            at += 1;
        }
        at
    };
    end = digits(bytes, end);
    while matches!(bytes.get(end), Some(b'.' | b':' | b'-'))
        && matches!(bytes.get(end + 1), Some(byte) if byte.is_ascii_digit())
    {
        end = digits(bytes, end + 1);
    }
    if matches!(bytes.get(end).map(|byte| byte | 32), Some(b'e')) {
        let mut after = end + 1;
        if matches!(bytes.get(after), Some(b'+' | b'-')) {
            after += 1;
        }
        if matches!(bytes.get(after), Some(byte) if byte.is_ascii_digit()) {
            end = digits(bytes, after);
        }
    }
    end
}

/// The interpreter a `#!` line names, reduced to its last path segment.
///
/// `#!/usr/bin/env bash` names it in the second word, which is why this is not
/// a `split('/').last()` at the call site. `None` when the line is not a
/// shebang or names nothing.
pub(crate) fn shebang_interpreter(first_line: &str) -> Option<&str> {
    let rest = first_line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let mut interpreter = words.next()?;
    if interpreter.rsplit('/').next() == Some("env") {
        interpreter = words.next()?;
    }
    Some(interpreter.rsplit('/').next().unwrap_or(interpreter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_swallows_what_reads_as_part_of_it() {
        for (line, end) in [
            ("1", 1),
            ("1.5", 3),
            ("127.0.0.1", 9),
            ("2026-08-08", 10),
            ("12:00:00", 8),
            ("0xdeadBEEF", 10),
            ("1_000", 5),
            ("1e-3", 4),
            ("1.5e3x", 5),
        ] {
            assert_eq!(number(line, 0), end, "{line}");
        }
    }

    #[test]
    fn stepping_over_a_character_never_lands_inside_one() {
        for line in ["a🙂b", "한글", "\u{1F1F0}\u{1F1F7}"] {
            let mut at = 0;
            while at < line.len() {
                assert!(line.is_char_boundary(at), "{line:?} at {at}");
                at += char_step(line, at);
            }
            assert_eq!(at, line.len());
        }
    }

    #[test]
    fn a_shebang_is_read_down_to_its_interpreter() {
        assert_eq!(shebang_interpreter("#!/bin/sh"), Some("sh"));
        assert_eq!(shebang_interpreter("#!/bin/bash -e"), Some("bash"));
        assert_eq!(
            shebang_interpreter("#!/usr/bin/env python3"),
            Some("python3")
        );
        assert_eq!(shebang_interpreter("#!/usr/bin/env"), None);
        assert_eq!(shebang_interpreter("not a shebang"), None);
    }

    #[test]
    fn a_span_that_would_step_back_is_clamped_rather_than_overlapping() {
        let mut spans = Spans::new();
        spans.push(Token::Comment, 2, 6);
        // Behind what was pushed: clamped forwards, and the empty remainder
        // dropped.
        spans.push(Token::String, 0, 4);
        spans.push(Token::Number, 6, 8);
        assert_eq!(
            spans.finish(),
            [
                Span::new(2..6, Token::Comment),
                Span::new(6..8, Token::Number),
            ]
        );
    }
}
