//! Render.

use super::*;

/// What the row at `index` of an open dropdown stages, or `None` when the list
/// is empty.
///
/// The one place the `NULL` row is told from a value row, and it is told by
/// *position* rather than by text: a column whose values include the string
/// `NULL` would otherwise clear itself when the user picked the value they
/// meant.
pub(super) fn choice_value(
    rows: &[SharedString],
    nullable: bool,
    index: usize,
) -> Option<EditValue> {
    let label = rows.get(index)?;
    Some(if nullable && index == 0 {
        EditValue::Null
    } else {
        EditValue::Text(label.to_string())
    })
}

/// The colour a row's marker is drawn in, or `None` for a row nothing has been
/// staged against.
///
/// Derived from the palette rather than added to it, exactly as the grid's own
/// tokens are (design notes, §7.2): a theme file written by hand knows
/// nothing about staged edits and gains nothing it has to know. The three
/// meanings map onto the three the palette already carries — a change is the
/// accent, something new is a success, something going is a danger — so a theme
/// that made its danger colour green would mark deletions green, which is what
/// its author asked for.
pub(super) fn status_color(status: RowStatus, theme: &Theme) -> Option<Hsla> {
    match status {
        RowStatus::Unchanged => None,
        RowStatus::Modified => Some(theme.accent),
        RowStatus::Inserted => Some(theme.success),
        RowStatus::Deleted => Some(theme.danger),
    }
}

