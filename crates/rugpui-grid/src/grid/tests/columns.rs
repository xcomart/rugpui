use super::*;

/// The claim the whole crate is built around: a million rows and forty
/// columns cost exactly one screenful of reads per frame, and the reads land
/// where the viewport is rather than at the start of the result.
#[gpui::test]
fn only_the_visible_rows_and_columns_are_read(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(1_000_000, 40, probe.clone()), cx);

    // A screenful is what fits in 1920 by 1080 at the default sizes: about
    // forty rows and thirteen columns. The bound is deliberately loose —
    // what matters is that it does not scale with the million.
    let visible_rows = grid.read(&mut cx, |grid| grid.visible_rows());
    assert!(
        visible_rows.len() < 60,
        "the list built {} rows",
        visible_rows.len()
    );
    assert!(
        (CONTENT_WIDTH / DEFAULT_COLUMN_WIDTH) as usize <= 14,
        "the fixture no longer matches the test window"
    );

    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));

    // Forty million cells exist; one frame reads six hundred odd of them —
    // 44 rows by 14 columns, plus the row `uniform_list` measures twice to
    // find the row height. The bound is loose on purpose: what must hold is
    // that it is a function of the window and not of the result.
    assert!(
        probe.reads.get() < 2_000,
        "one frame read {} cells",
        probe.reads.get()
    );
    assert!(
        probe.max_row.get() < 60,
        "row {} was read for a viewport of {} rows",
        probe.max_row.get(),
        visible_rows.len()
    );
    assert!(
        probe.max_column.get() < 20,
        "column {} was read of forty",
        probe.max_column.get()
    );

    // The edit markers are on the same budget as the values, and were the
    // easiest thing in the crate to get wrong: a row marker asked for down
    // the whole result would read a million rows to draw forty.
    assert!(
        probe.marks.get() < 2_000,
        "one frame asked about {} marks",
        probe.marks.get()
    );
    assert!(
        probe.max_mark_row.get() < 60,
        "the mark of row {} was asked for a viewport of {} rows",
        probe.max_mark_row.get(),
        visible_rows.len()
    );

    // And scrolling moves the window of reads rather than widening it: the
    // rows around row 900,000 are read, and none of the ones before them.
    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.scroll_to_row(900_000, cx));
    cx.run_until_parked();

    assert!(
        probe.reads.get() < 2_000,
        "the scrolled frame read {} cells",
        probe.reads.get()
    );
    assert!(
        grid.read(&mut cx, |grid| grid.visible_rows().start) > 800_000,
        "the viewport did not follow the scroll"
    );
    assert!(
        probe.max_row.get() > 800_000,
        "the reads did not follow the viewport"
    );
    assert!(
        probe.max_mark_row.get() > 800_000,
        "the marks did not follow the viewport"
    );
    assert!(
        probe.marks.get() < 2_000,
        "the scrolled frame asked about {} marks",
        probe.marks.get()
    );
}

/// The frame that has laid columns out but not yet measured the viewport —
/// the first one, where the header is built before the body's canvas runs —
/// draws every column rather than none.
///
/// A `VisualTestContext` draws repeatedly, so the ordinary tests never see
/// this frame: the real app did, as a permanently empty header band,
/// because the notify issued by the measurement does not buy a second
/// frame for an entity that was just drawn.
#[gpui::test]
fn an_unmeasured_viewport_shows_every_header(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(5, 7, probe), cx);

    grid.update(&mut cx, |grid, _| {
        assert!(!grid.laid_out.is_empty(), "the fixture never laid out");
        grid.viewport_width = 0.;
        assert_eq!(
            grid.visible_columns(),
            0..grid.laid_out.len(),
            "the header of the unmeasured frame"
        );
    });
}

