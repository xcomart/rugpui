//! Tab stops: how wide a tab character is drawn.
//!
//! gpui's shaper has no notion of a tab. Handed a line with one in it, it draws
//! whatever the font has at U+0009 — nothing at all in most faces, or one
//! space's width — and the text after it lands wherever that happens to put
//! it. So a line is never handed over as it is: every tab in it is first
//! *expanded*, replaced by as many spaces as it takes to reach the next tab
//! stop, and the shaper only ever sees the result.
//!
//! The buffer is never touched. The expansion exists between reading a line
//! and drawing it, and [`TabMap`] is what carries an offset across it: a caret,
//! a selection edge or a wrap break is a byte offset into the *line*, and the
//! shaped text it is measured against is a different string as soon as the
//! line holds a tab. A line without one — nearly every line of nearly every
//! document — has an empty map and costs nothing.
//!
//! # Columns
//!
//! Tab stops are counted in the cells a monospaced font draws a character
//! across: one for a Latin letter, two for a Hangul syllable or a CJK
//! ideograph, none for a combining mark. That is what keeps a tab after a
//! Korean comment lined up with one after an English comment on the line
//! below, which is the whole reason anybody types a tab between two columns of
//! text. The same count is what [`crate::EditorView`]'s vertical moves aim
//! for, so a caret walked down through a tab lands under where it was.

use std::borrow::Cow;

use gpui::TextRun;
use unicode_width::UnicodeWidthChar;

/// How many columns apart the tab stops are when the host says nothing.
pub const DEFAULT_TAB_WIDTH: usize = 4;

/// The widest tab stop an editor accepts.
///
/// Not a taste judgement: a value read from a hand-edited settings file can
/// be anything, and a tab a thousand columns wide is a line the shaper has to
/// lay out a thousand spaces of.
pub const MAX_TAB_WIDTH: usize = 16;

/// `width` held to the range an editor accepts.
pub(crate) fn clamp_width(width: usize) -> usize {
    width.clamp(1, MAX_TAB_WIDTH)
}

/// How many columns `c` takes, when it is not a tab.
pub(crate) fn char_columns(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// How many columns a tab that starts at `column` spans.
pub(crate) const fn tab_columns(column: usize, width: usize) -> usize {
    width - column % width
}

/// The column `text` ends at, starting from `column`.
pub(crate) fn columns_after(text: &str, column: usize, width: usize) -> usize {
    text.chars().fold(column, |column, c| {
        column
            + if c == '\t' {
                tab_columns(column, width)
            } else {
                char_columns(c)
            }
    })
}

/// Which way an offset inside a tab's spaces goes when it is carried back to
/// the line, where a tab is one byte with nothing inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Snap {
    /// To whichever edge of the tab is nearer, ties to the start. A click.
    Nearest,
    /// To the end of the tab. A wrap break, which the shaper may take partway
    /// through a run of spaces, and which has to leave the tab on the row the
    /// tab starts on: the row after it is where the next character goes.
    After,
}

/// Where a line's tabs are, and how wide each was drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TabMap {
    /// Each tab, in order: its byte offset into the line, and how many spaces
    /// it was expanded to.
    tabs: Vec<(usize, usize)>,
}

impl TabMap {
    /// `text` as it is drawn, with tab stops every `width` columns, and the
    /// map that carries offsets between the two.
    ///
    /// Borrowed, with an empty map, when there is no tab to expand.
    pub(crate) fn expand(text: &str, width: usize) -> (Cow<'_, str>, Self) {
        if !text.contains('\t') {
            return (Cow::Borrowed(text), Self::default());
        }
        let width = clamp_width(width);
        let mut drawn = String::with_capacity(text.len() + width);
        let mut tabs = Vec::new();
        let mut column = 0;
        for (at, c) in text.char_indices() {
            if c == '\t' {
                let span = tab_columns(column, width);
                tabs.push((at, span));
                drawn.extend(std::iter::repeat_n(' ', span));
                column += span;
            } else {
                drawn.push(c);
                column += char_columns(c);
            }
        }
        (Cow::Owned(drawn), Self { tabs })
    }

    /// Where byte `column` of the line is in the drawn text.
    pub(crate) fn to_drawn(&self, column: usize) -> usize {
        let grown: usize = self
            .tabs
            .iter()
            .take_while(|(at, _)| *at < column)
            .map(|(_, span)| span - 1)
            .sum();
        column + grown
    }

    /// Where byte `drawn` of the drawn text is in the line.
    ///
    /// An offset strictly inside a tab's spaces has no counterpart, and goes
    /// to an edge of the tab as `snap` says.
    pub(crate) fn to_line(&self, drawn: usize, snap: Snap) -> usize {
        let mut grown = 0;
        for &(at, span) in &self.tabs {
            let start = at + grown;
            if drawn <= start {
                break;
            }
            if drawn < start + span {
                return match snap {
                    Snap::Nearest if (drawn - start) * 2 <= span => at,
                    Snap::Nearest | Snap::After => at + 1,
                };
            }
            grown += span - 1;
        }
        drawn - grown
    }

