//! Layout.

use super::*;

/// How wide `text` is once the text system has really shaped it.
///
/// The reason fitting a column is worth doing at all. A width guessed from
/// character cells is out by however far the font's advances differ from the
/// guess, which on the proportional face an app actually ships is enough to
/// truncate the value the user was trying to read — and a fit that truncates is
/// the fault it was meant to cure.
pub(super) fn shaped_width(text: &str, font: &Font, size: Pixels, window: &Window) -> f32 {
    // `shape_line` shapes one line and *panics* on a newline, which a value out
    // of a database is entirely entitled to contain. A space is near enough what
    // a one-line cell draws it as, and one byte for one byte keeps the run's
    // length right.
    let text = if text.contains(['\n', '\r']) {
        SharedString::from(text.replace(['\n', '\r'], " "))
    } else {
        SharedString::from(text.to_string())
    };
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        // Shaping, not painting: a colour cannot move a glyph's advance, so any
        // of them measures the same.
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text, size, &[run], None)
        .width()
        .into()
}

/// `base` moved by `step`, kept inside `0..=last`.
pub(super) fn offset(base: usize, step: isize, last: usize) -> usize {
    let moved = base as isize + step;
    moved.clamp(0, last as isize) as usize
}

impl<S: GridSource> GridView<S> {
    /// Whether `column` is hidden.
    pub fn is_column_hidden(&self, column: usize) -> bool {
        self.columns.get(column).is_some_and(|state| state.hidden)
    }

    /// How many columns are hidden.
    ///
    /// What tells a host's menu whether "show every column" is worth offering:
    /// zero means there is nothing to show.
    pub fn hidden_column_count(&self) -> usize {
        self.columns.iter().filter(|state| state.hidden).count()
    }

    /// The name of source column `column`, or `None` when there is no such
    /// column.
    ///
    /// The grid draws this in the heading; a host menu labels its items with it
    /// — "hide *ORDER_ID*" — and copies it.
    pub fn column_name(&self, column: usize) -> Option<&str> {
        (column < self.source.column_count()).then(|| self.source.column(column).name)
    }

    /// Hides or shows `column`.
    ///
    /// Clears the selection: display positions are what a selection is written
    /// in, and hiding a column renumbers every one after it (see
    /// [`crate::selection`]).
    pub fn set_column_hidden(&mut self, column: usize, hidden: bool, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(state) = self.columns.get_mut(column) else {
            return;
        };
        if state.hidden == hidden {
            return;
        }
        state.hidden = hidden;
        self.relayout();
        self.clamp_h_offset();
        self.selection.clear();
        cx.notify();
    }

    /// Un-hides every column.
    ///
    /// The way back from [`GridView::set_column_hidden`], and the one thing a
    /// header menu needs that no other gesture offers: a hidden column has no
    /// heading to right click. Clears the selection for the same reason hiding
    /// one does — every display position after the first restored column moves.
    pub fn show_all_columns(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        if self.hidden_column_count() == 0 {
            return;
        }
        for state in &mut self.columns {
            state.hidden = false;
        }
        self.relayout();
        self.clamp_h_offset();
        self.selection.clear();
        cx.notify();
    }

    /// Widens or narrows `column` to fit what is in it.
    ///
    /// What a double click on the resize grip does, and what a host menu's "fit
    /// this column" calls. An explicit request, so it takes a column back off
    /// the user as well: they are the one asking.
    ///
    /// Only the first few hundred rows are looked at — see `AUTOFIT_SAMPLE` —
    /// and the fit lands on the *next* draw rather than this call, because
    /// measuring text needs a [`Window`] and this signature has none.
    pub fn autofit_column(&mut self, column: usize, cx: &mut Context<Self>) {
        self.ensure_layout();
        let Some(state) = self.columns.get_mut(column) else {
            return;
        };
        state.fit = true;
        cx.notify();
    }