/// Ascending, descending, gone — and the grid never touches its own rows.
#[gpui::test]
fn a_header_click_walks_the_sort_round(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(null_and_empty(), cx);

    grid.update(&mut cx, |grid, cx| grid.toggle_sort(1, cx));
    assert_eq!(
        grid.drain(),
        vec![GridEvent::SortRequested {
            column: 1,
            direction: Some(SortDirection::Ascending)
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.sort()),
        Some((1, SortDirection::Ascending))
    );

    grid.update(&mut cx, |grid, cx| grid.toggle_sort(1, cx));
    assert_eq!(
        grid.drain(),
        vec![GridEvent::SortRequested {
            column: 1,
            direction: Some(SortDirection::Descending)
        }]
    );

    grid.update(&mut cx, |grid, cx| grid.toggle_sort(1, cx));
    assert_eq!(
        grid.drain(),
        vec![GridEvent::SortRequested {
            column: 1,
            direction: None
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.sort()),
        None,
        "the third click left the column ordered"
    );

    // Another column starts its own round from the top rather than picking
    // up where the last one left off.
    grid.update(&mut cx, |grid, cx| grid.toggle_sort(1, cx));
    grid.drain();
    grid.update(&mut cx, |grid, cx| grid.toggle_sort(2, cx));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.sort()),
        Some((2, SortDirection::Ascending))
    );
}

/// A right click on a heading raises the column's menu, names the *source*
/// column, and does not re-sort what it was pressed on.
#[gpui::test]
fn a_right_click_on_a_heading_names_the_source_column(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(6, 4, probe), cx);

    click_cell(&mut cx, 1, 1);
    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(0, true, cx));
    grid.drain();

    // Display column 1 is now source column 2.
    let position = right_click_at(&mut cx, column_x(1), HEADER_HEIGHT / 2.);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::ContextMenu {
            target: MenuTarget::Header { column: 2 },
            position,
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.sort()),
        None,
        "a right click sorted the column"
    );
}

/// The way back from hiding: the only column gesture with no heading of its
/// own to be reached from, so a host menu is the only route to it.
#[gpui::test]
fn every_hidden_column_can_be_shown_again(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(null_and_empty(), cx);

    assert_eq!(grid.read(&mut cx, |grid| grid.hidden_column_count()), 0);
    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_name(0).map(str::to_owned)),
        Some("id".to_owned())
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_name(9).map(str::to_owned)),
        None
    );

    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(0, true, cx));
    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(2, true, cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.hidden_column_count()), 2);
    assert_eq!(
        grid.read(&mut cx, |grid| grid.visible_column_indices()),
        vec![1]
    );

    grid.update(&mut cx, |grid, cx| grid.show_all_columns(cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.hidden_column_count()), 0);
    assert_eq!(
        grid.read(&mut cx, |grid| grid.visible_column_indices()),
        vec![0, 1, 2]
    );
    assert!(
        grid.read(&mut cx, |grid| grid.selection().is_empty()),
        "the display positions moved under the selection"
    );
}

/// Hiding a column takes it out of the grid, out of a copy and out of the
/// numbering the selection is written in.
#[gpui::test]
fn a_hidden_column_leaves_the_grid_entirely(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(null_and_empty(), cx);

    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(1, true, cx));
    assert!(grid.read(&mut cx, |grid| grid.is_column_hidden(1)));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.visible_column_indices()),
        vec![0, 2]
    );

    grid.update(&mut cx, |grid, cx| grid.select_all(cx));
    grid.update(&mut cx, |grid, cx| grid.copy(CopyFormat::Tsv, cx));
    let tsv = cx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .expect("the clipboard was not written");
    assert_eq!(tsv, "1\t\n2\t");

    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(1, false, cx));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.visible_column_indices()),
        vec![0, 1, 2]
    );
}

/// A column can be widened and fitted, and a fit never shrinks a column
/// below what can be grabbed again.
///
/// On a `fixed_widths` grid, so that the starting width is known and so that
/// the fit under test is the one the double click asks for rather than the
/// one the first frame would have done anyway.
#[gpui::test]
fn a_column_can_be_widened_and_fitted(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Small {
            headings: vec![("id", GridColumnKind::Number)],
            rows: vec![vec![Some("a rather long value indeed")]],
        },
        cx,
    );

    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(0)),
        DEFAULT_COLUMN_WIDTH
    );

    grid.update(&mut cx, |grid, cx| grid.set_column_width(0, 4., cx));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(0)),
        MIN_COLUMN_WIDTH,
        "a column was dragged shut"
    );

    grid.update(&mut cx, |grid, cx| grid.autofit_column(0, cx));
    let fitted = grid.read(&mut cx, |grid| grid.column_width(0));
    assert!(
        fitted > DEFAULT_COLUMN_WIDTH && fitted <= MAX_AUTOFIT_WIDTH,
        "a twenty-six character value fitted to {fitted}"
    );
}