impl<S: GridSource> Render for GridView<S> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_layout();

        // Before anything is built, so that the header and the rows below are
        // laid out at the widths this frame decided rather than the last one's.
        // See `fit_columns` for why measuring can only happen here.
        if self.fit_columns(window) {
            self.relayout();
            self.clamp_h_offset();
        }

        // A result the host swapped out from under an open field: the cell the
        // field was over is not there any more, so there is nothing to commit
        // it *to*. Every route a host actually takes to swap a result commits
        // first; this is the backstop for the one it does not.
        if let Some((row, column)) = self.editing()
            && (row >= self.source.row_count() || self.display_of(column).is_none())
        {
            self.close_edit(None, true, cx);
        }
        // From here on the field's row can be judged against where the list
        // actually is; see the flag's docs for what the first frame is spared.
        if let Some(editing) = self.editing.as_mut() {
            editing.settled = true;
        }
        // Closing dropped the field, and the focus was in it. Done here because
        // this is the first place after a close that has a window to hand — see
        // the field's docs.
        if std::mem::take(&mut self.refocus) {
            self.focus_handle.focus(window, cx);
        }

        // Built before the tree it hangs off, because a custom editor is the
        // host's element and building it wants both the window and the app —
        // neither of which can be handed round inside the builder chain below.
        let editor = self.render_editor(window, cx);

        let palette = theme(cx);
        let rows = self.source.row_count();
        let grid = cx.entity();

        // Both bars, wired as every scrolling surface in the app wires one:
        // notice the surface moved, and arm the expiry from inside the draw that
        // noticed.
        if let Some(epoch) = self
            .v_bar
            .moved(scrolled(&self.base_handle(), ScrollbarAxis::Vertical))
        {
            hide_later(epoch, cx, |grid: &mut Self| Some(&mut grid.v_bar));
        }
        if let Some(epoch) = self.h_bar.moved(self.h_offset) {
            hide_later(epoch, cx, |grid: &mut Self| Some(&mut grid.h_bar));
        }

        let measure = {
            let grid = grid.clone();
            canvas(
                move |bounds, _window, cx| {
                    grid.update(cx, |grid, cx| grid.measured(bounds.size, cx));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };

        let mut list = uniform_list("grid-rows", rows, move |range, window, cx| {
            grid.update(cx, |grid, cx| {
                grid.note_visible(range.clone(), cx);
                let palette = theme(cx);
                let mut built = Vec::with_capacity(range.len());
                for row in range {
                    built.push(grid.render_row(row, &palette, window, cx));
                }
                built
            })
        })
        .track_scroll(&self.scroll)
        .size_full();
        // Keeps the sideways wheel that pans the columns from also dragging the
        // rows up and down — the grid is the one surface where both axes are
        // driven at once, so folding one delta onto the other is immediately
        // visible. Spelled against the interactivity rather than through
        // `restrict_scroll_to_axis()` because that method belongs to gpui's
        // *stateful* half of the interactive traits, which a `UniformList` —
        // scrolled by a handle of its own rather than by an element id — does
        // not implement. The flag itself lives on the shared style the same
        // paint code reads for both, so the effect is identical.
        list.interactivity().base_style.restrict_scroll_to_axis = Some(true);

        let body = div()
            .relative()
            .flex_grow_1()
            .w_full()
            .overflow_hidden()
            .child(measure)
            .child(list)
            .children(
                self.vertical_bar()
                    .on_hover(cx.listener(|grid, hovered: &bool, _window, cx| {
                        grid.hover_bar(ScrollbarAxis::Vertical, *hovered, cx);
                    }))
                    .render(&palette),
            )
            .child(
                // The horizontal thumb rides the content area rather than the
                // whole body, so its box has to be that area and not the body:
                // `Scrollbar::render` places the thumb against its parent.
                div()
                    .absolute()
                    .left(px(GUTTER_WIDTH))
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .children(
                        self.horizontal_bar()
                            .on_hover(cx.listener(|grid, hovered: &bool, _window, cx| {
                                grid.hover_bar(ScrollbarAxis::Horizontal, *hovered, cx);
                            }))
                            .render(&palette),
                    ),
            );

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            // Nothing, while the window is translucent. The pane behind the grid
            // already tints these same pixels with the very same colour, so an
            // opaque fill here would hide the blur and a tinted one would
            // saturate the surface alpha back to opaque; see
            // `app_settings::window_tint`. The header, the row stripes and the
            // selection go on painting — they are accents over the background,
            // not the background.
            .when(!window_translucent(cx), |grid| grid.bg(palette.background))
            .text_size(px(TEXT_SIZE))
            .text_color(palette.text)
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::extend_up))
            .on_action(cx.listener(Self::extend_down))
            .on_action(cx.listener(Self::extend_left))
            .on_action(cx.listener(Self::extend_right))
            .on_action(cx.listener(Self::move_row_start))
            .on_action(cx.listener(Self::move_row_end))
            .on_action(cx.listener(Self::move_first))
            .on_action(cx.listener(Self::move_last))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::extend_page_up))
            .on_action(cx.listener(Self::extend_page_down))
            .on_action(cx.listener(Self::select_everything))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::activate))
            .on_action(cx.listener(Self::cancel_editing))
            .on_action(cx.listener(Self::edit_next))
            .on_action(cx.listener(Self::edit_previous))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_drag_move::<DraggedThumb>(cx.listener(
                |grid, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                    if let Some(progress) = grid.vertical_bar().dragged(event, cx) {
                        grid.v_bar.hold();
                        scroll_to(&grid.base_handle(), ScrollbarAxis::Vertical, progress);
                        cx.notify();
                    }
                    if let Some(progress) = grid.horizontal_bar().dragged(event, cx) {
                        grid.h_bar.hold();
                        let offset = grid.max_h_offset() * progress;
                        grid.set_h_offset(offset, cx);
                    }
                },
            ))
            // Both halves: a thumb dragged off the end of its track, or a
            // selection dragged out of the window, lets go with the pointer
            // outside, which only the second sees.
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(self.render_header(&palette, cx))
                    .child(body),
            )
            // Last, and absolutely positioned, so it is painted over the rows
            // rather than between them.
            .children(editor)
    }
}