    /// The same for every column at once, for a host menu's "fit all columns".
    ///
    /// Explicit, and therefore allowed to take back a width the user dragged —
    /// which is the whole difference between this and the fitting the grid does
    /// of its own accord.
    pub fn autofit_all_columns(&mut self, cx: &mut Context<Self>) {
        self.ensure_layout();
        for state in &mut self.columns {
            state.fit = true;
        }
        cx.notify();
    }

    /// Walks the sort of `column` on one step: ascending, descending, none.
    ///
    /// What a header click does. Raises [`GridEvent::SortRequested`] and moves
    /// the marker; the rows do not move until the host re-runs the query.
    pub fn toggle_sort(&mut self, column: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        let direction = match self.sort {
            Some((sorted, SortDirection::Ascending)) if sorted == column => {
                Some(SortDirection::Descending)
            }
            Some((sorted, SortDirection::Descending)) if sorted == column => None,
            _ => Some(SortDirection::Ascending),
        };
        self.sort = direction.map(|direction| (column, direction));
        cx.emit(GridEvent::SortRequested { column, direction });
        cx.notify();
    }

    /// Puts the marker where the host says the result is really ordered,
    /// without asking for anything.
    ///
    /// For a host that ordered the query itself — a table opened with a default
    /// `ORDER BY`, say — so that the header agrees with the rows.
    pub fn set_sort(&mut self, sort: Option<(usize, SortDirection)>, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.sort = sort;
        cx.notify();
    }

    /// Picks the cell at `row` and display position `column`, dropping whatever
    /// was picked.
    pub fn select_cell(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(cell) = self.clamped(row, column) else {
            return;
        };
        self.selection.replace(cell);
        self.reveal(cell);
        cx.notify();
    }

    // TODO(M3): a filter row under the header, and pinned columns. The filter
    // row is one more fixed band drawn like the header; a pinned column is one
    // drawn in the gutter's strip instead of the scrolling one, which is why
    // `ColumnState` is indexed by source column and the strip's offset is a
    // single field.

    /// Rebuilds the column list when the source has a different number of them
    /// than the grid last saw.
    ///
    /// The whole of "the host replaced the result": widths, hidden flags and the
    /// selection are all keyed to a shape that no longer holds.
    pub(super) fn ensure_layout(&mut self) {
        let count = self.source.column_count();
        if self.columns.len() == count {
            self.selection
                .clamp(self.source.row_count(), self.laid_out.len());
            return;
        }

        self.columns = vec![
            ColumnState {
                width: DEFAULT_COLUMN_WIDTH,
                hidden: false,
                sizing: Sizing::Default,
                fit: false,
            };
            count
        ];
        // A different number of columns is a different result, whatever route
        // it arrived by, so the fitting starts over with it.
        self.fitted_at = None;
        self.selection.clear();
        self.h_offset = 0.;
        self.relayout();
    }

    /// Which columns are to be measured this frame, and whether each may only
    /// widen.
    ///
    /// Three things put a column in the list, and they are not the same thing.
    /// An **explicit** request — the grip's double click,
    /// [`GridView::autofit_column`], [`GridView::autofit_all_columns`] — puts
    /// any column in it, a width the user dragged included, and lets that width
    /// shrink: somebody asked. The **first** batch of a result puts every column
    /// that is not the user's in it. **Every batch after that** puts the fitted
    /// ones in it to widen only, and only while the sample is still filling up:
    /// a column that narrowed as page three landed would slide the whole table
    /// sideways under a pointer that is reading it, and that jitter is a worse
    /// fault than a column a few pixels wider than it needs to be.
    pub(super) fn fit_plan(&self) -> Vec<(usize, bool)> {
        let rows = self.source.row_count();
        // Nothing at all until there are rows: an empty result says nothing
        // about how wide its values are, so fitting to one would only throw the
        // default away and have to be done again when the rows arrive.
        let automatic = match self.fitted_at {
            _ if !self.autofit || rows == 0 => None,
            None => Some(false),
            Some(seen) if seen < AUTOFIT_SAMPLE && rows > seen => Some(true),
            Some(_) => None,
        };

        self.columns
            .iter()
            .enumerate()
            .filter_map(|(column, state)| {
                if state.fit {
                    return Some((column, false));
                }
                let grow_only = automatic?;
                match state.sizing {
                    Sizing::User => None,
                    Sizing::Default | Sizing::Fitted => Some((column, grow_only)),
                }
            })
            .collect()
    }

