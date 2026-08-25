//! Shell: `sh`, `bash`, `zsh`, and the rc files they read.
//!
//! The two things that make a script hard to read without colour are quoting
//! and expansion, so those are what this is careful about: a `'` and a `"`
//! behave differently, an unterminated one runs on to the next line, and
//! `${...}` is not the same as the text around it. Everything else is a word
//! list.
//!
//! # The state, and why a heredoc tag is not in it
//!
//! Two things cross a line here: a quote, which is one byte to remember, and a
//! heredoc, which is a tag of up to [`TAG_LIMIT`] bytes and a `<<-` flag. A
//! [`LineState`] is one `u32` of which a composable lexer may use sixteen bits
//! ([`LineState::COMPOSABLE_BITS`]), so the tag cannot live in it.
//!
//! What lives there instead is an *index* into a table of tags this highlighter
//! interns as it meets them — ten bits of it, so [`TAG_SLOTS`] distinct tags in
//! one document. The table only ever grows, and a tag already in it keeps the
//! index it was given, so the same `<<EOF` on two lines is the same state and
//! the syntax cache stops re-lexing where it should. A document with more than
//! a thousand *distinct* heredoc tags stops tracking the ones past that: they
//! are lexed as ordinary shell, which is wrong in colour and in nothing else.
//! That is the same answer a tag longer than [`TAG_LIMIT`] gets, and for the
//! same reason — better one un-coloured body than a state that cannot be
//! represented.
//!
//! # What is given up
//!
//! * A `$VAR` inside a double-quoted string stays part of the string. Splitting
//!   the run would be easy and would make every quoted path in the file flicker
//!   between two colours; a string that reads as one thing is worth more.
//! * Only the first heredoc on a line is tracked. `cmd <<A <<B` is legal and
//!   nobody writes it.
//! * A backslash at the end of a line joins it to the next one, which this does
//!   not follow. It costs nothing: the next line is lexed from the start state,
//!   and outside a quote that is where a continued command is anyway.

use std::sync::Mutex;

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::scan::{
    Spans, char_step, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// The longest heredoc tag this lexer tracks.
///
/// Sixteen bytes covers `EOF`, `SQL`, `PYTHON_SCRIPT` and every tag anybody
/// actually writes; a longer one means the heredoc is not tracked at all and
/// its body is lexed as ordinary shell.
const TAG_LIMIT: usize = 16;

/// How many distinct heredoc tags one highlighter can carry states for.
///
/// Ten bits of the sixteen a composable state may use. See the module header
/// for what happens to the thousand-and-first.
const TAG_SLOTS: usize = 1 << 10;

/// A state that carries an open quote. Which quote it was is [`FLAG`].
const QUOTE: u32 = 1;
/// A state that carries an open heredoc.
const HEREDOC: u32 = 2;
/// Set on a [`QUOTE`] state opened with `'` rather than `"`, and on a
/// [`HEREDOC`] state opened with `<<-` rather than `<<`.
const FLAG: u32 = 4;
/// How far the heredoc tag's index is shifted up, past [`HEREDOC`] and
/// [`FLAG`].
const INDEX_SHIFT: u32 = 3;

/// The words that give a script its shape, and the builtins that do the work.
///
/// One table rather than two because they land in the same colour: the
/// distinction between `if` and `export` is real to a shell and invisible to
/// someone scanning a file for what it does. Sorted, so the lookup is a binary
/// search; a test holds the order.
const KEYWORDS: &[&str] = &[
    "alias", "break", "case", "cd", "continue", "declare", "do", "done", "echo", "elif", "else",
    "esac", "eval", "exec", "exit", "export", "fi", "for", "function", "if", "in", "local",
    "printf", "read", "readonly", "return", "select", "set", "shift", "source", "then", "time",
    "trap", "typeset", "umask", "unalias", "unset", "until", "wait", "while",
];

/// The words that are values rather than commands.
const LITERALS: &[&str] = &["false", "true"];

/// An interned heredoc tag: the bytes that will close it, and how many of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tag {
    /// The tag, `len` bytes of it.
    bytes: [u8; TAG_LIMIT],
    /// How much of `bytes` is the tag.
    len: u8,
}

