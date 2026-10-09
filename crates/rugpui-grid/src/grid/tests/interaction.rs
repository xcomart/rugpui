use super::*;

/// The next batch is asked for once, not once per frame, and asked for again
/// only when the answer to the first one has landed.
#[gpui::test]
fn the_next_batch_is_asked_for_once(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(20, 3, probe).growing(), cx);

    assert_eq!(
        grid.drain(),
        vec![GridEvent::NearEnd],
        "the end was in sight and nobody was told"
    );

    // A burst of repaints — which is what a fast scroll is — asks for
    // nothing more, because nothing has changed about how much there is.
    for _ in 0..10 {
        grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    }
    assert_eq!(grid.drain(), vec![], "a redraw was mistaken for a scroll");

    // The batch lands: more rows, and the new end is in sight too.
    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).rows.set(60);
    });
    assert_eq!(grid.drain(), vec![GridEvent::NearEnd]);

    // And a source that has everything is never asked again, however often
    // it is redrawn.
    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).state.set(GridSourceState::Complete);
    });
    for _ in 0..5 {
        grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    }
    assert_eq!(grid.drain(), vec![]);
}

/// A fetch already in flight is not asked for again either: `Loading` is an
/// answer, and the request stands until it turns back into `HasMore`.
#[gpui::test]
fn a_fetch_in_flight_is_not_asked_for_again(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(20, 3, probe).growing(), cx);
    assert_eq!(grid.drain(), vec![GridEvent::NearEnd]);

    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).state.set(GridSourceState::Loading);
    });
    for _ in 0..5 {
        grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    }
    assert_eq!(grid.drain(), vec![]);
}

/// A click picks a cell; shift stretches a block; ctrl adds one; a row
/// number takes the whole row.
#[gpui::test]
fn the_pointer_picks_cells_blocks_and_rows(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(6, 4, probe), cx);

    click_cell(&mut cx, 1, 1);
    assert_eq!(grid.selected(&mut cx, 6, 4), vec![(1, 1)]);

    click_at(&mut cx, column_x(2), row_y(2), Modifiers::shift(), 1);
    assert_eq!(
        grid.selected(&mut cx, 6, 4),
        vec![(1, 1), (1, 2), (2, 1), (2, 2)]
    );

    click_at(
        &mut cx,
        column_x(0),
        row_y(4),
        Modifiers::secondary_key(),
        1,
    );
    assert_eq!(
        grid.selected(&mut cx, 6, 4),
        vec![(1, 1), (1, 2), (2, 1), (2, 2), (4, 0)]
    );

    // The row-number gutter takes the whole width, and drops the blocks.
    click_at(&mut cx, GUTTER_WIDTH / 2., row_y(3), Modifiers::none(), 1);
    assert_eq!(
        grid.selected(&mut cx, 6, 4),
        vec![(3, 0), (3, 1), (3, 2), (3, 3)]
    );
}

/// A right click asks for a menu and moves the selection onto what was
/// pressed — unless the press was already inside it, which is what keeps a
/// menu raised over a block from being about one cell of it.
#[gpui::test]
fn a_right_click_asks_for_a_menu_and_moves_the_selection(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(6, 4, probe), cx);

    // A block, so that "inside" and "outside" both exist.
    click_cell(&mut cx, 1, 1);
    click_at(&mut cx, column_x(2), row_y(2), Modifiers::shift(), 1);
    grid.drain();

    // Outside it: the selection follows the press.
    let position = right_click_at(&mut cx, column_x(3), row_y(4));
    assert_eq!(grid.selected(&mut cx, 6, 4), vec![(4, 3)]);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::ContextMenu {
            target: MenuTarget::Cell,
            position,
        }],
        "the press was not reported in window coordinates"
    );

    // Inside it: the selection stays whole.
    click_cell(&mut cx, 1, 1);
    click_at(&mut cx, column_x(2), row_y(2), Modifiers::shift(), 1);
    grid.drain();
    let block = grid.selected(&mut cx, 6, 4);
    let position = right_click_at(&mut cx, column_x(2), row_y(1));
    assert_eq!(
        grid.selected(&mut cx, 6, 4),
        block,
        "a right click inside the selection shrank it"
    );
    assert_eq!(
        grid.drain(),
        vec![GridEvent::ContextMenu {
            target: MenuTarget::Cell,
            position,
        }]
    );

    // The gutter takes the whole row, as a left click there does.
    let position = right_click_at(&mut cx, GUTTER_WIDTH / 2., row_y(5));
    assert_eq!(
        grid.selected(&mut cx, 6, 4),
        vec![(5, 0), (5, 1), (5, 2), (5, 3)]
    );
    assert_eq!(
        grid.drain(),
        vec![GridEvent::ContextMenu {
            target: MenuTarget::Cell,
            position,
        }]
    );
}