    /// Measures the columns [`GridView::fit_plan`] names and writes the widths
    /// back, answering whether any of them moved.
    ///
    /// Called from `render`, and before anything is built, for two reasons.
    /// Shaping text needs a [`Window`], which only a draw has; and deciding the
    /// widths before the header and the rows are laid out means the frame that
    /// fits a column is the frame that draws it fitted — no flash of the default
    /// width, no extra frame.
    pub(super) fn fit_columns(&mut self, window: &Window) -> bool {
        let plan = self.fit_plan();
        for state in &mut self.columns {
            state.fit = false;
        }
        let sample = self.source.row_count().min(AUTOFIT_SAMPLE);
        if self.autofit && sample > 0 {
            // Remembered whether or not anything was in the plan: the sample has
            // been seen either way, and what the next batch is judged against is
            // how much of it had arrived, not how many columns it moved.
            self.fitted_at = Some(sample);
        }
        if plan.is_empty() {
            return false;
        }

        // The font the cells are actually drawn in. The grid sets its own text
        // *size* and inherits everything else from whatever it was put inside,
        // so the family has to be read back off the window rather than assumed.
        let font = window.text_style().font();
        let widths: Vec<(usize, f32, bool)> = plan
            .into_iter()
            .map(|(column, grow_only)| {
                (
                    column,
                    self.fitted_width(column, sample, &font, window),
                    grow_only,
                )
            })
            .collect();

        let mut moved = false;
        for (column, width, grow_only) in widths {
            let state = &mut self.columns[column];
            let width = if grow_only {
                width.max(state.width)
            } else {
                width
            };
            state.sizing = Sizing::Fitted;
            if (width - state.width).abs() < 0.5 {
                continue;
            }
            state.width = width;
            moved = true;
        }
        moved
    }

    /// How wide `column` has to be for its content to be read, measured rather
    /// than guessed.
    pub(super) fn fitted_width(
        &self,
        column: usize,
        rows: usize,
        font: &Font,
        window: &Window,
    ) -> f32 {
        // The heading, with room for the sort marker whether or not it is
        // showing: a column that jumped wider the moment it was ordered by would
        // shove every column after it along, and the user has just clicked on
        // one of them.
        let name = self.source.column(column).name;
        let mut widest = shaped_width(name, font, px(TEXT_SIZE), window)
            + HEADING_GAP
            + shaped_width(SORT_ASCENDING, font, px(SORT_MARKER_SIZE), window);

        for row in self.candidate_rows(column, rows) {
            let label = cell_label(&self.source.cell(row, column));
            widest = widest.max(shaped_width(&label.text, font, px(TEXT_SIZE), window));
        }

        // Rounded up, and up rather than to the nearest, for the same reason the
        // border is counted at all: gpui rounds a width to whole device pixels,
        // and rounding a fit *down* is how a value ends up a fraction of a pixel
        // short of the box it was measured for.
        (widest + CELL_PADDING * 2. + CELL_BORDER)
            .ceil()
            .clamp(MIN_COLUMN_WIDTH, MAX_AUTOFIT_WIDTH)
    }

