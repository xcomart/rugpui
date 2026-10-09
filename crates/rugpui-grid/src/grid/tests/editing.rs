use super::*;

/// The `NULL` row of a nullable dropdown stages `EditValue::Null`, which is
/// the gesture that clears a cell rather than emptying it — the distinction
/// the crate is built around, now with a way for a user to reach it.
#[gpui::test]
fn the_null_row_stages_a_null(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Chooser {
            nullable: true,
            value: Some("store"),
        },
        cx,
    );

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, CHOICE_COLUMN, window, cx)
    });
    click_option(&mut cx, 1, CHOICE_COLUMN, 0);

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: CHOICE_COLUMN,
            value: EditValue::Null,
        }]
    );
}

/// Without `nullable` there is no `NULL` row at all, so the first row of
/// the list is the first value — and clearing the cell is simply not on
/// offer, which is right for a column that cannot hold a null.
#[gpui::test]
fn a_dropdown_that_is_not_nullable_offers_only_its_values(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Chooser {
            nullable: false,
            value: Some("phone"),
        },
        cx,
    );

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, CHOICE_COLUMN, window, cx)
    });
    click_option(&mut cx, 0, CHOICE_COLUMN, 0);

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: CHOICE_COLUMN,
            value: EditValue::Text("web".to_owned()),
        }]
    );
}

/// A host's own editor stages through the `commit` it is handed, and the
/// grid raises exactly what a field would have raised.
#[gpui::test]
fn a_custom_editor_commits_through_its_context(cx: &mut TestAppContext) {
    let ways_out: Rc<RefCell<Option<(CellCommit, CellCancel)>>> = Rc::new(RefCell::new(None));
    let (grid, mut cx) = open(
        Own {
            ways_out: ways_out.clone(),
        },
        cx,
    );

    assert!(grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 1, window, cx)
    }));
    let (commit, _cancel) = ways_out
        .borrow()
        .clone()
        .expect("the host's editor was never built");

    cx.update(|window, cx| commit(EditValue::Text("2026-03-01".to_owned()), window, cx));
    cx.run_until_parked();

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: 1,
            value: EditValue::Text("2026-03-01".to_owned()),
        }]
    );
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
}

/// And `cancel` is the other half: the editor goes and nothing is staged.
#[gpui::test]
fn a_custom_editor_cancels_through_its_context(cx: &mut TestAppContext) {
    let ways_out: Rc<RefCell<Option<(CellCommit, CellCancel)>>> = Rc::new(RefCell::new(None));
    let (grid, mut cx) = open(
        Own {
            ways_out: ways_out.clone(),
        },
        cx,
    );

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 1, window, cx)
    });
    let (_commit, cancel) = ways_out
        .borrow()
        .clone()
        .expect("the host's editor was never built");

    cx.update(|window, cx| cancel(window, cx));
    cx.run_until_parked();

    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(grid.drain(), vec![], "a cancelled editor staged something");
}

/// A source that never opted in cannot be typed into, however the host
/// asks: the default `cell_editable` is what stands between a read-only
/// result and an editor over it.
#[gpui::test]
fn a_source_that_did_not_opt_in_cannot_be_edited(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(6, 4, probe), cx);

    assert!(!grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 1, window, cx)
    }));
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(grid.drain(), vec![], "a refused edit announced something");
}

/// Opening a null cell and thinking better of it leaves it null.
///
/// The trap this whole crate is built to avoid (design notes,
/// §7.5): a field seeded empty because there was no value, committed
/// unchanged, must not become an `UPDATE … SET x = ''`. The same holds for a
/// cell that really does hold the empty string, and for one with a value in
/// it that nobody touched.
#[gpui::test]
fn a_field_nobody_changed_stages_nothing(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);

    // Row 0, column 1 is null.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 1, window, cx)
    });
    assert_eq!(grid.typed(&mut cx).as_deref(), Some(""));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        grid.drain(),
        vec![],
        "a null cell was turned into the empty string by being looked at"
    );

    // Row 1, column 1 really does hold the empty string.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 1, window, cx)
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(grid.drain(), vec![]);

    // And a value left exactly as it was found.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 2, window, cx)
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(grid.drain(), vec![]);

    // Typing into the null one, though, is a change — and the emptiness it
    // started from is not the value it commits.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 1, window, cx)
    });
    cx.simulate_input("x");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: 1,
            value: EditValue::Text("x".to_owned()),
        }]
    );
}