/// The default a host gets: every column sized to what is in it, so a
/// number column is narrow, a timestamp column is wide enough to read, and
/// neither is the width the other one happens to need.
#[gpui::test]
fn the_columns_fit_their_content_by_default(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);

    let (number, timestamp) =
        grid.read(&mut cx, |grid| (grid.column_width(0), grid.column_width(1)));
    assert!(
        number < DEFAULT_COLUMN_WIDTH,
        "a two-character number column fitted to {number}"
    );
    assert!(
        timestamp > DEFAULT_COLUMN_WIDTH,
        "a nineteen-character timestamp column fitted to {timestamp}"
    );
    assert!(
        timestamp > number,
        "the wide column ({timestamp}) came out no wider than the narrow one ({number})"
    );
}

/// The fit is a *measurement*: the width is what the text system says the
/// longest value shapes to, plus the padding either side of it, and not a
/// guess from character cells that would leave the value an ellipsis short.
#[gpui::test]
fn a_fitted_width_is_the_shaped_width_of_the_widest_value(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);

    let expected = fitted_width_of(LONGEST, &mut cx);
    assert_eq!(grid.read(&mut cx, |grid| grid.column_width(1)), expected);
}

/// Character cells are not pixels, and the fit must not confuse the two.
///
/// On the face an app really ships this is `Pinewood Hardware` beating
/// `Northwind Traders` — the same seventeen characters, visibly different
/// widths, because `W` is broad and `i` is a hairline. A fit that shaped only
/// the longest few values by character count picks whichever of those the
/// sample happened to reach first and truncates the other.
///
/// A test has no real face to show that with: gpui's test text system gives
/// every character in the basic plane exactly the same advance, so `W` and
/// `i` are the same width there. What it does give is the same *divergence*
/// approached from the other end — `unicode-width` counts an ideograph as
/// two cells while the test font advances it by one — so here the value that
/// is longest by cells is the one that shapes narrowest. Which is the case
/// that has to come out right either way.
#[gpui::test]
fn a_column_fits_the_widest_value_and_not_the_longest(cx: &mut TestAppContext) {
    // Eighteen cells, nine glyphs.
    let by_cells = "漢字漢字漢字漢字漢";
    // Sixteen cells, sixteen glyphs: narrower by the count that is cheap to
    // take, wider by the one that is on screen.
    let by_pixels = "WWWWWWWWWWWWWWWW";

    let (grid, mut cx) = open_fitted(
        Small {
            headings: vec![("customer", GridColumnKind::Text)],
            rows: vec![
                vec![Some(by_cells)],
                vec![Some(by_cells)],
                vec![Some(by_cells)],
                vec![Some(by_pixels)],
                vec![Some("short")],
            ],
        },
        cx,
    );

    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(0)),
        fitted_width_of(by_pixels, &mut cx),
        "the column was fitted to its longest value rather than its widest"
    );
}

/// `fixed_widths()` is the way out, and it means what it says.
#[gpui::test]
fn fixed_widths_leaves_every_column_at_the_default(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(narrow_and_wide(), cx);

    assert_eq!(
        grid.read(&mut cx, |grid| (grid.column_width(0), grid.column_width(1))),
        (DEFAULT_COLUMN_WIDTH, DEFAULT_COLUMN_WIDTH)
    );
}

/// A width the user dragged is theirs: neither another batch of the same
/// result nor a refresh takes it back.
#[gpui::test]
fn a_width_the_user_set_survives_more_rows(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);
    grid.update(&mut cx, |grid, cx| grid.set_column_width(1, 300., cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.column_width(1)), 300.);

    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).rows.push(vec![
            Some("3"),
            Some("a value far longer than three hundred pixels of it"),
        ]);
    });
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));

    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(1)),
        300.,
        "another batch took back a width the user had dragged"
    );
}

/// A *new* result is the one thing that does take it back: the widths were
/// chosen for values that are not there any more.
#[gpui::test]
fn a_new_result_fits_the_columns_again(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);
    let fitted = grid.read(&mut cx, |grid| grid.column_width(1));

    grid.update(&mut cx, |grid, cx| grid.set_column_width(1, 300., cx));
    grid.update(&mut cx, |grid, cx| grid.reset(cx));

    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(1)),
        fitted,
        "a new result left the last one's dragged width behind"
    );
}