    /// The sampled rows whose value in `column` is worth shaping: everything
    /// within [`FIT_SPREAD`] cells of the widest, at most [`FIT_CANDIDATES`] of
    /// them.
    ///
    /// A *band*, not a top three, and the difference is the whole point.
    /// Character cells and shaped pixels are different units on a proportional
    /// font — `Pinewood Hardware` and `Northwind Traders` are both seventeen
    /// cells and the first is visibly the wider — so the cheap count cannot be
    /// asked which value wins. It can only be asked which values are still in
    /// the running, and the exact answer decides between them. Taking the top
    /// few by count instead loses every tie, and a tie is exactly the case a
    /// column of names is made of.
    ///
    /// One pass over the sample, shaping nothing and allocating nothing per row.
    /// The leader only ever moves up, so a value that has fallen out of the band
    /// is out of it for good and can be dropped as soon as that is noticed.
    pub(super) fn candidate_rows(&self, column: usize, rows: usize) -> Vec<usize> {
        let mut best: Vec<(usize, usize)> = Vec::with_capacity(FIT_CANDIDATES + 1);
        let mut widest = 0;
        for row in 0..rows {
            let cells = match self.source.cell(row, column) {
                GridCell::Null => NULL_TEXT.width(),
                GridCell::Default => DEFAULT_TEXT.width(),
                GridCell::Text(text) => text.width(),
                GridCell::Lob { size } => lob_label(size).width(),
            };
            widest = widest.max(cells);
            let floor = widest.saturating_sub(FIT_SPREAD);
            // `best` is sorted widest first, so the ones the leader has just
            // left behind are all at the end of it.
            while best.last().is_some_and(|(width, _)| *width < floor) {
                best.pop();
            }
            if cells < floor {
                continue;
            }
            if best.len() == FIT_CANDIDATES && cells <= best[FIT_CANDIDATES - 1].0 {
                continue;
            }
            let at = best.partition_point(|(width, _)| *width >= cells);
            best.insert(at, (cells, row));
            best.truncate(FIT_CANDIDATES);
        }
        best.into_iter().map(|(_, row)| row).collect()
    }

    /// Works out where every showing column starts.
    pub(super) fn relayout(&mut self) {
        let mut laid_out = std::mem::take(&mut self.laid_out);
        laid_out.clear();

        let mut x = 0.;
        for (column, state) in self.columns.iter().enumerate() {
            if state.hidden {
                continue;
            }
            laid_out.push(Placed { column, x });
            x += state.width;
        }

        self.laid_out = laid_out;
        self.total_width = x;
    }

    /// The run of columns the content area can show.
    ///
    /// Two binary searches over the left edges, which is why several hundred
    /// columns cost nothing: the ones off either side are never looked at again.
    pub(super) fn visible_columns(&self) -> Range<usize> {
        if self.laid_out.is_empty() {
            return 0..0;
        }
        // The viewport is measured by the body's canvas during prepaint, which
        // is after the header for this frame was already built — and the
        // notify that measurement issues does not buy a second frame for an
        // entity that has just been drawn. On that first frame the header must
        // draw every column, clipped by its container, or it stays empty until
        // something else happens to invalidate the grid.
        if self.viewport_width <= 0. {
            return 0..self.laid_out.len();
        }
        let left = self.h_offset;
        let right = left + self.viewport_width;
        let first = self
            .laid_out
            .partition_point(|placed| placed.x + self.column_width(placed.column) <= left);
        let last = self.laid_out.partition_point(|placed| placed.x < right);
        first..last.max(first)
    }

    /// How far the columns could be scrolled sideways.
    pub(super) fn max_h_offset(&self) -> f32 {
        (self.total_width - self.viewport_width).max(0.)
    }

    /// Pulls the sideways offset back into range, after a resize or a hide.
    pub(super) fn clamp_h_offset(&mut self) {
        self.h_offset = self.h_offset.clamp(0., self.max_h_offset());
    }

