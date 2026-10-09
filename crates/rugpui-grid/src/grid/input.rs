//! Input.

use super::*;

impl<S: GridSource> GridView<S> {
    /// Stretches the selection out to the cell at `row` and `column`.
    pub fn extend_selection(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(cell) = self.clamped(row, column) else {
            return;
        };
        self.selection.extend_to(cell);
        self.reveal(cell);
        cx.notify();
    }

    /// Picks a whole row, as a click on its row number does.
    pub fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        if row >= self.source.row_count() {
            return;
        }
        self.selection.replace_rows(row..=row, self.laid_out.len());
        self.scroll.scroll_to_item(row, ScrollStrategy::Top);
        cx.notify();
    }

    /// Picks everything.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        self.selection
            .select_all(self.source.row_count(), self.laid_out.len());
        cx.notify();
    }

    /// Drops the selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        cx.notify();
    }

    /// Writes the selection to the clipboard in `format`.
    ///
    /// Nothing selected writes nothing at all, rather than blanking the
    /// clipboard. See [`crate::copy`] for what each format does with a null.
    pub fn copy(&mut self, format: CopyFormat, cx: &mut Context<Self>) {
        self.ensure_layout();
        let columns = self.visible_column_indices();
        let table = self
            .insert_table
            .as_ref()
            .map_or(DEFAULT_INSERT_TABLE, |table| table.as_ref());
        let text = copy_payload(&self.source, &columns, &self.selection, format, table);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Brings `row` into view.
    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.scroll.scroll_to_item(row, ScrollStrategy::Top);
        cx.notify();
    }

    /// Which cell the inline editor is open over, as `(row, source column)`.
    ///
    /// `None` while nobody is typing, which is nearly always.
    pub fn editing(&self) -> Option<(usize, usize)> {
        self.editing
            .as_ref()
            .map(|editing| (editing.row, editing.column))
    }

    /// Moves the cursor by `rows` and `columns`, stretching the selection or
    /// replacing it.
    pub(super) fn step(
        &mut self,
        rows: isize,
        columns: isize,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        // Only the keys the field does not want reach here while one is open:
        // `Left` and `Right` are the field's, `Up` and `Down` are not, so an
        // arrow out of a field commits it and walks on, which is what a
        // spreadsheet does.
        self.commit_edit(cx);
        self.ensure_layout();
        let (last_row, last_column) = match (
            self.source.row_count().checked_sub(1),
            self.laid_out.len().checked_sub(1),
        ) {
            (Some(row), Some(column)) => (row, column),
            _ => return,
        };

        // Nothing picked yet: the first keystroke lands on the first cell rather
        // than one step away from it.
        let cell = match self.selection.cursor() {
            None => CellAddress::new(0, 0),
            Some(cursor) => CellAddress::new(
                offset(cursor.row, rows, last_row),
                offset(cursor.column, columns, last_column),
            ),
        };

        if extend {
            self.selection.extend_to(cell);
        } else {
            self.selection.replace(cell);
        }
        self.reveal(cell);
        cx.notify();
    }

    /// Moves the cursor to an absolute cell.
    pub(super) fn jump(&mut self, row: usize, column: usize, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(cell) = self.clamped(row, column) else {
            return;
        };
        self.selection.replace(cell);
        self.reveal(cell);
        cx.notify();
    }

    pub(super) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        // An open dropdown eats the vertical arrows; see `step_choice`.
        if self.step_choice(-1, cx) {
            return;
        }
        self.step(-1, 0, false, cx);
    }

    pub(super) fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.step_choice(1, cx) {
            return;
        }
        self.step(1, 0, false, cx);
    }

    pub(super) fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.step(0, -1, false, cx);
    }

    pub(super) fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.step(0, 1, false, cx);
    }

    pub(super) fn extend_up(&mut self, _: &ExtendUp, _: &mut Window, cx: &mut Context<Self>) {
        self.step(-1, 0, true, cx);
    }

    pub(super) fn extend_down(&mut self, _: &ExtendDown, _: &mut Window, cx: &mut Context<Self>) {
        self.step(1, 0, true, cx);
    }

    pub(super) fn extend_left(&mut self, _: &ExtendLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.step(0, -1, true, cx);
    }

    pub(super) fn extend_right(&mut self, _: &ExtendRight, _: &mut Window, cx: &mut Context<Self>) {
        self.step(0, 1, true, cx);
    }

    pub(super) fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let page = self.page() as isize;
        self.step(-page, 0, false, cx);
    }

    pub(super) fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let page = self.page() as isize;
        self.step(page, 0, false, cx);
    }

    pub(super) fn extend_page_up(
        &mut self,
        _: &ExtendPageUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.page() as isize;
        self.step(-page, 0, true, cx);
    }

    pub(super) fn extend_page_down(
        &mut self,
        _: &ExtendPageDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.page() as isize;
        self.step(page, 0, true, cx);
    }

    pub(super) fn move_row_start(
        &mut self,
        _: &MoveRowStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.selection.cursor().map_or(0, |cursor| cursor.row);
        self.jump(row, 0, cx);
    }

    pub(super) fn move_row_end(&mut self, _: &MoveRowEnd, _: &mut Window, cx: &mut Context<Self>) {
        let row = self.selection.cursor().map_or(0, |cursor| cursor.row);
        let column = self.laid_out.len().saturating_sub(1);
        self.jump(row, column, cx);
    }

    pub(super) fn move_first(&mut self, _: &MoveFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.jump(0, 0, cx);
    }

    pub(super) fn move_last(&mut self, _: &MoveLast, _: &mut Window, cx: &mut Context<Self>) {
        let row = self.source.row_count().saturating_sub(1);
        let column = self.laid_out.len().saturating_sub(1);
        self.jump(row, column, cx);
    }

    pub(super) fn select_everything(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all(cx);
    }

    pub(super) fn copy_selection(&mut self, _: &CopyCells, _: &mut Window, cx: &mut Context<Self>) {
        self.copy(CopyFormat::Tsv, cx);
    }

    pub(super) fn activate(&mut self, _: &Activate, _: &mut Window, cx: &mut Context<Self>) {
        // `Enter` over an open list picks the highlighted row, the way it does
        // in the field it stands in for. The grid's own `Activate` is what
        // reaches here because a `Select` has no submit action of its own.
        if self.commit_choice(cx) {
            return;
        }
        let Some(cursor) = self.selection.cursor() else {
            return;
        };
        let Some(placed) = self.laid_out.get(cursor.column) else {
            return;
        };
        cx.emit(GridEvent::CellActivated {
            row: cursor.row,
            column: placed.column,
        });
    }

    pub(super) fn cancel_editing(
        &mut self,
        _: &CancelEdit,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_edit(cx);
    }

    pub(super) fn edit_next(&mut self, _: &EditNext, window: &mut Window, cx: &mut Context<Self>) {
        self.step_edit(true, window, cx);
    }

    pub(super) fn edit_previous(
        &mut self,
        _: &EditPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_edit(false, window, cx);
    }

    /// What the pointer is over, worked out from the grid's own geometry.
    ///
    /// Done arithmetically rather than with a listener per cell: a cell that
    /// answers presses needs an id and a hitbox, and a screenful of them is
    /// several hundred of both, every frame, for a gesture that can be resolved
    /// from four numbers.
    pub(super) fn hit(&self, position: Point<Pixels>) -> Option<Hit> {
        let body = self.base_handle().bounds();
        if body.size.width <= px(0.) || !body.contains(&position) {
            return None;
        }

        let scrolled_by = f32::from(self.base_handle().offset().y);
        let local_x = f32::from(position.x - body.origin.x);
        let content_y = f32::from(position.y - body.origin.y) - scrolled_by;
        if content_y < 0. {
            return None;
        }

        let row = (content_y / ROW_HEIGHT) as usize;
        if row >= self.source.row_count() {
            return None;
        }
        if local_x < GUTTER_WIDTH {
            return Some(Hit::Gutter(row));
        }

        let x = local_x - GUTTER_WIDTH + self.h_offset;
        let display = self
            .laid_out
            .partition_point(|placed| placed.x + self.column_width(placed.column) <= x);
        let placed = self.laid_out.get(display)?;
        (x >= placed.x).then_some(Hit::Cell(CellAddress::new(row, display)))
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(hit) = self.hit(event.position) else {
            return;
        };
        self.focus_handle.focus(window, cx);

        let columns = self.laid_out.len();
        match hit {
            Hit::Gutter(row) => {
                if event.modifiers.shift {
                    // From the pivot, which a row selection puts on its top row
                    // — so a shift-click below the block grows it and one above
                    // it redraws from where the block started.
                    let anchor = self.selection.anchor().map_or(row, |cell| cell.row);
                    self.selection
                        .replace_rows(anchor.min(row)..=anchor.max(row), columns);
                } else if event.modifiers.secondary() {
                    self.selection.add_rows(row..=row, columns);
                } else {
                    self.selection.replace_rows(row..=row, columns);
                }
            }
            Hit::Cell(cell) => {
                if event.modifiers.shift {
                    self.selection.extend_to(cell);
                } else if event.modifiers.secondary() {
                    self.selection.add(cell);
                } else {
                    self.selection.replace(cell);
                }
                self.dragging = true;

                if event.click_count >= 2
                    && let Some(placed) = self.laid_out.get(cell.column)
                {
                    cx.emit(GridEvent::CellActivated {
                        row: cell.row,
                        column: placed.column,
                    });
                }
            }
        }
        cx.notify();
    }

    /// A right click on a column heading, from the heading itself or from its
    /// resize grip.
    ///
    /// Takes the focus — the menu's items act on the grid, so the keys should
    /// too afterwards — and leaves the selection exactly as it was: a header
    /// menu is about the column, and "hide this column" would be a strange
    /// thing to have just cleared the selection for.
    pub(super) fn on_header_menu(
        &mut self,
        column: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus_handle.focus(window, cx);
        cx.emit(GridEvent::ContextMenu {
            target: MenuTarget::Header { column },
            position: event.position,
        });
    }

    /// A right click in the body: move the selection if the press fell outside
    /// it, then hand the gesture to the host.
    ///
    /// The selection rule is the one every grid and file list uses, and the one
    /// §7.8 states: a press *inside* what is picked leaves it alone — otherwise
    /// "copy" on a block of a hundred cells would copy one — and a press
    /// outside picks what was pressed, so the menu is never about something the
    /// user cannot see. A press in the gutter picks the whole row, exactly as a
    /// left one does.
    ///
    /// Nothing else happens: no drag is started, and no
    /// [`GridEvent::CellActivated`] is raised however many times the button is
    /// clicked.
    pub(super) fn on_right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_edit(cx);
        self.ensure_layout();
        let Some(hit) = self.hit(event.position) else {
            return;
        };
        self.focus_handle.focus(window, cx);
        cx.stop_propagation();

        let columns = self.laid_out.len();
        match hit {
            Hit::Gutter(row) => {
                let picked = (0..columns).any(|column| self.selection.contains(row, column));
                if !picked {
                    self.selection.replace_rows(row..=row, columns);
                }
            }
            Hit::Cell(cell) => {
                if !self.selection.contains(cell.row, cell.column) {
                    self.selection.replace(cell);
                }
            }
        }

        cx.emit(GridEvent::ContextMenu {
            target: MenuTarget::Cell,
            position: event.position,
        });
        cx.notify();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(resize) = self.resizing {
            let width = resize.width + f32::from(event.position.x - resize.from);
            self.set_column_width(resize.column, width, cx);
            return;
        }
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(Hit::Cell(cell)) = self.hit(event.position) {
            self.selection.extend_to(cell);
            cx.notify();
        }
    }

    pub(super) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging = false;
        self.resizing = None;
        if let Some(epoch) = self.v_bar.release() {
            hide_later(epoch, cx, |grid: &mut Self| Some(&mut grid.v_bar));
        }
        if let Some(epoch) = self.h_bar.release() {
            hide_later(epoch, cx, |grid: &mut Self| Some(&mut grid.h_bar));
        }
        cx.notify();
    }

    pub(super) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(ROW_HEIGHT));
        // A plain mouse has no sideways wheel, so `Shift` folds the vertical one
        // onto the horizontal axis — the convention every other scrolling
        // surface uses.
        let sideways = if delta.x.is_zero() && event.modifiers.shift {
            delta.y
        } else {
            delta.x
        };
        if sideways.is_zero() {
            return;
        }
        self.set_h_offset(self.h_offset - f32::from(sideways), cx);
    }

    /// The scroll container behind the list, which is what the vertical bar
    /// measures and what the pointer arithmetic is done against.
    pub(super) fn base_handle(&self) -> ScrollHandle {
        self.scroll.0.borrow().base_handle.clone()
    }

    /// The vertical bar as it stands this frame.
    pub(super) fn vertical_bar(&self) -> Scrollbar {
        Scrollbar::for_handle(
            self.v_bar_id.clone(),
            ScrollbarAxis::Vertical,
            &self.base_handle(),
        )
        .fade(self.v_bar.fade())
    }

    /// The horizontal bar as it stands this frame.
    ///
    /// Built from the grid's own numbers rather than from a scroll handle,
    /// because the columns are not in a scroll container: its track is the
    /// content area, which is the body less the gutter.
    pub(super) fn horizontal_bar(&self) -> Scrollbar {
        let body = self.base_handle().bounds();
        let track = Bounds::new(
            body.origin + point(px(GUTTER_WIDTH), px(0.)),
            size(
                (body.size.width - px(GUTTER_WIDTH)).max(px(0.)),
                body.size.height,
            ),
        );
        Scrollbar::new(
            self.h_bar_id.clone(),
            ScrollbarAxis::Horizontal,
            track,
            self.viewport_width,
            self.max_h_offset(),
            self.h_offset,
        )
        .fade(self.h_bar.fade())
    }

    /// The state of whichever bar rides `axis`.
    pub(super) fn bar_mut(&mut self, axis: ScrollbarAxis) -> &mut ScrollbarState {
        match axis {
            ScrollbarAxis::Vertical => &mut self.v_bar,
            ScrollbarAxis::Horizontal => &mut self.h_bar,
        }
    }

    /// Puts a bar up while the pointer rests on the edge it rides, and starts
    /// it going the moment the pointer leaves.
    pub(super) fn hover_bar(&mut self, axis: ScrollbarAxis, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.bar_mut(axis).hover_enter() {
                cx.notify();
            }
            return;
        }

        if let Some(epoch) = self.bar_mut(axis).hover_leave() {
            hide_now(self, epoch, cx, move |grid: &mut Self| {
                Some(grid.bar_mut(axis))
            });
        }
    }

    /// Draws the fixed header band.
    pub(super) fn render_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.visible_columns();
        let start = self
            .laid_out
            .get(visible.start)
            .map_or(0., |placed| placed.x);

        let cells: Vec<AnyElement> = visible
            .map(|display| self.render_heading(display, theme, cx))
            .collect();

        div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .w_full()
            .bg(theme.grid_header)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("grid-corner")
                    .flex_none()
                    .w(px(GUTTER_WIDTH))
                    .h_full()
                    .border_r_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .on_click(cx.listener(|grid, _: &ClickEvent, window, cx| {
                        grid.focus_handle.focus(window, cx);
                        grid.select_all(cx);
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
}