/// A double click is how a LOB reaches its viewer, and it names the *source*
/// column rather than the one the user happens to be looking at.
#[gpui::test]
fn a_double_click_activates_the_cell(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(6, 4, probe), cx);

    grid.update(&mut cx, |grid, cx| grid.set_column_hidden(1, true, cx));
    grid.drain();

    // Display column 1 is now source column 2.
    click_at(&mut cx, column_x(1), row_y(2), Modifiers::none(), 2);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::CellActivated { row: 2, column: 2 }]
    );
}

/// The arrows walk the cells, shift stretches from where they started, and
/// `Ctrl+A` takes everything.
#[gpui::test]
fn the_keyboard_walks_and_stretches(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(8, 4, probe), cx);

    // Nothing picked yet: the first key lands on the first cell rather than
    // one step away from it.
    cx.simulate_keystrokes("down");
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(0, 0)]);

    cx.simulate_keystrokes("down right");
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(1, 1)]);

    cx.simulate_keystrokes("shift-down shift-right");
    assert_eq!(
        grid.selected(&mut cx, 8, 4),
        vec![(1, 1), (1, 2), (2, 1), (2, 2)]
    );

    // And the ends: `Home` and `End` on the row, the modifier for the whole
    // result.
    cx.simulate_keystrokes("end");
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(2, 3)]);
    cx.simulate_keystrokes("home");
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(2, 0)]);

    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{modifier}-end"));
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(7, 3)]);
    cx.simulate_keystrokes(&format!("{modifier}-home"));
    assert_eq!(grid.selected(&mut cx, 8, 4), vec![(0, 0)]);

    cx.simulate_keystrokes(&format!("{modifier}-a"));
    assert_eq!(grid.selected(&mut cx, 8, 4).len(), 32);
}

/// A page key moves by a screenful and stops at the end rather than running
/// off it.
#[gpui::test]
fn a_page_key_moves_by_a_screenful(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(1_000, 3, probe), cx);
    let page = grid.read(&mut cx, |grid| grid.visible_rows().len() - 1);
    assert!(page > 10, "the test window is smaller than it was");

    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("pagedown");
    assert_eq!(
        grid.selected(&mut cx, 1_000, 1).first().map(|c| c.0),
        Some(page)
    );

    cx.simulate_keystrokes("pageup pageup");
    assert_eq!(
        grid.selected(&mut cx, 1_000, 1).first().map(|c| c.0),
        Some(0)
    );
}

/// `Ctrl+C` puts the selection on the clipboard as TSV, and the other three
/// formats are a method call away.
#[gpui::test]
fn the_selection_reaches_the_clipboard(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(null_and_empty(), cx);

    grid.update(&mut cx, |grid, cx| grid.select_all(cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });

    let tsv = cx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .expect("the clipboard was not written");
    assert_eq!(tsv, "1\t\t\n2\there\t");

    // The same block in the format that can carry the difference the TSV
    // above cannot: row one's second column is null and its third is the
    // empty string.
    grid.update(&mut cx, |grid, cx| grid.copy(CopyFormat::Json, cx));
    let json = cx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .expect("the clipboard was not written");
    assert!(json.contains("\"nothing\": null,"), "{json}");
    assert!(json.contains("\"empty\": \"\""), "{json}");
}

