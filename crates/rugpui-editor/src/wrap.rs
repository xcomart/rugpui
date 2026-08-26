//! Where a line breaks when word wrap is on, and the row arithmetic that
//! follows from it.
//!
//! # Lines and rows
//!
//! With wrapping off the two words mean the same thing and every function here
//! is the identity. With it on a line takes as many *rows* as the text area's
//! width forces it to, and everything the editor measures in rows — the scroll
//! extent, which line the viewport starts at, where a caret sits, what `Up`
//! moves to — has to go through this map to get there.
//!
//! # What is stored
//!
//! Per line, the byte offsets at which each row after the first begins,
//! relative to the start of the line. That is all: the row *count* is the
//! length of that list plus one, which is what the prefix sums are built from,
//! and the offsets themselves are what a caret and a selection are cut on.
//!
//! # What measures it
//!
//! Not this module. Breaking a line needs a shaped line, which needs a text
//! system, which only exists inside a frame — so
//! [`crate::element::EditorElement`]'s prepaint measures, and every line it
//! measures it measures once: [`WrapMap::edited`] blanks the lines an edit
//! touched and leaves the rest alone, so typing a character into a ten thousand
//! line buffer re-breaks one line. [`WrapMap::measures`] counts the work, which
//! is how the tests hold that down.

use std::cell::{Cell, RefCell};
use std::ops::Range;

use gpui::{Font, Pixels};

/// The offsets, relative to the start of a line, where its rows after the first
/// begin.
type Breaks = Box<[u32]>;

/// What the rows were measured against. Any change to it invalidates all of
/// them, because all of them would break somewhere else.
#[derive(Clone, PartialEq)]
struct Measure {
    /// The width a row is broken at.
    width: Pixels,
    /// The size the text is shaped at.
    size: Pixels,
    /// The family it is shaped in.
    font: Font,
}

/// How each line of a buffer breaks into rows.
#[derive(Default)]
pub struct WrapMap {
    /// Whether wrapping is on at all. Everything below is empty when it is not.
    on: bool,
    /// What the measured lines were measured against, once anything has been.
    measure: Option<Measure>,
    /// `lines[i]` is where line `i` breaks, or [`None`] when it has not been
    /// measured since it last changed.
    lines: Vec<Option<Breaks>>,
    /// `prefix[i]` is how many rows sit above line `i`, so `prefix.len()` is
    /// `lines.len() + 1`.
    ///
    /// A [`RefCell`] because it is derived rather than held: every read of a
    /// row wants it and every write of a line invalidates it, and the reads
    /// come from an element's prepaint, which holds the view by shared
    /// reference. The same trade [`crate::highlight::SyntaxCache`] makes for
    /// its call counter.
    prefix: RefCell<Vec<u32>>,
    /// Whether `prefix` still answers for `lines`.
    stale: Cell<bool>,
    /// Whether any line is unmeasured.
    pending: bool,
    /// How many lines have been measured through this map, ever.
    measures: Cell<usize>,
}

impl WrapMap {
    /// A map with wrapping off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether lines are wrapped.
    pub(crate) const fn is_on(&self) -> bool {
        self.on
    }