/// `Tab` commits and walks on to the next cell of the row that will take an
/// edit — stepping over the key column, which will not — and `Shift+Tab`
/// walks back. Falling off the end leaves the commit standing and the field
/// closed.
#[gpui::test]
fn tab_commits_and_opens_the_next_editable_cell(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 1, window, cx)
    });
    cx.simulate_input("typed");
    cx.simulate_keystrokes("tab");

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: 1,
            value: EditValue::Text("typed".to_owned()),
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        Some((1, 2)),
        "tab did not reopen on the next editable cell"
    );
    assert_eq!(grid.typed(&mut cx).as_deref(), Some("there"));

    // Backwards, over the same gap, and stopping at column 1 rather than
    // landing on the key column.
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), Some((1, 1)));
    assert_eq!(
        grid.drain(),
        vec![],
        "a field nobody touched was staged on the way back"
    );

    cx.simulate_keystrokes("shift-tab");
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        None,
        "shift-tab opened the key column"
    );

    // And off the far end: the last editable cell of the row has nowhere to
    // hand the field on to.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 2, window, cx)
    });
    cx.simulate_input("!");
    cx.simulate_keystrokes("tab");
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: 2,
            value: EditValue::Text("there!".to_owned()),
        }]
    );
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
}

/// Everything that moves the cell out from under the field ends the edit,
/// and ends it by committing — see the module docs on why that way round.
#[gpui::test]
fn anything_that_moves_the_cell_commits_the_field(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);

    // A sort: the rows are about to be a different set of rows.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 2, window, cx)
    });
    cx.simulate_input("?");
    grid.update(&mut cx, |grid, cx| grid.toggle_sort(0, cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![
            GridEvent::EditCommitted {
                row: 1,
                column: 2,
                value: EditValue::Text("there?".to_owned()),
            },
            GridEvent::SortRequested {
                column: 0,
                direction: Some(SortDirection::Ascending),
            },
        ],
        "the commit did not come before the thing that caused it"
    );

    // A column hidden out from under it: there would be nowhere left to
    // draw the field.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 2, window, cx)
    });
    cx.simulate_input("!");
    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(2, true, cx));
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: 2,
            value: EditValue::Text("here!".to_owned()),
        }]
    );

    // An arrow key, which the field does not want and the grid does.
    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(2, false, cx));
    grid.drain();
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 1, window, cx)
    });
    cx.simulate_input("q");
    cx.simulate_keystrokes("down");
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: 1,
            value: EditValue::Text("q".to_owned()),
        }]
    );
    assert_eq!(
        grid.selected(&mut cx, 2, 3),
        vec![(1, 1)],
        "the arrow committed but did not move"
    );

    // And the host dropping a batch in, which is the same problem arriving
    // from the other side.
    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 2, window, cx)
    });
    cx.simulate_input("z");
    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).status[0] = RowStatus::Modified;
    });
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: 2,
            value: EditValue::Text("herez".to_owned()),
        }]
    );
}

/// A wheel is the one scroll the grid does not run itself — the list owns
/// the vertical axis — so the field has to notice on its own that its row
/// has gone, rather than being told.
#[gpui::test]
fn a_row_scrolled_out_of_sight_takes_its_field_with_it(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(10_000, 4, probe).editable(), cx);

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 1, window, cx)
    });
    cx.simulate_input("typed");
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), Some((0, 1)));

    cx.simulate_event(gpui::ScrollWheelEvent {
        position: point(px(column_x(1)), px(row_y(2))),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-4_000.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    assert!(
        grid.read(&mut cx, |grid| grid.visible_rows().start) > 0,
        "the wheel scrolled nothing, so the test proves nothing"
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        None,
        "the field was left over a row that is no longer on screen"
    );
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: 1,
            value: EditValue::Text("valuetyped".to_owned()),
        }]
    );
}