/// Paging only ever widens. A column that narrowed as the next batch landed
/// would slide every column after it sideways under a pointer that is
/// reading them, which is worse than a column a few pixels too wide.
#[gpui::test]
fn more_rows_only_ever_widen_a_fitted_column(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);
    let fitted = grid.read(&mut cx, |grid| grid.column_width(1));

    // More rows than before — so a fit does run — but every value in them
    // shorter than what the column was sized to.
    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).rows = vec![
            vec![Some("1"), Some("x")],
            vec![Some("2"), Some("y")],
            vec![Some("3"), Some("z")],
        ];
    });
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(1)),
        fitted,
        "a second batch of shorter values narrowed the column"
    );

    // A longer one, though, does widen it: the sample is still filling up.
    let longer = "a good deal longer than any timestamp";
    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).rows.push(vec![Some("4"), Some(longer)]);
    });
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(1)),
        fitted_width_of(longer, &mut cx),
        "a longer value in a later batch did not widen the column"
    );
}

/// "Fit all columns" is somebody asking, so it is allowed to take back a
/// width the user dragged — which is the whole difference between it and
/// the fitting the grid does on its own.
#[gpui::test]
fn fitting_every_column_overrules_a_width_the_user_set(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_fitted(narrow_and_wide(), cx);
    grid.update(&mut cx, |grid, cx| grid.set_column_width(1, 400., cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.column_width(1)), 400.);

    grid.update(&mut cx, |grid, cx| grid.autofit_all_columns(cx));

    assert_eq!(
        grid.read(&mut cx, |grid| grid.column_width(1)),
        fitted_width_of(LONGEST, &mut cx)
    );
}

/// The other axis, which no `uniform_list` does for us: scrolling sideways
/// moves the run of columns that is read, and the ones behind the left edge
/// stop being read at all.
#[gpui::test]
fn only_the_visible_columns_are_read(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(200, 60, probe.clone()), cx);

    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert_eq!(probe.min_column.get(), 0, "the left edge was not drawn");
    let first_screen = probe.max_column.get();
    assert!(
        first_screen < 20,
        "column {first_screen} of sixty was drawn"
    );

    // Walking the cursor out to column fifty scrolls the strip along; the
    // columns at the left-hand end are now off screen, and are not asked
    // about at all.
    grid.update(&mut cx, |grid, cx| grid.select_cell(0, 50, cx));
    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));

    assert!(
        probe.min_column.get() > first_screen,
        "columns 0..={} were still being read after scrolling to fifty",
        probe.min_column.get()
    );
    assert!(probe.max_column.get() >= 50, "column fifty was not drawn");
    assert!(
        probe.reads.get() < 2_000,
        "one frame read {} cells",
        probe.reads.get()
    );
}

/// A sideways wheel — or a plain one with `Shift`, which is what a mouse
/// without a second axis has — scrolls the columns.
#[gpui::test]
fn the_wheel_scrolls_the_columns_sideways(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(20, 60, probe.clone()), cx);
    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    let before = probe.max_column.get();

    cx.simulate_event(gpui::ScrollWheelEvent {
        position: point(px(column_x(2)), px(row_y(2))),
        delta: gpui::ScrollDelta::Pixels(point(px(-600.), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert!(
        probe.max_column.get() > before,
        "the wheel moved nothing: still stopping at column {}",
        probe.max_column.get()
    );
    assert!(probe.min_column.get() > 0, "the left edge never left");
}

/// The same sideways wheel leaves the vertical scroll exactly where it
/// was: `restrict_scroll_to_axis` on the row list is what stops gpui's shared
/// listener from folding the X delta it doesn't otherwise use onto Y.
#[gpui::test]
fn the_wheel_scrolls_the_columns_sideways_without_scrolling_the_rows(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(10_000, 60, probe.clone()), cx);
    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    let before_column = probe.max_column.get();
    let before_rows = grid.read(&mut cx, |grid| grid.visible_rows());

    cx.simulate_event(gpui::ScrollWheelEvent {
        position: point(px(column_x(2)), px(row_y(2))),
        delta: gpui::ScrollDelta::Pixels(point(px(-600.), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert!(
        probe.max_column.get() > before_column,
        "the wheel moved nothing sideways: still stopping at column {}",
        probe.max_column.get()
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.visible_rows()),
        before_rows,
        "a sideways wheel scrolled the rows too"
    );
}