    /// Breaks taken in the drawn text, as offsets into the line.
    pub(crate) fn breaks(&self, drawn: &[u32]) -> Vec<u32> {
        if self.tabs.is_empty() {
            return drawn.to_vec();
        }
        drawn
            .iter()
            .map(|at| self.to_line(*at as usize, Snap::After) as u32)
            .collect()
    }

    /// Runs that tile the line, stretched to tile the drawn text: a run with a
    /// tab in it is longer by the spaces the tab became.
    pub(crate) fn widen(&self, runs: Vec<TextRun>) -> Vec<TextRun> {
        if self.tabs.is_empty() {
            return runs;
        }
        let mut at = 0;
        runs.into_iter()
            .map(|run| {
                let end = at + run.len;
                let len = self.to_drawn(end) - self.to_drawn(at);
                at = end;
                TextRun { len, ..run }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{black, font};

    use super::*;

    fn run(len: usize) -> TextRun {
        crate::highlight::plain_run(len, black(), &font("Consolas"))
    }

    #[test]
    fn a_line_without_a_tab_is_drawn_as_it_is() {
        let (drawn, map) = TabMap::expand("select 1", 4);
        assert!(matches!(drawn, Cow::Borrowed("select 1")));
        assert_eq!(map, TabMap::default());
        assert_eq!(map.to_drawn(5), 5);
        assert_eq!(map.to_line(5, Snap::Nearest), 5);
    }

    #[test]
    fn a_tab_reaches_the_next_stop() {
        assert_eq!(TabMap::expand("\tx", 4).0, "    x");
        assert_eq!(TabMap::expand("ab\tx", 4).0, "ab  x");
        assert_eq!(TabMap::expand("abcd\tx", 4).0, "abcd    x");
        assert_eq!(TabMap::expand("a\t\tx", 4).0, "a       x");
        assert_eq!(TabMap::expand("ab\tx", 8).0, "ab      x");
        assert_eq!(TabMap::expand("ab\tx", 2).0, "ab  x");
    }

    #[test]
    fn a_wide_character_counts_the_cells_it_takes() {
        // "이름" is four cells, so the tab after it fills the stop out with
        // the same spaces "name" would leave it.
        assert_eq!(TabMap::expand("이름\tx", 8).0, "이름    x");
        assert_eq!(TabMap::expand("name\tx", 8).0, "name    x");
    }

    #[test]
    fn an_impossible_width_is_held_in_range() {
        assert_eq!(TabMap::expand("\tx", 0).0, " x");
        assert_eq!(
            TabMap::expand("\tx", 1000).0.len(),
            MAX_TAB_WIDTH + 1,
            "a tab no wider than the widest stop"
        );
    }

    #[test]
    fn offsets_cross_the_expansion_both_ways() {
        // "a\tbc\td": the first tab spans drawn 1..4, the second 6..8.
        let (drawn, map) = TabMap::expand("a\tbc\td", 4);
        assert_eq!(drawn, "a   bc  d");
        for (line, at) in [(0, 0), (1, 1), (2, 4), (3, 5), (4, 6), (5, 8), (6, 9)] {
            assert_eq!(map.to_drawn(line), at, "line offset {line}");
            assert_eq!(map.to_line(at, Snap::Nearest), line, "drawn offset {at}");
            assert_eq!(map.to_line(at, Snap::After), line, "drawn offset {at}");
        }
    }

    #[test]
    fn inside_a_tab_snaps_to_an_edge() {
        let (_, map) = TabMap::expand("a\tb", 4);
        // The tab is drawn 1..4: three spaces.
        assert_eq!(map.to_line(2, Snap::Nearest), 1, "nearer the start");
        assert_eq!(map.to_line(3, Snap::Nearest), 2, "nearer the end");
        assert_eq!(map.to_line(2, Snap::After), 2);
        // A break partway through the spaces and one just after them both
        // open the row on the `b`.
        assert_eq!(map.breaks(&[3]), [2]);
        assert_eq!(map.breaks(&[4]), [2]);
    }

    #[test]
    fn a_run_over_a_tab_grows_by_its_spaces() {
        // "ab" + "\tc" + "d": the tab becomes two spaces.
        let (drawn, map) = TabMap::expand("ab\tcd", 4);
        let lens: Vec<usize> = map
            .widen(vec![run(2), run(2), run(1)])
            .iter()
            .map(|run| run.len)
            .collect();
        assert_eq!(lens, [2, 3, 1]);
        assert_eq!(lens.iter().sum::<usize>(), drawn.len());
    }

    #[test]
    fn columns_count_tabs_to_the_next_stop() {
        assert_eq!(columns_after("ab\t", 0, 4), 4);
        assert_eq!(columns_after("\t", 3, 4), 4);
        assert_eq!(columns_after("한\t", 0, 4), 4);
        assert_eq!(columns_after("abc", 2, 4), 5);
    }
}