/// A result the host replaces with a smaller one leaves no selection
/// hanging over rows that are gone.
#[gpui::test]
fn a_replaced_result_pulls_the_selection_back_in(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(50, 4, probe), cx);

    grid.update(&mut cx, |grid, cx| grid.select_all(cx));
    assert!(grid.read(&mut cx, |grid| grid.is_selected(49, 3)));

    grid.update(&mut cx, |grid, cx| {
        grid.source_mut(cx).rows.set(3);
    });
    assert!(!grid.read(&mut cx, |grid| grid.is_selected(49, 3)));
    assert!(grid.read(&mut cx, |grid| grid.is_selected(2, 3)));

    // And a new result — a different shape entirely — starts clean.
    grid.update(&mut cx, |grid, cx| grid.reset(cx));
    assert!(grid.read(&mut cx, |grid| grid.selection().is_empty()));
    assert_eq!(grid.read(&mut cx, |grid| grid.sort()), None);
}

/// A source may draw a cell itself, and is asked to do so only for the
/// cells somebody can see.
///
/// The second half is the whole point: a hook called once per visible cell
/// per frame is affordable, and one called per *row* would undo the
/// virtualisation the crate exists for. A hundred thousand rows are
/// offered; the hook must never hear of the ones off screen, before or
/// after a scroll to the middle of them.
#[gpui::test]
fn a_source_can_draw_its_own_cells(cx: &mut TestAppContext) {
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let (grid, mut cx) = open(
        Drawn {
            rows: 100_000,
            drawn: drawn.clone(),
        },
        cx,
    );

    let asked_about = |drawn: &Rc<RefCell<Vec<usize>>>| {
        let mut rows = drawn.borrow().clone();
        rows.sort_unstable();
        rows.dedup();
        rows
    };

    let visible = grid.read(&mut cx, |grid| grid.visible_rows());
    let rows = asked_about(&drawn);
    assert!(!rows.is_empty(), "the hook was never called at all");
    assert!(
        rows.iter().all(|row| visible.contains(row)),
        "the hook was asked about rows nobody can see: {rows:?} against {visible:?}"
    );
    assert!(
        drawn.borrow().len() <= visible.len() * 8,
        "one screenful cost {} calls",
        drawn.borrow().len()
    );

    drawn.borrow_mut().clear();
    let before = visible;
    grid.update(&mut cx, |grid, cx| grid.scroll_to_row(50_000, cx));

    let visible = grid.read(&mut cx, |grid| grid.visible_rows());
    assert!(visible.start >= 49_000, "the scroll did not happen");
    let rows = asked_about(&drawn);
    assert!(!rows.is_empty(), "the hook stopped being called");
    // Two viewports and not one: a scroll is drawn twice, once where the
    // list was and once where it lands, and both are screenfuls. What is
    // being ruled out is the fifty thousand rows between them.
    assert!(
        rows.iter()
            .all(|row| visible.contains(row) || before.contains(row)),
        "the hook was asked about rows nobody can see: {rows:?} against {visible:?}"
    );
}

/// A cell whose source asked for a dropdown gets one, opened over the cell,
/// and picking a row stages it there and then — no `Enter`, because there is
/// nothing half-typed to confirm.
#[gpui::test]
fn picking_a_row_of_a_dropdown_stages_it(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Chooser {
            nullable: true,
            value: Some("store"),
        },
        cx,
    );

    assert!(grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, CHOICE_COLUMN, window, cx)
    }));
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        Some((0, CHOICE_COLUMN))
    );
    assert_eq!(
        grid.typed(&mut cx),
        None,
        "a dropdown offered a half-typed value"
    );

    // Row 0 is the `NULL` row, so row 1 is the first of the three values.
    click_option(&mut cx, 0, CHOICE_COLUMN, 1);

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: CHOICE_COLUMN,
            value: EditValue::Text("web".to_owned()),
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        None,
        "the list outlived the pick"
    );
}