    /// Scrolls the columns sideways.
    pub(super) fn set_h_offset(&mut self, offset: f32, cx: &mut Context<Self>) {
        let offset = offset.clamp(0., self.max_h_offset());
        if offset == self.h_offset {
            return;
        }
        // A sideways scroll is the user looking at another part of the row, and
        // a field that rode along would end up over a cell nobody is looking
        // at. `reveal` moves the offset by hand rather than through here, so
        // opening a field off the right-hand edge does not close it again.
        self.commit_edit(cx);
        self.h_offset = offset;
        cx.notify();
    }

    /// Notes how wide the content area turned out to be.
    ///
    /// Called from the body's [`canvas`] during prepaint. Asks for another frame
    /// when the width changed, because the header was drawn against the old one
    /// — see the module docs.
    pub(super) fn measured(&mut self, area: Size<Pixels>, cx: &mut Context<Self>) {
        let width = (f32::from(area.width) - GUTTER_WIDTH).max(0.);
        if (width - self.viewport_width).abs() < 0.5 {
            return;
        }
        self.viewport_width = width;
        self.clamp_h_offset();
        cx.notify();
    }

    /// Notes which rows the list built, and asks for the next batch when the
    /// end is in sight.
    pub(super) fn note_visible(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        self.visible_rows = rows;

        // The field is placed from the list's scroll offset every frame, so a
        // scroll that keeps its row on screen carries it along with the cell
        // and there is nothing to do here. A scroll that takes the row off
        // screen would leave the user typing into something they cannot see,
        // and the edit ends with the row. The wheel is why this is checked here
        // at all: the list owns the vertical axis, so a wheel scroll never
        // passes through any of the grid's own methods.
        if let Some(editing) = self.editing.as_ref()
            && editing.settled
            && !self.row_on_screen(editing.row)
        {
            self.commit_edit(cx);
        }

        let count = self.source.row_count();
        if self.source.state() != GridSourceState::HasMore {
            // A source that has stopped growing — or is already fetching —
            // forgets the request, so that the next time it says `HasMore` it
            // is asked afresh.
            self.asked_at = None;
            return;
        }
        if self.visible_rows.end + NEAR_END_ROWS < count {
            return;
        }
        if self.asked_at == Some(count) {
            return;
        }
        self.asked_at = Some(count);
        cx.emit(GridEvent::NearEnd);
    }

    /// Whether any part of `row` is inside the body.
    ///
    /// Worked out from the list's scroll offset rather than from the range the
    /// list last reported, because that range is not to be trusted at every
    /// point in a frame: the list renders one row on its own to find out how
    /// tall a row is, and reports `0..1` while it does.
    pub(super) fn row_on_screen(&self, row: usize) -> bool {
        let height = f32::from(self.base_handle().bounds().size.height);
        if height <= 0. {
            return true;
        }
        let top = row as f32 * ROW_HEIGHT + f32::from(self.base_handle().offset().y);
        top + ROW_HEIGHT > 0. && top < height
    }

    /// `row` and `column` as a cell, or `None` when there is no such cell.
    pub(super) fn clamped(&self, row: usize, column: usize) -> Option<CellAddress> {
        (row < self.source.row_count() && column < self.laid_out.len())
            .then_some(CellAddress::new(row, column))
    }

    /// Brings `cell` into view on both axes.
    pub(super) fn reveal(&mut self, cell: CellAddress) {
        self.scroll.scroll_to_item(cell.row, ScrollStrategy::Top);
        let Some(placed) = self.laid_out.get(cell.column).copied() else {
            return;
        };
        if self.viewport_width <= 0. {
            return;
        }

        let left = placed.x;
        let right = left + self.column_width(placed.column);
        if left < self.h_offset {
            self.h_offset = left;
        } else if right > self.h_offset + self.viewport_width {
            self.h_offset = right - self.viewport_width;
        }
        self.clamp_h_offset();
    }

    /// How many rows a page key moves by.
    ///
    /// One short of a screenful, so that the row that was at the bottom is at
    /// the top afterwards and the user has something to hold on to.
    pub(super) fn page(&self) -> usize {
        self.visible_rows.len().saturating_sub(1).max(1)
    }
}
