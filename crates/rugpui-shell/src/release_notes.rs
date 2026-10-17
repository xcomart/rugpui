//! The "what's new" of a release, read out of its GitHub body.
//!
//! A release body is Markdown, and the update dialog has no Markdown renderer —
//! nor wants one: what the applications publish is a summary paragraph, a list
//! of bullets with a bold lead-in, and the occasional `code` span. So this reads
//! exactly that much into [`Block`]s and leaves everything else as plain words.
//! A construct it does not know degrades to its own text, never to a missing
//! line.
//!
//! GitHub's generated tail — "What's Changed", "New Contributors", "Full
//! Changelog" — is cut off. It lists pull requests and a compare link, which is
//! a maintainer's view of the release; the dialog already offers the release
//! page for anyone who wants it.

use std::ops::Range;

/// One block of release notes, in the order the body gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A `#` heading of any level.
    Heading(Inline),
    /// Running text: consecutive lines joined into one.
    Paragraph(Inline),
    /// A list item, bulleted or numbered, with its continuation lines.
    Item(Inline),
}

/// A run of text with the spans Markdown marked inside it.
///
/// The ranges are byte ranges into [`Inline::text`], always on character
/// boundaries, which is the shape `gpui::StyledText` takes its highlights in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inline {
    /// The words, with the Markdown markers removed.
    pub text: String,
    /// The `**strong**` spans.
    pub strong: Vec<Range<usize>>,
    /// The `` `code` `` spans.
    pub code: Vec<Range<usize>>,
}

/// Reads a release body into blocks, without GitHub's generated tail.
///
/// An empty or whitespace-only body yields no blocks, which is what tells the
/// dialog to show no notes at all.
pub fn parse(body: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    // The block being built, and whether it is a list item: a continuation line
    // joins either kind, so the two are told apart only when it is finished.
    let mut open: Option<(bool, String)> = None;

    let flush = |open: &mut Option<(bool, String)>, blocks: &mut Vec<Block>| {
        if let Some((item, words)) = open.take() {
            let inline = inline(&words);
            blocks.push(if item {
                Block::Item(inline)
            } else {
                Block::Paragraph(inline)
            });
        }
    };

    for line in body.lines() {
        let line = line.trim();
        if is_generated_tail(line) {
            break;
        }
        if line.is_empty() || is_rule(line) || line.starts_with("<!--") {
            flush(&mut open, &mut blocks);
            continue;
        }
        if let Some(words) = heading(line) {
            flush(&mut open, &mut blocks);
            if !words.is_empty() {
                blocks.push(Block::Heading(inline(words)));
            }
            continue;
        }
        if let Some(words) = list_item(line) {
            flush(&mut open, &mut blocks);
            open = Some((true, words.to_owned()));
            continue;
        }
        match &mut open {
            Some((_, words)) => {
                words.push(' ');
                words.push_str(line);
            }
            None => open = Some((false, line.to_owned())),
        }
    }
    flush(&mut open, &mut blocks);
    blocks
}

/// Whether `line` starts the section GitHub generates under the author's notes.
fn is_generated_tail(line: &str) -> bool {
    if line.starts_with("**Full Changelog**") {
        return true;
    }
    heading(line).is_some_and(|words| {
        words.eq_ignore_ascii_case("What's Changed")
            || words.eq_ignore_ascii_case("What’s Changed")
            || words.eq_ignore_ascii_case("New Contributors")
    })
}

/// The words of an ATX heading, `None` when `line` is not one.
fn heading(line: &str) -> Option<&str> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        // `#hashtag`, not a heading.
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim_end())
}

/// The words of a list item, `None` when `line` is not one.
fn list_item(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim_start());
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &line[digits..];
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .map(str::trim_start)
}

/// Whether `line` is a thematic break: three or more of one of `-*_`.
fn is_rule(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3 && matches!(marks[0], '-' | '*' | '_') && marks.iter().all(|&c| c == marks[0])
}