/// The whole gesture, end to end: a double click on a cell whose source
/// asked for a dropdown opens one, and the row picked out of it is staged.
///
/// Worth its own test because the activation and the editor are two
/// separate round trips through the host — the grid raises
/// `CellActivated`, the host answers with `begin_edit` — and because the
/// press that opened the list must not be the press that dismisses it.
#[gpui::test]
fn a_double_click_opens_the_dropdown_a_source_asked_for(cx: &mut TestAppContext) {
    let (grid, mut cx) = open_activating(
        Chooser {
            nullable: true,
            value: Some("store"),
        },
        cx,
    );

    click_at(
        &mut cx,
        column_x(CHOICE_COLUMN),
        row_y(0),
        Modifiers::none(),
        2,
    );

    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        Some((0, CHOICE_COLUMN)),
        "the double click did not reach an editor"
    );

    click_option(&mut cx, 0, CHOICE_COLUMN, 3);

    assert_eq!(
        grid.drain(),
        vec![
            GridEvent::CellActivated {
                row: 0,
                column: CHOICE_COLUMN,
            },
            GridEvent::EditCommitted {
                row: 0,
                column: CHOICE_COLUMN,
                value: EditValue::Text("phone".to_owned()),
            }
        ]
    );
}

/// `Escape` closes a dropdown with nothing staged, exactly as it throws a
/// field's typing away.
#[gpui::test]
fn escape_closes_a_dropdown_without_staging(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Chooser {
            nullable: true,
            value: Some("store"),
        },
        cx,
    );

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, CHOICE_COLUMN, window, cx)
    });
    cx.simulate_keystrokes("escape");

    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![],
        "escaping a list staged something anyway"
    );
}

/// The arrows walk the open list rather than the selection under it, and
/// `Enter` stages where they stopped.
///
/// The one place the grid does not simply hand the keys to the control: the
/// focus is on the box the list hangs from, not on the trigger inside it,
/// so `Select`'s own arrow handling never sees the keystroke — and the
/// grid's `MoveDown` would otherwise walk the cursor out from under the
/// list the user is reading.
#[gpui::test]
fn the_arrows_walk_an_open_list_and_enter_picks(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(
        Chooser {
            nullable: true,
            value: Some("store"),
        },
        cx,
    );

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, CHOICE_COLUMN, window, cx)
    });
    // The list opened on "store", which is row 2 of `NULL`/web/store/phone.
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 0,
            column: CHOICE_COLUMN,
            value: EditValue::Text("phone".to_owned()),
        }]
    );
    // And the cursor stayed where the edit was, rather than following the
    // arrow down a row.
    assert_eq!(grid.selected(&mut cx, 3, 2), vec![(0, CHOICE_COLUMN)]);
}

/// The whole round trip: the field opens over the cell holding what the cell
/// holds, what is typed goes into it, and `Enter` hands the host the value
/// and the *source* column it belongs to.
#[gpui::test]
fn typing_into_a_cell_and_pressing_enter_stages_the_value(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);

    assert!(grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(1, 2, window, cx)
    }));
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), Some((1, 2)));
    assert_eq!(
        grid.typed(&mut cx).as_deref(),
        Some("there"),
        "the field was not seeded with what the cell held"
    );
    // The cursor followed the field, which is where a user would expect it.
    assert_eq!(grid.selected(&mut cx, 2, 3), vec![(1, 2)]);

    cx.simulate_input("!");
    cx.simulate_keystrokes("enter");

    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: 2,
            value: EditValue::Text("there!".to_owned()),
        }]
    );
    assert_eq!(
        grid.read(&mut cx, |grid| grid.editing()),
        None,
        "the field outlived the commit"
    );
    // Nothing was staged *here*: the grid changed no value of its own, and
    // goes on drawing whatever the source returns.
    assert_eq!(
        grid.read(&mut cx, |grid| match grid.source().cell(1, 2) {
            GridCell::Text(text) => text.to_owned(),
            other => panic!("{other:?}"),
        }),
        "there",
        "the grid wrote the typed value into the result"
    );
    // And the keyboard came back to the grid, rather than being left on a
    // field that no longer exists.
    cx.simulate_keystrokes("down");
    assert_eq!(grid.selected(&mut cx, 2, 3), vec![(1, 2)]);
}