impl Tag {
    /// Whether `line` is the terminator, given how the heredoc was opened.
    ///
    /// Trailing whitespace is forgiven and leading whitespace is forgiven only
    /// for `<<-`, which is roughly what a shell does and exactly what a person
    /// reading the file expects.
    fn terminates(&self, line: &str, dash: bool) -> bool {
        let candidate = if dash { line.trim_start() } else { line };
        candidate.trim_end().as_bytes() == &self.bytes[..self.len as usize]
    }
}

/// `sh`, `bash`, `zsh` and the rc files they read.
///
/// Holds the table of heredoc tags it has met — see the module header for why
/// the tag cannot live in the [`LineState`] itself. Two editors over two
/// documents want two of these; sharing one is harmless but fills its table
/// with tags neither document uses.
#[derive(Debug, Default)]
pub struct ShellHighlighter {
    /// The tags met so far, in the order they were first seen. A state's index
    /// is a position in here.
    tags: Mutex<Vec<Tag>>,
}

impl Highlighter for ShellHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        self.lex_line(text, state)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("#")
    }
}

impl ShellHighlighter {
    /// A highlighter with an empty tag table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The index `tag` is interned at, or `None` when the table is full or the
    /// tag will not fit in one.
    fn intern(&self, tag: &str) -> Option<usize> {
        if tag.is_empty() || tag.len() > TAG_LIMIT {
            return None;
        }
        let mut bytes = [0; TAG_LIMIT];
        bytes[..tag.len()].copy_from_slice(tag.as_bytes());
        let tag = Tag {
            bytes,
            len: tag.len() as u8,
        };

        let mut tags = self
            .tags
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(found) = tags.iter().position(|known| *known == tag) {
            return Some(found);
        }
        if tags.len() >= TAG_SLOTS {
            return None;
        }
        tags.push(tag);
        Some(tags.len() - 1)
    }

    /// The tag `index` was interned at, if anything was.
    fn tag(&self, index: usize) -> Option<Tag> {
        self.tags
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(index)
            .copied()
    }