/// Removes the inline markers from `source`, recording where they were.
///
/// Understands `**strong**`, `` `code` ``, `[text](url)` (kept as its text) and
/// backslash escapes. A marker with no partner is left as a literal character:
/// an unmatched `**` in a version range is far likelier than an unterminated
/// strong span.
fn inline(source: &str) -> Inline {
    let mut out = Inline::default();
    let mut strong_from: Option<usize> = None;
    let mut rest = source;

    while let Some(c) = rest.chars().next() {
        if c == '\\'
            && let Some(next) = rest[1..].chars().next()
            && next.is_ascii_punctuation()
        {
            out.text.push(next);
            rest = &rest[1 + next.len_utf8()..];
            continue;
        }
        if c == '`'
            && let Some(end) = rest[1..].find('`')
        {
            let start = out.text.len();
            out.text.push_str(&rest[1..1 + end]);
            out.code.push(start..out.text.len());
            rest = &rest[2 + end..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("**") {
            match strong_from.take() {
                Some(start) => {
                    if start < out.text.len() {
                        out.strong.push(start..out.text.len());
                    }
                }
                None if after.contains("**") => strong_from = Some(out.text.len()),
                None => out.text.push_str("**"),
            }
            rest = after;
            continue;
        }
        if c == '['
            && let Some((words, after)) = link(rest)
        {
            out.text.push_str(words);
            rest = after;
            continue;
        }
        out.text.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Splits `[text](url)` off the front of `source`: its text, and what follows.
fn link(source: &str) -> Option<(&str, &str)> {
    let close = source.find("](")?;
    let words = &source[1..close];
    if words.contains('[') || words.contains(']') {
        return None;
    }
    let target = &source[close + 2..];
    let end = target.find(')')?;
    Some((words, &target[end + 1..]))
}

// A one-span list is exactly what these tests mean, not a mistyped range.
#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Inline {
        Inline {
            text: text.to_owned(),
            ..Inline::default()
        }
    }

    #[test]
    fn a_published_body_keeps_the_summary_and_bullets_and_drops_the_generated_tail() {
        let body = "widget 0.2.0 — faster and safer.\r\n\
            \r\n\
            - **Tabs can be dragged.** A drop bar marks the spot.\r\n\
            - **Saves run under `sh`** whatever the shell.\r\n\
            \r\n\
            \r\n\
            \r\n\
            ## What's Changed\r\n\
            * chore: release 0.2.0 by @someone in https://github.com/o/r/pull/7\r\n\
            \r\n\
            \r\n\
            **Full Changelog**: https://github.com/o/r/compare/v0.1.9...v0.2.0";
        let blocks = parse(body);
        assert_eq!(
            blocks,
            vec![
                Block::Paragraph(plain("widget 0.2.0 — faster and safer.")),
                Block::Item(Inline {
                    text: "Tabs can be dragged. A drop bar marks the spot.".to_owned(),
                    strong: vec![0..20],
                    code: vec![],
                }),
                Block::Item(Inline {
                    text: "Saves run under sh whatever the shell.".to_owned(),
                    strong: vec![0..18],
                    code: vec![16..18],
                }),
            ]
        );
    }

    #[test]
    fn an_empty_or_generated_only_body_has_no_notes() {
        assert!(parse("").is_empty());
        assert!(parse("  \n\n ").is_empty());
        assert!(parse("**Full Changelog**: https://example.com").is_empty());
        assert!(parse("## New Contributors\n* @a made a first contribution").is_empty());
    }

    #[test]
    fn continuation_lines_join_their_block_and_headings_stand_alone() {
        let blocks =
            parse("### Fixes ###\n- one\n  and more\n1. two\nplain\nrunning on\n\n---\nlast");
        assert_eq!(
            blocks,
            vec![
                Block::Heading(plain("Fixes")),
                Block::Item(plain("one and more")),
                Block::Item(plain("two plain running on")),
                Block::Paragraph(plain("last")),
            ]
        );
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert_eq!(
            parse("#42 fixed"),
            vec![Block::Paragraph(plain("#42 fixed"))]
        );
    }

    #[test]
    fn links_keep_their_text_and_escapes_their_character() {
        let blocks = parse(r"See [the guide](https://example.com/guide) for \*details\*.");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(plain("See the guide for *details*."))]
        );
    }

    #[test]
    fn an_unmatched_marker_stays_literal() {
        assert_eq!(inline("2 ** 3"), plain("2 ** 3"));
        assert_eq!(inline("a ` b"), plain("a ` b"));
        assert_eq!(inline("[not a link]"), plain("[not a link]"));
    }

    #[test]
    fn spans_land_on_character_boundaries_in_non_ascii_text() {
        let line = inline("**변경** 내역 `코드`");
        assert_eq!(line.text, "변경 내역 코드");
        for range in line.strong.iter().chain(&line.code) {
            assert!(line.text.is_char_boundary(range.start));
            assert!(line.text.is_char_boundary(range.end));
        }
        assert_eq!(&line.text[line.strong[0].clone()], "변경");
        assert_eq!(&line.text[line.code[0].clone()], "코드");
    }
}
