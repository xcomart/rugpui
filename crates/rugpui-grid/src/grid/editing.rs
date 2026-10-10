//! Editing.

use super::*;

impl<S: GridSource> GridView<S> {
    /// The field the user is typing into, while there is one.
    ///
    /// For a host that wants to read the half-typed value — a live validation
    /// hint beside the grid, say. Nothing about the edit is settled until
    /// [`GridEvent::EditCommitted`] arrives.
    ///
    /// `None` unless the open editor is the *field*: a
    /// [`CellEditor::Choice`] or a [`CellEditor::Custom`] has no half-typed
    /// value to show, which is why picking a row out of a dropdown stages it
    /// there and then.
    pub fn editor(&self) -> Option<&Entity<TextInput>> {
        match &self.editing.as_ref()?.editor {
            OpenEditor::Field { input } => Some(input),
            OpenEditor::Choice { .. } | OpenEditor::Custom { .. } => None,
        }
    }

    /// Opens the inline editor over the cell at `row` and *source* `column`.
    ///
    /// `column` is a source column, the same numbering
    /// [`GridEvent::CellActivated`] hands out and [`GridSource::cell`] takes, so
    /// a host that answers an activation with this needs no translation. Answers
    /// whether the editor opened; it refuses when
    ///
    /// * the cell is not there,
    /// * its column is hidden — a field has to be drawn somewhere,
    /// * [`GridSource::cell_editable`] says no, which is the default and
    ///   therefore the answer for every source that has not opted in,
    /// * or the cell holds a [`GridCell::Lob`], whose body is not in the grid to
    ///   be seeded into a field or replaced from one.
    ///
    /// *Which* editor opens is [`GridSource::cell_editor`], asked once the cell
    /// has agreed to take an edit at all:
    ///
    /// * [`CellEditor::Text`] — the field, seeded with the cell's text; a null
    ///   cell seeds an empty one, so that the caret starts where typing starts.
    ///   What the emptiness *means* is remembered separately: leaving an empty
    ///   field on a cell that was null commits nothing, rather than quietly
    ///   turning `NULL` into `''`.
    /// * [`CellEditor::Choice`] — a dropdown, opened at once on the value the
    ///   cell holds. A row picked with the pointer stages immediately; the
    ///   arrows walk the list and `Enter` stages where they stopped.
    /// * [`CellEditor::Custom`] — the host's own element, handed a
    ///   [`CellEditorContext`] with the two ways out on it.
    ///
    /// Any editor already open is committed first, and the selection moves onto
    /// the cell — an editor is a strange place for the cursor not to be.
    pub fn begin_edit(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.ensure_layout();
        if row >= self.source.row_count() || column >= self.source.column_count() {
            return false;
        }
        let Some(display) = self.display_of(column) else {
            return false;
        };
        if !self.source.cell_editable(row, column) {
            return false;
        }
        let (seeded, was_null) = match self.source.cell(row, column) {
            // A cell the server is going to fill in seeds the same empty field
            // a null does, and for the same reason: leaving it as it was found
            // must commit nothing, or opening a `DEFAULT` cell and thinking
            // better of it would turn it into the empty string.
            GridCell::Null | GridCell::Default => (String::new(), true),
            GridCell::Text(text) => (text.to_string(), false),
            GridCell::Lob { .. } => return false,
        };

        // Asked only now, after the cell has been found and agreed to take an
        // edit at all — so a source may build its option list here rather than
        // keeping one for every cell of the result.
        let editor = self.source.cell_editor(row, column);

        self.commit_edit(cx);

        let cell = CellAddress::new(row, display);
        self.selection.replace(cell);
        self.reveal(cell);

        let (editor, handle) = match editor {
            CellEditor::Text => {
                let grid = cx.entity().downgrade();
                let content = seeded.clone();
                let input = cx.new(|cx| {
                    // `Enter` is the field's own action, bound in the field's
                    // own deeper key context, so the grid's `Activate` never
                    // sees it and this callback is the only way the keystroke
                    // comes back. It is handed the content because the field is
                    // mid-update while it runs and cannot be read out of the
                    // entity map.
                    let mut input = TextInput::new(cx).on_submit(move |typed, _window, cx| {
                        let typed = EditValue::Text(typed.to_string());
                        grid.update(cx, |grid, cx| grid.close_edit(Some(typed), true, cx))
                            .ok();
                    });
                    input.set_content(content, cx);
                    input
                });
                let handle = input.read(cx).focus_handle(cx);
                (OpenEditor::Field { input }, handle)
            }
            CellEditor::Choice { options, nullable } => {
                // The `NULL` row is prepended here rather than at every use, so
                // that the index the list hands back and the index the keyboard
                // walks are indices into one list.
                let rows: Vec<SharedString> = nullable
                    .then(|| SharedString::new_static(NULL_TEXT))
                    .into_iter()
                    .chain(options)
                    .collect();
                // The list opens on what the cell holds, so that the first
                // arrow steps away from the current value rather than from the
                // top of the list.
                let highlight = if was_null {
                    0
                } else {
                    rows.iter().position(|row| *row == seeded).unwrap_or(0)
                };
                let focus = cx.focus_handle();
                (
                    OpenEditor::Choice {
                        focus: focus.clone(),
                        rows,
                        nullable,
                        highlight,
                    },
                    focus,
                )
            }
            CellEditor::Custom(build) => {
                let focus = cx.focus_handle();
                (
                    OpenEditor::Custom {
                        focus: focus.clone(),
                        build,
                    },
                    focus,
                )
            }
        };

        let field = matches!(editor, OpenEditor::Field { .. });
        let blur = if field {
            cx.on_focus_out(&handle, window, |grid, _event, _window, cx| {
                // The focus has gone somewhere deliberate. Committing is right;
                // taking the focus back is not, which is the one close that
                // leaves `refocus` alone.
                let Some(typed) = grid.typed(cx) else {
                    return;
                };
                grid.close_edit(Some(EditValue::Text(typed)), false, cx);
            })
        } else {
            cx.on_focus_out(&handle, window, |grid, _event, _window, cx| {
                // Neither of the other two has anything half-finished in it: a
                // dropdown stages the moment a row is picked and a host's
                // editor stages when it says so, so the focus leaving is a
                // dismissal and stages nothing.
                if grid.editing.is_some() {
                    grid.close_edit(None, false, cx);
                }
            })
        };

        self.editing = Some(Editing {
            row,
            column,
            editor,
            seeded,
            was_null,
            settled: false,
            _blur: blur,
        });
        self.refocus = false;
        handle.focus(window, cx);
        cx.notify();
        true
    }