    /// Turns wrapping on or off, discarding everything measured.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        self.measure = None;
        self.lines.clear();
        self.pending = on;
        self.stale.set(true);
    }

    /// Every line has to be measured again — a new font, a new width, a new
    /// document.
    pub fn invalidate(&mut self) {
        if !self.on {
            return;
        }
        self.measure = None;
        self.lines.fill(None);
        self.pending = true;
        self.stale.set(true);
    }

    /// Brings the map back into step after the buffer changed.
    ///
    /// The arguments are [`crate::highlight::SyntaxCache::edited`]'s: `first`
    /// is the first line the edit touched, `added` the number of lines the
    /// replacement spans and `removed` the number the replaced text spanned.
    pub fn edited(&mut self, first: usize, removed: usize, added: usize) {
        if !self.on {
            return;
        }
        if removed != added {
            let at = (first + 1).min(self.lines.len());
            let old_end = (at + removed).min(self.lines.len());
            self.lines
                .splice(at..old_end, std::iter::repeat_n(None, added));
        }
        // Every line the replacement covers breaks somewhere new, whatever it
        // broke at before.
        for line in self.lines.iter_mut().skip(first).take(added + 1) {
            *line = None;
        }
        self.pending = true;
        self.stale.set(true);
    }

    /// Opens a measuring pass, and says whether one is needed.
    ///
    /// A width, size or family that is not what the rows were measured against
    /// throws all of them away first.
    pub fn begin(&mut self, width: Pixels, size: Pixels, font: &Font, line_count: usize) -> bool {
        if !self.on {
            return false;
        }
        let measure = Measure {
            width,
            size,
            font: font.clone(),
        };
        if self.measure.as_ref() != Some(&measure) {
            self.invalidate();
            self.measure = Some(measure);
        }
        if self.lines.len() != line_count {
            self.lines.resize_with(line_count, || None);
            self.pending = true;
            self.stale.set(true);
        }
        self.pending
    }

    /// Whether `line` has to be measured.
    pub fn unmeasured(&self, line: usize) -> bool {
        self.on && self.lines.get(line).is_none_or(Option::is_none)
    }

    /// Records where `line` breaks.
    pub fn measured(&mut self, line: usize, breaks: Vec<u32>) {
        self.measures.set(self.measures.get() + 1);
        if let Some(slot) = self.lines.get_mut(line) {
            *slot = Some(breaks.into_boxed_slice());
            self.stale.set(true);
        }
    }

    /// Closes a measuring pass: everything that was asked for has been given.
    pub fn finish(&mut self) {
        self.pending = false;
    }

    /// Where `line` breaks, relative to its start.
    pub fn breaks(&self, line: usize) -> &[u32] {
        match self.lines.get(line) {
            Some(Some(breaks)) => breaks,
            _ => &[],
        }
    }

    /// How many rows `line` takes. At least one, always.
    pub fn rows_in(&self, line: usize) -> usize {
        self.breaks(line).len() + 1
    }

    /// The byte range of row `sub` of `line`, relative to the start of the
    /// line, where `len` is the length of the line in bytes.
    pub fn row_range(&self, line: usize, sub: usize, len: usize) -> Range<usize> {
        let breaks = self.breaks(line);
        let start = match sub.checked_sub(1) {
            None => 0,
            Some(before) => breaks.get(before).map_or(len, |at| *at as usize),
        };
        // Clamped to the line rather than trusted: a line can be edited and
        // read again before the pass that re-measures it has run.
        let start = start.min(len);
        start
            ..breaks
                .get(sub)
                .map_or(len, |at| *at as usize)
                .clamp(start, len)
    }

    /// Which row of `line` byte offset `column` falls on, `column` counted from
    /// the start of the line.
    ///
    /// A caret exactly on a break belongs to the row the break opens, which is
    /// where it is drawn: at the head of the next row rather than past the
    /// right edge of the one before.
    pub fn row_of_column(&self, line: usize, column: usize) -> usize {
        self.breaks(line)
            .partition_point(|at| (*at as usize) <= column)
    }

    /// The row `line` starts on, counting from the top of the buffer.
    pub fn first_row(&self, line: usize) -> usize {
        if !self.on {
            return line;
        }
        let prefix = self.sums();
        let prefix = prefix.borrow();
        match prefix.get(line) {
            Some(rows) => *rows as usize,
            // Past what has been measured: every line beyond it is one row
            // until a pass says otherwise.
            None => prefix.last().map_or(line, |rows| {
                *rows as usize + line.saturating_sub(self.lines.len())
            }),
        }
    }

    /// The line row `row` falls on, and which of that line's rows it is.
    pub fn row_at(&self, row: usize) -> (usize, usize) {
        if !self.on {
            return (row, 0);
        }
        let prefix = self.sums();
        let prefix = prefix.borrow();
        let Some(total) = prefix.last().map(|rows| *rows as usize) else {
            return (row, 0);
        };
        if row >= total {
            // Past the measured lines, as `first_row` is.
            return (self.lines.len() + (row - total), 0);
        }
        let line = prefix.partition_point(|rows| (*rows as usize) <= row) - 1;
        (line, row - prefix[line] as usize)
    }

    /// How many rows the whole buffer takes.
    pub fn total_rows(&self, line_count: usize) -> usize {
        if !self.on {
            return line_count;
        }
        self.first_row(line_count)
    }

    /// How many lines have been measured through this map.
    ///
    /// For tests and for profiling; only differences between two reads of it
    /// mean anything.
    pub fn measures(&self) -> usize {
        self.measures.get()
    }

    /// The prefix sums, rebuilt if a line has changed since they were last
    /// asked for.
    fn sums(&self) -> &RefCell<Vec<u32>> {
        if self.stale.replace(false) {
            let mut prefix = self.prefix.borrow_mut();
            prefix.clear();
            prefix.reserve(self.lines.len() + 1);
            let mut rows = 0;
            prefix.push(0);
            for line in &self.lines {
                rows += line.as_ref().map_or(1, |breaks| breaks.len() as u32 + 1);
                prefix.push(rows);
            }
        }
        &self.prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    /// A map over `rows` lines, each broken into as many rows as its entry
    /// says, at made-up offsets a hundred bytes apart.
    fn map(rows: &[usize]) -> WrapMap {
        let mut map = WrapMap::new();
        map.set_on(true);
        assert!(map.begin(px(100.), px(12.), &gpui::font("Test"), rows.len()));
        for (line, count) in rows.iter().enumerate() {
            let breaks = (1..*count).map(|row| (row * 100) as u32).collect();
            map.measured(line, breaks);
        }
        map.finish();
        map
    }

    #[test]
    fn wrapping_off_is_the_identity() {
        let map = WrapMap::new();
        assert_eq!(map.rows_in(7), 1);
        assert_eq!(map.first_row(7), 7);
        assert_eq!(map.row_at(7), (7, 0));
        assert_eq!(map.total_rows(9), 9);
    }

    #[test]
    fn a_row_is_a_line_when_nothing_wraps() {
        let map = map(&[1, 1, 1]);
        assert_eq!(map.first_row(2), 2);
        assert_eq!(map.row_at(2), (2, 0));
        assert_eq!(map.total_rows(3), 3);
    }

    #[test]
    fn the_prefix_sums_place_every_row() {
        let map = map(&[2, 1, 3, 1]);
        assert_eq!(
            (0..4).map(|line| map.first_row(line)).collect::<Vec<_>>(),
            vec![0, 2, 3, 6]
        );
        assert_eq!(map.total_rows(4), 7);
        assert_eq!(
            (0..7).map(|row| map.row_at(row)).collect::<Vec<_>>(),
            vec![(0, 0), (0, 1), (1, 0), (2, 0), (2, 1), (2, 2), (3, 0)]
        );
    }

    #[test]
    fn a_row_past_the_end_stays_past_the_end() {
        let map = map(&[2, 1]);
        assert_eq!(map.row_at(3), (2, 0));
        assert_eq!(map.row_at(4), (3, 0));
        assert_eq!(map.first_row(3), 4);
    }

    #[test]
    fn a_row_carries_the_byte_range_it_covers() {
        let map = map(&[3]);
        assert_eq!(map.row_range(0, 0, 250), 0..100);
        assert_eq!(map.row_range(0, 1, 250), 100..200);
        assert_eq!(map.row_range(0, 2, 250), 200..250);
        // A line shorter than its breaks say cannot hand out a range past its
        // end, whatever order the measuring and the edit arrived in.
        assert_eq!(map.row_range(0, 2, 150), 150..150);
    }

    #[test]
    fn a_column_on_a_break_belongs_to_the_row_it_opens() {
        let map = map(&[3]);
        assert_eq!(map.row_of_column(0, 0), 0);
        assert_eq!(map.row_of_column(0, 99), 0);
        assert_eq!(map.row_of_column(0, 100), 1);
        assert_eq!(map.row_of_column(0, 200), 2);
        assert_eq!(map.row_of_column(0, 999), 2);
    }

    #[test]
    fn an_edit_blanks_the_lines_it_touched_and_no_others() {
        let mut map = map(&[2, 2, 2]);
        let measured = map.measures();
        // Typing inside line 1: one line to measure again.
        map.edited(1, 0, 0);
        assert!(!map.unmeasured(0));
        assert!(map.unmeasured(1));
        assert!(!map.unmeasured(2));
        // Until it is, the line stands at one row, and the sums move with it.
        assert_eq!(map.total_rows(3), 5);
        assert!(map.begin(px(100.), px(12.), &gpui::font("Test"), 3));
        map.measured(1, vec![100]);
        map.finish();
        assert_eq!(map.total_rows(3), 6);
        assert_eq!(map.measures() - measured, 1);
    }

    #[test]
    fn a_split_line_leaves_the_lines_under_it_alone() {
        let mut map = map(&[2, 2, 2]);
        // A newline in line 0: it and the line it made are unmeasured, the two
        // that were below it are not.
        map.edited(0, 0, 1);
        assert!(map.begin(px(100.), px(12.), &gpui::font("Test"), 4));
        assert!(map.unmeasured(0));
        assert!(map.unmeasured(1));
        assert!(!map.unmeasured(2));
        assert!(!map.unmeasured(3));
        assert_eq!(map.rows_in(2), 2);
        assert_eq!(map.rows_in(3), 2);
    }

    #[test]
    fn a_joined_line_takes_the_rows_of_the_one_below_away() {
        let mut map = map(&[2, 2, 2]);
        // Backspace at the head of line 1, which pulls line 2 up into it.
        map.edited(0, 1, 0);
        assert!(map.begin(px(100.), px(12.), &gpui::font("Test"), 2));
        assert!(map.unmeasured(0));
        assert!(!map.unmeasured(1));
        assert_eq!(map.rows_in(1), 2);
        assert_eq!(map.total_rows(2), 3);
    }

    #[test]
    fn a_new_width_measures_everything_again() {
        let mut map = map(&[2, 2]);
        assert!(!map.begin(px(100.), px(12.), &gpui::font("Test"), 2));
        assert!(map.begin(px(120.), px(12.), &gpui::font("Test"), 2));
        assert!(map.unmeasured(0));
        assert!(map.unmeasured(1));
    }

    #[test]
    fn a_new_font_measures_everything_again() {
        let mut map = map(&[2, 2]);
        assert!(map.begin(px(100.), px(12.), &gpui::font("Other"), 2));
        assert!(map.unmeasured(1));
    }
}