    /// The spans of one line of shell, and the state it leaves behind.
    fn lex_line(&self, line: &str, state: LineState) -> (Vec<Span>, LineState) {
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut spans = Spans::new();
        let mut at = 0;

        match state.0 & 0b11 {
            HEREDOC => {
                let dash = state.0 & FLAG != 0;
                let index = (state.0 >> INDEX_SHIFT) as usize;
                // A state naming a tag this highlighter never interned cannot
                // arise from its own lexing; drawing the line as code is what
                // breaking that assumption should cost.
                match self.tag(index) {
                    Some(tag) if !tag.terminates(line, dash) => {
                        spans.push(Token::String, 0, len);
                        return (spans.finish(), state);
                    }
                    // The terminator is code again, and falls through to the
                    // loop.
                    _ => {}
                }
            }
            QUOTE => {
                let quote = if state.0 & FLAG != 0 { b'\'' } else { b'"' };
                match quote_body(line, 0, quote, quote == b'"') {
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
            _ => {}
        }

        // Set when a `<<TAG` is seen, and handed to the next line at the end.
        let mut opened = None;

        while at < len {
            let byte = bytes[at];
            match byte {
                // A `#` only opens a comment where a word could start, so `x#y`
                // is one word and not a comment. The other place a `#` is
                // ordinary — inside a `${name#prefix}` — never reaches here,
                // because the `$` arm below has already swallowed the whole
                // expansion.
                b'#' if word_boundary(bytes, at) => {
                    spans.push(Token::Comment, at, len);
                    at = len;
                }
                b'\'' | b'"' => {
                    // Single quotes take no escapes at all, which is the whole
                    // reason a script uses them.
                    match quote_body(line, at + 1, byte, byte == b'"') {
                        Some(end) => {
                            spans.push(Token::String, at, end);
                            at = end;
                        }
                        None => {
                            spans.push(Token::String, at, len);
                            let carry = QUOTE | if byte == b'\'' { FLAG } else { 0 };
                            return (spans.finish(), LineState(carry));
                        }
                    }
                }
                b'$' => {
                    let end = expansion(line, at);
                    if end > at {
                        spans.push(Token::Variable, at, end);
                        at = end;
                    } else {
                        // `$(` and a bare `$`: step over it so that what is
                        // inside a command substitution is lexed as the code it
                        // is.
                        at += 1;
                    }
                }
                // `<<<` is a here-string, and the whole operator has to be
                // stepped over at once: leaving the last two bytes to the arm
                // below would read them as the `<<` they are not.
                b'<' if bytes.get(at + 1) == Some(&b'<') && bytes.get(at + 2) == Some(&b'<') => {
                    at += 3;
                }
                // `<<` opens a heredoc; a lone `<` is a redirect.
                b'<' if bytes.get(at + 1) == Some(&b'<') => match self.heredoc_tag(line, at + 2) {
                    Some((start, end, carry)) => {
                        spans.push(Token::String, start, end);
                        opened = Some(carry);
                        at = end;
                    }
                    None => at += 2,
                },
                b'0'..=b'9' if word_boundary(bytes, at) => {
                    let end = number(line, at);
                    spans.push(Token::Number, at, end);
                    at = end;
                }
                _ if (byte.is_ascii_alphabetic() || byte == b'_') && word_boundary(bytes, at) => {
                    let end = word_end(bytes, at);
                    let word = &line[at..end];
                    if LITERALS.contains(&word) {
                        spans.push(Token::Number, at, end);
                    } else if KEYWORDS.binary_search(&word).is_ok() {
                        spans.push(Token::Keyword, at, end);
                    } else if bytes.get(end) == Some(&b'=') {
                        // `PORT=22`, and `local x=1` after the keyword: the name
                        // being assigned reads as the key of a mapping, because
                        // that is what it is.
                        spans.push(Token::Key, at, end);
                    }
                    at = end;
                }
                _ => at += char_step(line, at),
            }
        }

        (spans.finish(), opened.unwrap_or(LineState::START))
    }

    /// The tag of a heredoc introduced at `at`, which is just past the `<<`.
    ///
    /// Answers the span to colour — the tag with its quotes, if it has any —
    /// and the state to carry. `None` when what follows is not a tag, which is
    /// what `x << 2` looks like, and when the tag cannot be interned.
    fn heredoc_tag(&self, line: &str, at: usize) -> Option<(usize, usize, LineState)> {
        let bytes = line.as_bytes();
        let mut at = at;
        let dash = bytes.get(at) == Some(&b'-');
        if dash {
            at += 1;
        }
        let start = skip_spaces(bytes, at);
        let (end, tag) = match bytes.get(start) {
            // `<<'EOF'` and `<<"EOF"` turn expansion off inside the body, which
            // this does not colour differently; the quotes are part of the span
            // either way.
            Some(quote @ (b'\'' | b'"')) => {
                let end = quote_body(line, start + 1, *quote, false)?;
                (end, line.get(start + 1..end - 1)?)
            }
            Some(byte) if byte.is_ascii_alphabetic() || *byte == b'_' => {
                let end = word_end(bytes, start);
                (end, &line[start..end])
            }
            _ => return None,
        };
        let index = self.intern(tag)?;
        let carry = HEREDOC | if dash { FLAG } else { 0 } | ((index as u32) << INDEX_SHIFT);
        Some((start, end, LineState(carry)))
    }
}

/// The end of the expansion whose `$` is at `at`, or `at` when there is none.
fn expansion(line: &str, at: usize) -> usize {
    let bytes = line.as_bytes();
    let Some(byte) = bytes.get(at + 1) else {
        return at;
    };
    match byte {
        b'{' => {
            // To the closing brace, or to the end of the line: a `${` that never
            // closes is a broken script, and colouring the rest of the line as
            // the expansion it was meant to be says so more usefully than
            // colouring nothing.
            let mut end = at + 2;
            while end < bytes.len() && bytes[end] != b'}' {
                end += char_step(line, end);
            }
            if end < bytes.len() {
                end + 1
            } else {
                bytes.len()
            }
        }
        // `$(...)` is a command substitution: the caller steps over the `$` so
        // that what is inside is lexed as code.
        b'(' => at,
        // The positional and special parameters, each exactly one byte.
        b'?' | b'!' | b'#' | b'$' | b'@' | b'*' | b'-' | b'0'..=b'9' => at + 2,
        byte if byte.is_ascii_alphabetic() || *byte == b'_' => word_end(bytes, at + 1),
        _ => at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::test_support::lex;

    /// The spans of `line` from a clean state under a fresh highlighter.
    fn spans(line: &str) -> Vec<(&str, Token)> {
        lex(&ShellHighlighter::new(), line, LineState::START).0
    }

    #[test]
    fn the_keyword_table_is_sorted() {
        assert!(KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn every_state_round_trips_inside_the_composable_budget() {
        let shell = ShellHighlighter::new();
        // A quote, both ways round.
        for (line, quote) in [("echo \"one", b'"'), ("echo 'one", b'\'')] {
            let state = lex(&shell, line, LineState::START).1;
            assert_eq!(state.0 >> LineState::COMPOSABLE_BITS, 0);
            assert_eq!(state.0 & 0b11, QUOTE);
            assert_eq!(state.0 & FLAG != 0, quote == b'\'');
        }

        // A heredoc for every slot the index field holds, each a distinct tag.
        // The last one interned still fits the budget, and the one after it is
        // not tracked at all rather than overflowing into the neighbouring
        // half of the state.
        let mut states = Vec::new();
        for slot in 0..TAG_SLOTS {
            let line = format!("cat <<T{slot}");
            let state = lex(&shell, &line, LineState::START).1;
            assert_eq!(
                state.0 >> LineState::COMPOSABLE_BITS,
                0,
                "tag {slot} overflowed the budget"
            );
            assert_eq!(state.0 & 0b11, HEREDOC);
            states.push(state);
        }
        states.sort_unstable_by_key(|state| state.0);
        states.dedup();
        assert_eq!(states.len(), TAG_SLOTS, "two tags shared a state");

        // Full: the next distinct tag is refused rather than colliding.
        assert!(lex(&shell, "cat <<OVERFLOW", LineState::START).1.is_start());
        // And one already interned still works, which is what keeps a document
        // that reuses `EOF` a thousand times from ever reaching the limit.
        assert_eq!(
            lex(&shell, "cat <<T0", LineState::START).1,
            LineState(HEREDOC)
        );
    }

    #[test]
    fn the_same_tag_twice_is_the_same_state() {
        // What the syntax cache stands on: two identical openings have to leave
        // identical states, or an edit above one of them re-lexes to the end of
        // the document.
        let shell = ShellHighlighter::new();
        let first = lex(&shell, "cat <<EOF", LineState::START).1;
        let second = lex(&shell, "tee <<EOF", LineState::START).1;
        assert_eq!(first, second);
        // And `<<-` is a different state from `<<`, since it forgives an
        // indented terminator and the plain one does not.
        assert_ne!(first, lex(&shell, "cat <<-EOF", LineState::START).1);
    }

    #[test]
    fn a_comment_runs_to_the_end_of_the_line() {
        assert_eq!(
            spans("echo hi # and then"),
            [("echo", Token::Keyword), ("# and then", Token::Comment)]
        );
    }

    #[test]
    fn a_hash_inside_a_word_is_not_a_comment() {
        // The two places a `#` is ordinary: mid-word, and inside a `${}`.
        assert!(
            !spans("id=a#b")
                .iter()
                .any(|(_, token)| *token == Token::Comment)
        );
        assert!(
            !spans("echo ${name#prefix}")
                .iter()
                .any(|(_, token)| *token == Token::Comment)
        );
    }

    #[test]
    fn the_two_quotes_behave_differently() {
        // A backslash escapes inside `"` and is a plain byte inside `'`.
        let strings: Vec<_> = spans(r#"echo "a\"b" 'c\'"#)
            .into_iter()
            .filter(|(_, token)| *token == Token::String)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(strings, [r#""a\"b""#, r"'c\'"]);
    }

    #[test]
    fn an_open_quote_carries_to_the_next_line() {
        let shell = ShellHighlighter::new();
        let (_, after) = lex(&shell, "echo \"one", LineState::START);
        assert!(!after.is_start());
        let (found, closed) = lex(&shell, "two\" done", after);
        assert_eq!(found[0], ("two\"", Token::String));
        assert!(closed.is_start());
    }

    #[test]
    fn expansions_come_out_whole() {
        let variables: Vec<_> = spans("cp $src ${dst:-/tmp} $1 $? $HOME/x")
            .into_iter()
            .filter(|(_, token)| *token == Token::Variable)
            .map(|(text, _)| text)
            .collect();
        assert_eq!(variables, ["$src", "${dst:-/tmp}", "$1", "$?", "$HOME"]);
    }

    #[test]
    fn a_command_substitution_is_lexed_as_code() {
        // The point of not swallowing `$(`: the `echo` inside is still a word.
        assert!(
            spans("x=$(echo hi)")
                .iter()
                .any(|(text, token)| *token == Token::Keyword && *text == "echo")
        );
    }

    #[test]
    fn an_unclosed_brace_takes_the_rest_of_the_line() {
        assert_eq!(
            spans("echo ${broken").last(),
            Some(&("${broken", Token::Variable))
        );
    }

    #[test]
    fn an_assignment_names_a_key() {
        assert_eq!(
            spans("PORT=22"),
            [("PORT", Token::Key), ("22", Token::Number)]
        );
    }

    #[test]
    fn a_heredoc_body_is_a_string_until_its_tag() {
        let shell = ShellHighlighter::new();
        let (_, after) = lex(&shell, "cat <<EOF", LineState::START);
        assert!(!after.is_start());

        let (body, still) = lex(&shell, "  anything at all # not a comment", after);
        assert_eq!(body[0].1, Token::String);
        assert_eq!(still, after, "the body does not change the state");

        let (_, closed) = lex(&shell, "EOF", after);
        assert!(closed.is_start());
    }

    #[test]
    fn a_dash_heredoc_forgives_an_indented_terminator() {
        let shell = ShellHighlighter::new();
        let (_, after) = lex(&shell, "cat <<-'END'", LineState::START);
        assert!(lex(&shell, "\t\tEND", after).1.is_start());
        // And a plain one does not.
        let (_, strict) = lex(&shell, "cat <<END", LineState::START);
        assert!(!lex(&shell, "\t\tEND", strict).1.is_start());
    }

    #[test]
    fn a_here_string_and_a_shift_are_not_heredocs() {
        let shell = ShellHighlighter::new();
        assert!(lex(&shell, "cat <<<\"$x\"", LineState::START).1.is_start());
        assert!(
            lex(&shell, "n=$(( 1 << 2 ))", LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn a_tag_too_long_to_carry_is_not_tracked() {
        // Documented behaviour rather than a silent truncation: the body is
        // lexed as shell, which is wrong in colour and in nothing else.
        let shell = ShellHighlighter::new();
        assert!(
            lex(
                &shell,
                "cat <<A_TAG_NOBODY_WOULD_EVER_WRITE",
                LineState::START
            )
            .1
            .is_start()
        );
        assert!(
            lex(
                &shell,
                &format!("cat <<{}", "x".repeat(TAG_LIMIT)),
                LineState::START
            )
            .1
            .0 != 0
        );
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        let shell = ShellHighlighter::new();
        for line in [
            "",
            "\"",
            "'",
            "$",
            "${",
            "<<",
            "<<-",
            "<<''",
            "\\",
            "한글 $변수 \"열린",
            "🙂 <<🙂",
        ] {
            // Including states this highlighter never made: a heredoc index it
            // has not interned draws the line as code rather than panicking.
            for state in [
                LineState::START,
                LineState(QUOTE),
                LineState(QUOTE | FLAG),
                LineState(HEREDOC),
                LineState(0xffff),
            ] {
                lex(&shell, line, state);
            }
        }
    }
}