/// `Escape` is the one way out that stages nothing, however much was typed.
#[gpui::test]
fn escape_throws_the_typing_away(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(0, 2, window, cx)
    });
    cx.simulate_input("rewritten");
    assert_eq!(grid.typed(&mut cx).as_deref(), Some("hererewritten"));

    cx.simulate_keystrokes("escape");
    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![],
        "escape staged what it was supposed to throw away"
    );
}

/// The focus going somewhere else commits, and — alone among the closes —
/// does not drag it back.
#[gpui::test]
fn the_focus_leaving_commits_without_taking_itself_back(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(20, 4, probe).editable(), cx);

    // gpui reports a focus change only while the window is active, and a
    // test window is inactive until it is told otherwise.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();

    grid.update_in(&mut cx, |grid, window, cx| {
        grid.begin_edit(2, 1, window, cx)
    });
    cx.simulate_input("!");

    // Somewhere else in the window, which for a grid on its own is the grid
    // itself — what matters is that the field is not it.
    cx.update(|window, cx| {
        let handle = grid.grid.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    });
    cx.run_until_parked();

    assert_eq!(grid.read(&mut cx, |grid| grid.editing()), None);
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 2,
            column: 1,
            value: EditValue::Text("value!".to_owned()),
        }]
    );
}

/// The markers are drawn from what the source says, and the grid asks it
/// only about the rows and cells it is drawing.
#[gpui::test]
fn the_markers_read_only_what_is_drawn(cx: &mut TestAppContext) {
    let probe = Rc::new(Probe::default());
    let (grid, mut cx) = open(Huge::new(500_000, 40, probe.clone()), cx);

    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert!(probe.marks.get() > 0, "nothing asked about the markers");
    assert!(
        probe.max_mark_row.get() < 60,
        "the marker of row {} was asked for",
        probe.max_mark_row.get()
    );

    // And the dirty marks are a question per *drawn* cell: hiding all but
    // two columns cuts the count to what two columns and a row marker
    // cost, rather than leaving it at what forty would.
    let before = probe.marks.get();
    for column in 2..40 {
        grid.update(&mut cx, |grid, cx| {
            grid.set_column_hidden(column, true, cx);
        });
    }
    probe.forget();
    grid.update(&mut cx, |grid, cx| grid.refresh(cx));
    assert!(
        probe.marks.get() * 3 < before,
        "hiding thirty-eight of forty columns left {} of {before} marks",
        probe.marks.get()
    );
}

/// A staged result draws its markers and its tints, and an unstaged one
/// draws neither — the whole of what the defaults buy a read-only source.
#[gpui::test]
fn a_row_is_marked_only_when_the_source_says_so(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);
    let palette = cx.update(|_, cx| rugpui::theme::theme(cx));

    assert_eq!(status_color(RowStatus::Unchanged, &palette), None);
    assert_eq!(
        status_color(RowStatus::Modified, &palette),
        Some(palette.accent)
    );
    assert_eq!(
        status_color(RowStatus::Inserted, &palette),
        Some(palette.success)
    );
    assert_eq!(
        status_color(RowStatus::Deleted, &palette),
        Some(palette.danger)
    );

    assert_eq!(
        grid.read(&mut cx, |grid| grid.source().row_status(0)),
        RowStatus::Unchanged
    );
    grid.update(&mut cx, |grid, cx| {
        let source = grid.source_mut(cx);
        source.status[0] = RowStatus::Deleted;
        source.dirty.push((1, 2));
    });
    assert_eq!(
        grid.read(&mut cx, |grid| grid.source().row_status(0)),
        RowStatus::Deleted
    );
    assert!(grid.read(&mut cx, |grid| grid.source().cell_dirty(1, 2)));
    assert!(!grid.read(&mut cx, |grid| grid.source().cell_dirty(1, 1)));
}