    /// Closes the editor, staging whatever is in it.
    ///
    /// What every gesture but `Escape` ends up in — see the module docs on why a
    /// close commits. Raises nothing when the field holds what the cell already
    /// held.
    pub fn commit_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        // A dropdown and a host's editor have nothing to hand over here: both
        // stage at the moment of the gesture, so everything that merely *closes*
        // one — a scroll, a sort, a column dragged out from under it — takes it
        // down with nothing staged.
        let typed = self.typed(cx).map(EditValue::Text);
        self.close_edit(typed, true, cx);
    }

    /// Closes the editor and throws away what was typed.
    ///
    /// `Escape`, and the only way back out of a field without staging anything.
    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.close_edit(None, true, cx);
    }

    /// What is in the field, while there is one.
    ///
    /// Reads the field out of the entity map, so it must not be called while the
    /// field itself is being updated — which is why the submit callback is
    /// handed its content instead of asking for it.
    pub(super) fn typed(&self, cx: &App) -> Option<String> {
        match &self.editing.as_ref()?.editor {
            OpenEditor::Field { input } => Some(input.read(cx).content().to_string()),
            OpenEditor::Choice { .. } | OpenEditor::Custom { .. } => None,
        }
    }

    /// Takes the editor down, raising [`GridEvent::EditCommitted`] when `typed`
    /// is something other than what the cell held.
    ///
    /// `refocus` is false for exactly one caller — the focus having left of its
    /// own accord — because taking the focus back from wherever the user just
    /// put it would be worse than the edit ending quietly.
    pub(super) fn close_edit(
        &mut self,
        value: Option<EditValue>,
        refocus: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        self.refocus = refocus;
        if let Some(value) = value
            && editing.changed(&value)
        {
            cx.emit(GridEvent::EditCommitted {
                row: editing.row,
                column: editing.column,
                value,
            });
        }
        cx.notify();
    }

    /// Moves the dropdown's highlight by `delta`, and says whether there was a
    /// dropdown to move.
    ///
    /// The arrows have to be caught here rather than left to [`Select`]'s own
    /// key handling: the focus is on the box the list hangs from and not on the
    /// trigger inside it, so the trigger never sees the keystroke — and the
    /// grid's own `MoveUp`/`MoveDown` would otherwise walk the selection out
    /// from under an open list. Moving the highlight rather than picking as it
    /// goes is the deliberate difference from the bare control: an arrow that
    /// staged every row it passed over would write three values on the way to
    /// the fourth.
    pub(super) fn step_choice(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(Editing {
            editor: OpenEditor::Choice {
                rows, highlight, ..
            },
            ..
        }) = self.editing.as_mut()
        else {
            return false;
        };
        let Some(last) = rows.len().checked_sub(1) else {
            return true;
        };
        *highlight = (*highlight as isize + delta).clamp(0, last as isize) as usize;
        cx.notify();
        true
    }

    /// Stages the dropdown's highlighted row, and says whether there was one.
    ///
    /// What `Enter` does over an open list, and the one keystroke that ends a
    /// choice: the pointer has no need of it, since clicking a row is already
    /// unambiguous.
    pub(super) fn commit_choice(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Editing {
            editor:
                OpenEditor::Choice {
                    rows,
                    nullable,
                    highlight,
                    ..
                },
            ..
        }) = self.editing.as_ref()
        else {
            return false;
        };
        let value = choice_value(rows, *nullable, *highlight);
        self.close_edit(value, true, cx);
        true
    }

    /// Commits the editor and opens the next — or previous — cell of the row
    /// that will take one.
    ///
    /// What `Tab` does. Stops at the ends of the row rather than wrapping onto
    /// the next one: a `Tab` that fell off the end and landed on a different
    /// row would be a keystroke that moved the edit somewhere the user was not
    /// looking. Nothing to move to leaves the commit standing and the editor
    /// closed, which is what `Tab` out of the last field of anything does.
    pub(super) fn step_edit(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((row, column)) = self.editing() else {
            return;
        };
        self.commit_edit(cx);
        let Some(display) = self.display_of(column) else {
            return;
        };
        let Some(next) = self.next_editable(row, display, forward) else {
            return;
        };
        self.begin_edit(row, next, window, cx);
    }

    /// The source column of the next cell of `row` that will take an edit,
    /// starting one display position beyond `from`.
    ///
    /// Walks display positions rather than source columns, so `Tab` follows the
    /// order the columns are drawn in and steps over the hidden ones — which
    /// have nowhere to put a field anyway.
    pub(super) fn next_editable(&self, row: usize, from: usize, forward: bool) -> Option<usize> {
        let range: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(from + 1..self.laid_out.len())
        } else {
            Box::new((0..from).rev())
        };
        range
            .map(|display| self.laid_out[display].column)
            .find(|&column| self.source.cell_editable(row, column))
    }

    /// Where source column `column` sits along the header, if it is showing.
    pub(super) fn display_of(&self, column: usize) -> Option<usize> {
        self.laid_out
            .iter()
            .position(|placed| placed.column == column)
    }
}