impl<S: GridSource> GridView<S> {
    /// Draws one column heading, with its sort marker and its resize grip.
    pub(super) fn render_heading(
        &self,
        display: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let placed = self.laid_out[display];
        let column = self.source.column(placed.column);
        let marker = self.sort.and_then(|(sorted, direction)| {
            (sorted == placed.column).then_some(match direction {
                SortDirection::Ascending => SORT_ASCENDING,
                SortDirection::Descending => SORT_DESCENDING,
            })
        });
        let source_column = placed.column;

        div()
            .id(ElementId::from(("grid-heading", display)))
            .relative()
            .flex_none()
            .w(px(self.column_width(source_column)))
            .h_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(HEADING_GAP))
            .px(px(CELL_PADDING))
            .border_r_1()
            .border_color(theme.border)
            .cursor_pointer()
            // The one thing the primary key gets: its own colour, on the header
            // and nowhere else. A key icon would need a font this layer does not
            // pick.
            .text_color(if column.primary_key {
                theme.grid_pk
            } else {
                theme.text
            })
            .on_click(cx.listener(move |grid, _: &ClickEvent, window, cx| {
                grid.focus_handle.focus(window, cx);
                grid.toggle_sort(source_column, cx);
            }))
            // A right click on a heading is a menu about that column and does
            // not re-sort it, so it does not go through `on_click`. The header
            // band is its own element tree above the body, which the body's
            // arithmetic hit test does not cover — hence a listener here rather
            // than another branch in `hit`.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |grid, event: &MouseDownEvent, window, cx| {
                    grid.on_header_menu(source_column, event, window, cx);
                }),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .when(column.align == GridColumnAlign::Right, |label| {
                        label.text_right()
                    })
                    .child(SharedString::from(column.name.to_string())),
            )
            .children(marker.map(|marker| {
                div()
                    .flex_none()
                    .text_size(px(SORT_MARKER_SIZE))
                    .text_color(theme.accent)
                    .child(marker)
            }))
            .child(
                div()
                    .id(ElementId::from(("grid-grip", display)))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(-GRIP_WIDTH / 2.))
                    .w(px(GRIP_WIDTH))
                    // Occluding is what keeps the press off the heading
                    // underneath, so that grabbing the edge of a column does not
                    // also re-sort it.
                    .occlude()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |grid, event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            if event.click_count >= 2 {
                                grid.autofit_column(source_column, cx);
                            } else {
                                grid.resizing = Some(Resize {
                                    column: source_column,
                                    from: event.position.x,
                                    width: grid.column_width(source_column),
                                });
                            }
                        }),
                    )
                    // Occluding keeps the heading underneath from seeing the
                    // press at all, so the grip has to raise the menu itself —
                    // otherwise the last few pixels of every heading would be
                    // the one part of the header with no menu.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |grid, event: &MouseDownEvent, window, cx| {
                            grid.on_header_menu(source_column, event, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Draws one body row: the number in the gutter, and the strip of cells the
    /// content area can see.
    pub(super) fn render_row(
        &self,
        row: usize,
        theme: &Theme,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let visible = self.visible_columns();
        let start = self
            .laid_out
            .get(visible.start)
            .map_or(0., |placed| placed.x);
        // Asked once for the whole row, here, rather than once per cell: the
        // marker is the row's and the cells only need to know whether they are
        // being struck through.
        let status = self.source.row_status(row);
        let marker = status_color(status, theme);
        // A loop rather than a `map`, because the source may want the window
        // and the app to draw a cell of its own and a closure cannot hand both
        // round.
        let mut cells: Vec<AnyElement> = Vec::with_capacity(visible.len());
        for display in visible {
            cells.push(self.render_cell(row, display, status, theme, window, cx));
        }

        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(ROW_HEIGHT))
            .w_full()
            // Zebra striping is a hint and nothing more; see the token's docs.
            .when(row % 2 == 1, |stripe| stripe.bg(theme.grid_row_alt))
            // A row that is going or that was never there is tinted whole,
            // because the change is the *row* and not any value in it. Weakly:
            // a wash across the full width at the strength a single dirty cell
            // is tinted at would read as a change of theme.
            .when_some(
                match status {
                    RowStatus::Inserted | RowStatus::Deleted => marker,
                    _ => None,
                },
                |row, colour| row.bg(colour.opacity(ROW_TINT)),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(GUTTER_WIDTH))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px(px(CELL_PADDING))
                    .bg(theme.grid_header)
                    .border_r_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_color(theme.text_muted)
                    .child(SharedString::from((row + 1).to_string()))
                    // The whole of the row marker: a bar on the outer edge of
                    // the gutter, in the colour the status derives from. It is
                    // on the edge rather than beside the number so that a
                    // column of them can be read down at a glance, and it is
                    // three pixels wide so that the number it shares the gutter
                    // with is still the thing being read.
                    .children(marker.map(|colour| {
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(STATUS_WIDTH))
                            .bg(colour)
                    })),
            )
            .child(
                div()
                    .relative()
                    .flex_grow_1()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .h_full()
                            .left(px(start - self.h_offset))
                            .flex()
                            .flex_row()
                            .children(cells),
                    ),
            )
            .into_any_element()
    }

    /// Draws one cell.
    ///
    /// Plain divs with no id and no listeners: the whole of the pointer
    /// behaviour is [`GridView::hit`], so a cell is only a box with something in
    /// it.
    ///
    /// The source gets first refusal through [`GridSource::render_cell`]; what
    /// it draws goes into the same box the grid's own text would have gone
    /// into, minus the padding and the alignment, which belong to text and not
    /// to a badge or a bar. Everything *around* the content — the selection
    /// background, the dirty tint, the cursor outline — is painted here either
    /// way, so a cell the host drew is picked and marked exactly as a plain one
    /// is.
    pub(super) fn render_cell(
        &self,
        row: usize,
        display: usize,
        status: RowStatus,
        theme: &Theme,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let placed = self.laid_out[display];
        let column = self.source.column(placed.column);
        let align = column.align;
        let width = self.column_width(placed.column);
        let dirty = self.source.cell_dirty(row, placed.column);
        let selected = self.selection.contains(row, display);
        let cursor = self.selection.cursor() == Some(CellAddress::new(row, display));

        let info = CellInfo {
            kind: column.kind,
            selected,
            dirty,
            editing: self
                .editing
                .as_ref()
                .is_some_and(|editing| editing.row == row && editing.column == placed.column),
            width: px(width),
            height: px(ROW_HEIGHT),
            theme,
        };
        let custom = self
            .source
            .render_cell(row, placed.column, &info, window, cx);
        // Only asked for when the host did not draw the cell itself: the label
        // allocates a `SharedString` per cell, and a cell nobody is going to
        // draw text into has no use for one.
        let label = custom
            .is_none()
            .then(|| cell_label(&self.source.cell(row, placed.column)));

        div()
            .relative()
            .flex_none()
            .w(px(width))
            .h_full()
            .flex()
            .items_center()
            // A custom element is given the bare box: it is handed the same two
            // numbers in `CellInfo` and lines its own content up, which is what
            // lets a bar run from edge to edge.
            .when(custom.is_none(), |cell| {
                cell.px(px(CELL_PADDING))
                    .when(align == GridColumnAlign::Right, |cell| cell.justify_end())
            })
            .when(custom.is_some(), |cell| cell.overflow_hidden())
            .border_r_1()
            .border_b_1()
            .border_color(theme.border)
            .when(selected, |cell| cell.bg(theme.grid_selection))
            .when(label.as_ref().is_some_and(|label| label.muted), |cell| {
                cell.text_color(theme.grid_null)
            })
            // A child rather than a background, for the same reason the cursor
            // outline is one: the background is the selection's, and a dirty
            // cell that stopped looking dirty the moment it was picked would
            // hide the thing the user is about to copy or revert. Drawn before
            // the text so the text sits on top of it.
            .when(dirty, |cell| {
                cell.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .bg(theme.accent.opacity(DIRTY_TINT)),
                )
            })
            .children(custom)
            .children(label.map(|label| {
                div()
                    .truncate()
                    // A deleted row is still shown in its place — see
                    // `RowStatus::Deleted` — so its values need to say that
                    // they are on their way out rather than merely that
                    // something happened to the row.
                    .when(status == RowStatus::Deleted, |text| text.line_through())
                    .child(label.text)
            }))
            // The cursor outline is a child rather than a border, so that the
            // cell it is on stays exactly as wide as the others and the text
            // under it does not shift by a pixel as the cursor arrives.
            .when(cursor, |cell| {
                cell.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .border_1()
                        .border_color(theme.accent),
                )
            })
            .into_any_element()
    }

    /// Draws the inline editor over the cell it was opened on.
    ///
    /// The only place in the crate that turns a cell address back into a
    /// rectangle, and it does it from the four numbers the grid already keeps:
    /// `laid_out` for the column's left edge, `h_offset` for how far the strip
    /// has slid, `ROW_HEIGHT` for the row's top and the list's own scroll offset
    /// for where that row has been carried to. The same arithmetic
    /// [`GridView::hit`] runs backwards, which is what makes a field land on
    /// exactly the cell a click would have found.
    ///
    /// Everything is recomputed per frame rather than remembered, so a resize, a
    /// scroll or a column dragged out from under the field moves the field with
    /// it instead of stranding it.
    pub(super) fn render_editor(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editing = self.editing.as_ref()?;
        let display = self.display_of(editing.column)?;
        let placed = self.laid_out[display];
        let scrolled_by = f32::from(self.base_handle().offset().y);
        let width = px(self.column_width(editing.column));

        let field = div()
            .key_context(EDITOR_KEY_CONTEXT)
            .absolute()
            .left(px(placed.x - self.h_offset))
            // Centred on the row rather than fitted into it: the field is
            // taller than a row and squeezing it would clip its own border.
            .top(px(
                editing.row as f32 * ROW_HEIGHT + scrolled_by - (EDITOR_HEIGHT - ROW_HEIGHT) / 2.
            ))
            .w(width)
            // Without this the grid's own arithmetic hit test would see every
            // press meant for the field, move the selection and — since moving
            // the selection commits — close the field the user was aiming at.
            .occlude();

        let field = match &editing.editor {
            OpenEditor::Field { input } => field.child(input.clone()),
            OpenEditor::Choice {
                focus,
                rows,
                nullable,
                highlight,
            } => {
                let grid = cx.entity().downgrade();
                let nullable = *nullable;
                let picked = rows.get(*highlight).cloned();
                let rows = rows.clone();
                field
                    // Focused rather than left to the trigger, so that `Escape`
                    // lands in `EDITOR_KEY_CONTEXT` and the arrows reach
                    // `step_choice` instead of walking the selection.
                    .track_focus(focus)
                    .child(
                        Select::new("grid-cell-choice")
                            .options(rows.clone())
                            .selected(picked)
                            // Open from the first frame: the dropdown *is* the
                            // editor, so a trigger the user had to click again
                            // would be one gesture too many.
                            .open(true)
                            .width(width)
                            .on_select({
                                let grid = grid.clone();
                                move |index, _label, _window, cx| {
                                    let value = choice_value(&rows, nullable, index);
                                    grid.update(cx, |grid, cx| grid.close_edit(value, true, cx))
                                        .ok();
                                }
                            })
                            .on_open_change(move |open, _window, cx| {
                                // The only way this arrives with `false` and an
                                // editor still open is a press outside the
                                // list, which is a dismissal.
                                if !open {
                                    grid.update(cx, |grid, cx| grid.close_edit(None, true, cx))
                                        .ok();
                                }
                            }),
                    )
            }
            OpenEditor::Custom { focus, build } => {
                let grid = cx.entity().downgrade();
                let context = CellEditorContext {
                    row: editing.row,
                    column: editing.column,
                    seeded: editing.seeded.clone(),
                    was_null: editing.was_null,
                    width,
                    height: px(EDITOR_HEIGHT),
                    commit: {
                        let grid = grid.clone();
                        Rc::new(move |value, _window: &mut Window, cx: &mut App| {
                            grid.update(cx, |grid, cx| grid.close_edit(Some(value), true, cx))
                                .ok();
                        })
                    },
                    cancel: Rc::new(move |_window: &mut Window, cx: &mut App| {
                        grid.update(cx, |grid, cx| grid.close_edit(None, true, cx))
                            .ok();
                    }),
                };
                let build = build.clone();
                // Focused here too, so that a host element which takes no focus
                // of its own is still dismissible with `Escape`; one that takes
                // the focus takes it *inside* this box, which keeps the handle
                // in the focus path and the subscription quiet.
                let element = build(&context, window, cx);
                field.track_focus(focus).child(element)
            }
        };

        Some(
            // Clipped to the content area, so a field on a column half off the
            // right-hand edge is half drawn rather than painted over the
            // gutter and the scrollbar.
            div()
                .absolute()
                .left(px(GUTTER_WIDTH))
                .top(px(HEADER_HEIGHT))
                .right_0()
                .bottom_0()
                .overflow_hidden()
                .child(field)
                .into_any_element(),
        )
    }
}
