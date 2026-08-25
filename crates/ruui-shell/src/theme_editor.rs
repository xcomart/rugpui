//! Editing one palette, colour by colour.
//!
//! # Where the editor is drawn
//!
//! Not as a modal of its own. A host's settings dialog is already a modal, and
//! stacking a second one on top of it would leave the form underneath rendered
//! — which is to say still in the window's tab ring, so `Tab` would walk out of
//! the editor and into controls nobody can see. The settings dialog therefore
//! swaps its *body* for this view while an editor is open: one modal, one set
//! of tab stops, and `Escape` has a single obvious meaning at every moment. The
//! view returned by [`ThemeEditor`]'s `Render` is consequently a plain panel,
//! not a dialog; the frame around it belongs to the settings dialog.
//!
//! # What it edits
//!
//! One component for every catalogue, because catalogues differ only in which
//! slots they carry: a chrome theme is a name, a dark/light flag, eleven
//! required colours and five optional grid ones, and an editor theme is a name,
//! a dark/light flag and nineteen required token colours. Everything else — the
//! hex fields, the live preview, the refusal of a malformed colour, saving
//! under an id that never changes — is the same work, and
//! [`ThemeCatalog`](crate::ThemeCatalog) is the one place they part ways.
//!
//! # Automatic slots
//!
//! A format may let a slot be left out of the file, in which case the loader
//! derives it from the rest of the palette; that is what lets a theme written
//! against an older, shorter format keep loading. The editor has to be able to
//! say which of the two a slot is in, so an *empty* field means "derive it":
//! the swatch then shows the derived colour, the placeholder spells its hex
//! out, and a button beside the field puts a slot that has been given a colour
//! back to automatic. A required slot has neither.

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Div, DragMoveEvent, Entity, EventEmitter, FocusHandle, Focusable,
    Hsla, IntoElement, MouseButton, MouseUpEvent, Render, ScrollHandle, SharedString, Window, div,
    prelude::*, px,
};
use ruui::{
    Button, ButtonVariant, Checkbox, DraggedThumb, Scrollbar, ScrollbarAxis, ScrollbarState,
    TextInput, ThemeFile, form_row, hide_later, hide_now, parse_hex, scroll_to, scrolled, theme,
    to_hex,
};

use crate::catalog::{CatalogFile, Slot, ThemeCatalog, ui_colors, valid_hex};
use crate::inject::{label, text};

/// Element id of the editor's overlay scroll indicator.
const SCROLLBAR_ID: &str = "theme-editor-scrollbar";

/// Height at which the editor's field list starts scrolling.
///
/// The same cap a settings form uses, so the modal keeps its size as the dialog
/// swaps one body for the other.
const BODY_MAX_HEIGHT: f32 = 520.;

/// Colour fields per row.
///
/// Two, for every catalogue: at a dialog's width a row of two leaves each label
/// enough room to be read in every language.
const FIELD_COLUMNS: usize = 2;

/// Width of a colour field's label, in pixels.
const LABEL_WIDTH: f32 = 118.;

/// Side of the swatch drawn beside a colour field.
const SWATCH_SIZE: f32 = 26.;

/// Tab order inside the editor, spaced so slots can be inserted later.
///
/// A ring of its own rather than a continuation of the settings form's: while
/// the editor is open the form is not rendered at all, so there is nothing for
/// these indices to collide with.
pub mod tab {
    /// The name field.
    pub const NAME: isize = 10;
    /// The dark/light checkbox.
    pub const DARK: isize = 20;
    /// The first colour field; the rest follow two apart, because an optional
    /// slot puts its "automatic" button in the odd index behind its field.
    pub const FIRST_COLOR: isize = 30;
    /// Cancel. Far enough past the colours that no catalogue can reach it.
    pub const CANCEL: isize = 900;
    /// Save.
    pub const SAVE: isize = 910;
}

/// One editable colour: what it is called, and what has been typed into it.
struct ColorField {
    /// The slot the field stands for.
    slot: Slot,
    /// The field itself.
    input: Entity<TextInput>,
}

/// Emitted by [`ThemeEditor`] when the user is done with it.
pub enum ThemeEditorEvent {
    /// The entry has been written and the catalogue reloaded. The host has to
    /// repaint whatever was already wearing it.
    Saved,
    /// The user backed out; nothing was written.
    Cancelled,
}

/// Editor for one entry of one catalogue.
///
/// Built with [`ThemeEditor::new`] from the file that is to be edited, rendered
/// as the body of a settings dialog, and finished by one of
/// [`ThemeEditorEvent`]'s two variants. The id it saves under is fixed at
/// construction and never follows the name: renaming a palette must not orphan
/// the settings entry that selected it.
pub struct ThemeEditor {
    /// Which catalogue the entry belongs to.
    catalog: Arc<dyn ThemeCatalog>,
    /// The id it is saved under, from construction to save.
    id: String,
    /// The name, which is the only thing about it that is free text.
    name_input: Entity<TextInput>,
    /// Whether the palette is a dark one.
    dark: bool,
    /// One field per colour slot, in the catalogue's own order.
    fields: Vec<ColorField>,
    /// Why the last save did not go through, if it did not.
    status: Option<SharedString>,
    /// Focus of the editor root; the anchor the host's `Escape` handler sits
    /// on.
    focus_handle: FocusHandle,
    /// Whether focus should move into the name field on the next render.
    pending_focus: bool,
    /// Scroll position of the field list.
    scroll: ScrollHandle,
    /// Whether the field list's overlay scroll indicator is on screen.
    scrollbar: ScrollbarState,
}

impl ThemeEditor {
    /// Builds an editor over `file`, which will be saved back under `id`.
    pub fn new(
        catalog: Arc<dyn ThemeCatalog>,
        id: impl Into<String>,
        file: &CatalogFile,
        cx: &mut Context<Self>,
    ) -> Self {
        let (values, dark) = catalog.values_of(file);
        let name = catalog.name_of(file);

        let name_input = cx.new(|cx| {
            let mut input = TextInput::new(cx)
                .tab_index(tab::NAME)
                .context_menu(crate::input_menu_labels);
            input.set_content(name, cx);
            input
        });
        // The name is not validated, but it *is* previewed, so the editor has
        // to hear about it changing just as it hears about the colours.
        cx.observe(&name_input, |_editor, _input, cx| cx.notify())
            .detach();

        let slots = catalog.slots();
        let mut fields = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            let value = values.get(index).cloned().unwrap_or_default();
            // Marked as it opens, not only once it is typed into: a file edited
            // by hand can arrive with a slot that is not a colour, and the
            // editor is exactly where that has to be visible.
            let valid = valid_hex(&value, slot.alpha, slot.optional);
            let input = cx.new(|cx| {
                let mut input = TextInput::new(cx)
                    .placeholder("#000000")
                    .tab_index(tab::FIRST_COLOR + 2 * index as isize)
                    .context_menu(crate::input_menu_labels);
                input.set_content(value, cx);
                input.set_invalid(!valid, cx);
                input
            });
            // gpui does not re-render a parent when a child entity notifies, so
            // without this the live preview would only follow the typing at the
            // next unrelated repaint — and the refusal of a malformed colour
            // would never appear at all.
            cx.observe(&input, |editor, _input, cx| editor.revalidate(cx))
                .detach();
            fields.push(ColorField { slot: *slot, input });
        }

        Self {
            catalog,
            id: id.into(),
            name_input,
            dark,
            fields,
            status: None,
            focus_handle: cx.focus_handle(),
            pending_focus: true,
            scroll: ScrollHandle::new(),
            scrollbar: ScrollbarState::new(),
        }
    }

    /// Heading the host draws over the editor.
    pub fn title(&self, cx: &App) -> SharedString {
        label(cx, self.catalog.kind_label_key())
    }

    /// Discards the edits and tells the host to put its own body back.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(ThemeEditorEvent::Cancelled);
    }

    /// Re-marks every field that does not hold a colour, and repaints.
    fn revalidate(&mut self, cx: &mut Context<Self>) {
        for field in &self.fields {
            let valid = valid_hex(
                field.input.read(cx).content(),
                field.slot.alpha,
                field.slot.optional,
            );
            field
                .input
                .update(cx, |input, cx| input.set_invalid(!valid, cx));
        }
        cx.notify();
    }

    /// Whether every field holds a colour the format accepts.
    fn is_valid(&self, cx: &App) -> bool {
        self.fields.iter().all(|field| {
            valid_hex(
                field.input.read(cx).content(),
                field.slot.alpha,
                field.slot.optional,
            )
        })
    }

    /// What has been typed into every field, in the catalogue's own order.
    fn values(&self, cx: &App) -> Vec<String> {
        self.fields
            .iter()
            .map(|field| field.input.read(cx).content().trim().to_owned())
            .collect()
    }

    /// Puts an optional slot back to automatic by emptying its field.
    fn clear_field(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(field) = self.fields.get(index) else {
            return;
        };
        field.input.update(cx, |input, cx| input.clear(cx));
        self.revalidate(cx);
    }

    /// The file the fields currently describe.
    fn collect(&self, cx: &App) -> CatalogFile {
        let name = self.name_input.read(cx).content().trim().to_owned();
        self.catalog.file_from(name, &self.values(cx), self.dark)
    }

    /// Writes the edits and reloads the catalogue.
    ///
    /// A failed write leaves the editor open with the reason showing, so the
    /// user never believes a colour took effect when it did not.
    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.is_valid(cx) {
            self.status = Some(label(cx, "settings.editor.invalid"));
            cx.notify();
            return;
        }

        let file = self.collect(cx);
        if let Err(err) = self.catalog.save(&self.id, &file) {
            log::error!("could not write the {} file: {err:#}", self.id);
            self.status = Some(text(
                cx,
                "settings.manage.write_failed",
                &[("error", &format!("{err:#}"))],
            ));
            cx.notify();
            return;
        }

        self.catalog.reload(cx);
        cx.emit(ThemeEditorEvent::Saved);
    }

    /// Moves focus into the name field the first time the editor is drawn.
    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_focus {
            return;
        }
        self.pending_focus = false;
        let handle = self.name_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// The overlay scroll indicator of the field list, as it stands.
    fn scrollbar(&self) -> Scrollbar {
        Scrollbar::for_handle(SCROLLBAR_ID, ScrollbarAxis::Vertical, &self.scroll)
            .fade(self.scrollbar.fade())
    }

    /// Puts the bar up whenever the list has been scrolled, and starts the
    /// clock that takes it down again.
    fn watch_scroll(&mut self, cx: &mut Context<Self>) {
        let scrolled = scrolled(&self.scroll, ScrollbarAxis::Vertical);
        if let Some(epoch) = self.scrollbar.moved(scrolled) {
            hide_later(epoch, cx, |editor| Some(&mut editor.scrollbar));
        }
    }

    /// Scrolls the list while its thumb is dragged.
    fn drag_scrollbar(&mut self, event: &DragMoveEvent<DraggedThumb>, cx: &mut Context<Self>) {
        let Some(progress) = self.scrollbar().dragged(event, cx) else {
            return;
        };
        self.scrollbar.hold();
        scroll_to(&self.scroll, ScrollbarAxis::Vertical, progress);
        cx.notify();
    }

    /// Lets go of the thumb, and starts its clock again.
    fn release_scrollbar(&mut self, cx: &mut Context<Self>) {
        if let Some(epoch) = self.scrollbar.release() {
            hide_later(epoch, cx, |editor| Some(&mut editor.scrollbar));
            cx.notify();
        }
    }

    /// Puts the bar up while the pointer rests on the edge it rides, and starts
    /// it going the moment the pointer leaves.
    fn hover_scrollbar(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.scrollbar.hover_enter() {
                cx.notify();
            }
            return;
        }

        if let Some(epoch) = self.scrollbar.hover_leave() {
            hide_now(self, epoch, cx, |editor| Some(&mut editor.scrollbar));
        }
    }

    /// The colour a field currently describes.
    ///
    /// For an automatic slot — an optional one left empty — that is the colour
    /// the palette derives, so the swatch never goes blank and the user can see
    /// what "automatic" actually resolved to. `None` only for a field that
    /// holds something which is not a colour at all.
    fn color_of(&self, index: usize, cx: &App) -> Option<Hsla> {
        let field = self.fields.get(index)?;
        let value = field.input.read(cx).content();
        if value.trim().is_empty() {
            return field
                .slot
                .optional
                .then(|| {
                    self.catalog
                        .derived_color(index, self.dark, &self.values(cx))
                })
                .flatten();
        }
        valid_hex(value, field.slot.alpha, field.slot.optional)
            .then(|| parse_hex(value))
            .flatten()
    }

    /// One labelled colour field: the slot's name, the hex value, the swatch,
    /// and — for an optional slot — the button that puts it back to automatic.
    ///
    /// The swatch is what turns a hex value back into something a person can
    /// judge, and it doubles as the refusal: a field holding anything but a
    /// colour has nothing to draw, so the swatch shows an outline instead —
    /// next to the field, which is itself already outlined in the danger
    /// colour.
    ///
    /// An automatic slot is told apart from an explicit one by its label, which
    /// gains the word, and by the muted hex printed after it, which spells out
    /// what the derivation produced; the swatch alone could not say which of
    /// the two a colour came from. That hex is text rather than the field's own
    /// placeholder, because a placeholder rewritten from `render` would notify
    /// the field, which notifies this view, which renders again.
    fn render_field(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chrome = theme(cx);
        let field = &self.fields[index];
        let slot = field.slot;
        let color = self.color_of(index, cx);
        let automatic = slot.optional && field.input.read(cx).content().trim().is_empty();
        let this = cx.entity();

        let swatch = div()
            .flex_none()
            .size(px(SWATCH_SIZE))
            .rounded_md()
            .border_1()
            .border_color(match color {
                Some(_) => chrome.border,
                None => chrome.danger,
            })
            .when_some(color, |this, color| this.bg(color));

        // The two states of an optional slot are mutually exclusive, so they
        // share one cell: an automatic slot shows the hex it resolved to, and
        // an explicit one shows the button that gives the derivation back. A
        // required slot has neither, and no cell.
        let trailing = slot.optional.then(|| {
            if automatic {
                div()
                    .flex_none()
                    .w(px(64.))
                    .truncate()
                    .text_size(px(11.))
                    .text_color(chrome.text_muted)
                    .child(SharedString::from(color.map(to_hex).unwrap_or_default()))
                    .into_any_element()
            } else {
                Button::new(
                    ("theme-editor-auto", index),
                    label(cx, "settings.editor.automatic"),
                )
                .variant(ButtonVariant::Secondary)
                .tab_index(tab::FIRST_COLOR + 2 * index as isize + 1)
                .on_click(move |_, _window, cx| {
                    this.update(cx, |editor, cx| editor.clear_field(index, cx));
                })
                .into_any_element()
            }
        });

        let words = label(cx, slot.label_key);
        let words = if automatic {
            text(cx, "settings.editor.automatic_slot", &[("name", &words)])
        } else {
            words
        };

        div()
            // Named after the slot rather than numbered, so that the element
            // keeps its identity as two catalogues swap field lists.
            .id(slot.key)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .w(px(LABEL_WIDTH))
                    .truncate()
                    .text_size(px(12.))
                    .text_color(if automatic {
                        chrome.text_muted
                    } else {
                        chrome.text
                    })
                    .child(words),
            )
            .child(div().flex_1().min_w_0().child(field.input.clone()))
            .child(swatch)
            .children(trailing)
    }

    /// The colour fields, laid out [`FIELD_COLUMNS`] to a row.
    fn render_fields(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<Div> {
        range
            .collect::<Vec<_>>()
            .chunks(FIELD_COLUMNS)
            .map(|row| {
                let cells: Vec<_> = row
                    .iter()
                    .map(|index| self.render_field(*index, cx).into_any_element())
                    .collect();
                // Pad a short last row so its fields keep the width they have
                // in every other row rather than stretching to fill it.
                let padding = (FIELD_COLUMNS - row.len()) % FIELD_COLUMNS;
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .gap(px(12.))
                    .children(cells)
                    .children((0..padding).map(|_| div().flex_1().min_w_0().into_any_element()))
            })
            .collect()
    }

    /// One heading inside the field list.
    ///
    /// Drawn the way the optional group's heading is drawn, because the two sit
    /// in one list and a reader has no reason to be told that two kinds of
    /// divider exist.
    fn render_heading(&self, key: &str, cx: &App) -> Div {
        let chrome = theme(cx);
        div()
            .pt(px(4.))
            .text_size(px(11.))
            .text_color(chrome.text_muted)
            .child(label(cx, key))
    }

    /// The rows `range` covers, with every heading the catalogue placed inside
    /// it drawn before the slot it names.
    fn render_section(
        &self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        for piece in split(range, &self.catalog.group_headings()) {
            match piece {
                Piece::Heading(key) => out.push(self.render_heading(key, cx).into_any_element()),
                Piece::Fields(fields) => out.extend(
                    self.render_fields(fields, cx)
                        .into_iter()
                        .map(IntoElement::into_any_element),
                ),
            }
        }
        out
    }

    /// The dark/light checkbox, or nothing at all for a format without such a
    /// flag.
    ///
    /// A catalogue answering `false` to [`ThemeCatalog::has_dark_flag`] gets no
    /// checkbox *and* no invented value: `self.dark` stays whatever
    /// [`ThemeCatalog::values_of`] reported when the editor opened, and that is
    /// what [`ThemeCatalog::file_from`] is handed back on save.
    ///
    /// Answers the control rather than the form row around it, so that a test
    /// can ask the question without an `AnyElement` being built: gpui allocates
    /// those in the per-frame element arena, which only a draw sweeps, and one
    /// built outside a render pass strands whatever it captured — here a handle
    /// on the editor itself.
    fn dark_checkbox(&self, cx: &mut Context<Self>) -> Option<Checkbox> {
        if !self.catalog.has_dark_flag() {
            return None;
        }
        let this = cx.entity();
        let dark = Checkbox::new("theme-editor-dark", label(cx, "settings.editor.dark"))
            .checked(self.dark)
            .tab_index(tab::DARK)
            .on_toggle(move |checked, _window, cx| {
                this.update(cx, |editor, cx| {
                    editor.dark = checked;
                    cx.notify();
                });
            });
        Some(dark)
    }

    /// The message strip and the two buttons that end the editor.
    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chrome = theme(cx);
        let this = cx.entity();
        let valid = self.is_valid(cx);

        // A refused colour explains itself the moment it is typed rather than
        // waiting for a Save that is already held back — otherwise the only
        // sign would be a greyed-out button with no reason attached.
        let status = self
            .status
            .clone()
            .or_else(|| (!valid).then(|| label(cx, "settings.editor.invalid")))
            .map(|message| {
                div()
                    .text_size(px(12.))
                    .text_color(chrome.danger)
                    .child(message)
            });

        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(10.))
            .child(div().h(px(1.)).w_full().flex_none().bg(chrome.border))
            .children(status)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("theme-editor-cancel", label(cx, "common.cancel"))
                            .variant(ButtonVariant::Secondary)
                            .tab_index(tab::CANCEL)
                            .on_click({
                                let this = this.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |editor, cx| editor.cancel(cx));
                                }
                            }),
                    )
                    .child(
                        Button::new("theme-editor-save", label(cx, "common.save"))
                            .variant(ButtonVariant::Primary)
                            .disabled(!valid)
                            .tab_index(tab::SAVE)
                            .on_click({
                                let this = this.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |editor, cx| editor.save(cx));
                                }
                            }),
                    ),
            )
    }
}

/// One run of the field list, as [`split`] hands it out.
#[derive(Debug, PartialEq, Eq)]
enum Piece<'a> {
    /// A heading, under the given label key.
    Heading(&'a str),
    /// A run of consecutive slots.
    Fields(std::ops::Range<usize>),
}

/// How `range` breaks up around the headings a catalogue placed inside it.
///
/// Pure, and separated from the rendering for exactly that reason: the rules
/// about which headings count are the part worth stating once and testing.
///
/// A heading never reorders the slots around it. One naming an index outside
/// `range`, or one behind a heading already emitted, is dropped rather than
/// allowed to split the list backwards — a catalogue that lists its headings
/// out of order loses the ones that are out of order and nothing else. Two on
/// the same index are both drawn, in the order given. A catalogue that names
/// none — the default — yields the single range it was handed, which is what
/// keeps the optional-group layout exactly as it was.
fn split<'a>(range: std::ops::Range<usize>, headings: &[(usize, &'a str)]) -> Vec<Piece<'a>> {
    let mut out = Vec::new();
    let mut start = range.start;
    for (index, key) in headings {
        let index = *index;
        if index < start || index >= range.end {
            continue;
        }
        if index > start {
            out.push(Piece::Fields(start..index));
            start = index;
        }
        out.push(Piece::Heading(key));
    }
    if start < range.end || out.is_empty() {
        out.push(Piece::Fields(start..range.end));
    }
    out
}

impl EventEmitter<ThemeEditorEvent> for ThemeEditor {}

impl Focusable for ThemeEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ThemeEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_focus(window, cx);
        self.watch_scroll(cx);
        let chrome = theme(cx);
        let bar = self
            .scrollbar()
            .on_hover(cx.listener(|editor, hovered: &bool, _window, cx| {
                editor.hover_scrollbar(*hovered, cx);
            }));

        let values = self.values(cx);
        let name = SharedString::from(self.name_input.read(cx).content().to_owned());
        let preview = self
            .catalog
            .clone()
            .render_preview(&self.id, name, &values, self.dark, cx);

        let dark_row = self.dark_checkbox(cx).map(|dark| form_row("", dark));

        // A format's optional slots get a heading of their own: without it they
        // would run on from the required ones with nothing to say that these
        // are the ones the file may leave out.
        let (required, derived) = match self.catalog.optional_group_start() {
            Some(first) if first < self.fields.len() => (0..first, Some(first..self.fields.len())),
            _ => (0..self.fields.len(), None),
        };

        let name_label = label(cx, "settings.editor.name");
        let group_label = label(cx, "settings.editor.grid_group");
        let list = div()
            .id("theme-editor-fields")
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .min_h_0()
            .gap(px(8.))
            .max_h(px(BODY_MAX_HEIGHT))
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .child(preview)
            .child(form_row(name_label, self.name_input.clone()))
            .children(dark_row)
            .children(self.render_section(required, cx))
            .children(derived.map(|derived| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(chrome.text_muted)
                            .child(group_label),
                    )
                    .children(self.render_section(derived, cx))
            }));

        div()
            .id("theme-editor")
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .min_h_0()
            .gap(px(12.))
            .on_drag_move::<DraggedThumb>(cx.listener(
                |editor, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                    editor.drag_scrollbar(event, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _: &MouseUpEvent, _window, cx| {
                    editor.release_scrollbar(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _: &MouseUpEvent, _window, cx| {
                    editor.release_scrollbar(cx);
                }),
            )
            .child(
                // The middle box exists only to hold the overlay bar, as in a
                // settings form: a scrolling box cannot, because its children
                // are what scroll away underneath it.
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .child(list)
                    .children(bar.render(&chrome)),
            )
            .child(self.render_footer(cx))
    }
}

/// A miniature of the chrome a chrome theme would draw.
///
/// The colours such a theme is actually judged by: a window background with a
/// raised surface on it, primary and muted text, a chip each for the accent and
/// the two status colours, and — because the five grid slots are invisible
/// anywhere else in a settings dialog — three rows of a result grid, header
/// included, with a `NULL` cell and a primary-key column in it.
///
/// Lives here rather than in [`crate::catalog`] because it is the editor's
/// preview and shares the editor's units; [`crate::UiThemeCatalog`] is the one
/// caller.
pub(crate) fn render_ui_preview(
    name: SharedString,
    values: &[String],
    dark: bool,
    _cx: &mut App,
) -> AnyElement {
    let palette = ThemeFile::new("", dark, ui_colors(values)).to_theme();

    let chip = |color: Hsla| div().flex_none().size(px(12.)).rounded_full().bg(color);
    let cell = |color: Hsla, words: &'static str| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .px(px(4.))
            .text_color(color)
            .child(words)
    };
    let row = |background: Option<Hsla>, selected: bool| {
        div()
            .flex()
            .flex_row()
            .w_full()
            .py(px(1.))
            .when_some(background, |this, color| this.bg(color))
            .when(selected, |this| this.bg(palette.grid_selection))
            .child(cell(palette.grid_pk, "id"))
            .child(cell(palette.text, "name"))
            .child(cell(palette.grid_null, "NULL"))
    };

    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(10.))
        .rounded_md()
        .border_1()
        .border_color(palette.border)
        .bg(palette.background)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(6.))
                .rounded_md()
                .bg(palette.surface)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(palette.text)
                        .child(name),
                )
                .child(
                    div()
                        .flex_none()
                        .size(px(14.))
                        .rounded_sm()
                        .bg(palette.surface_active),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(chip(palette.accent))
                .child(chip(palette.success))
                .child(chip(palette.danger))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(palette.text_muted)
                        .child("Aa Bb Cc 0123"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .overflow_hidden()
                .rounded_sm()
                .border_1()
                .border_color(palette.border)
                .text_size(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .w_full()
                        .py(px(1.))
                        .bg(palette.grid_header)
                        .text_color(palette.text_muted)
                        .child(cell(palette.text_muted, "id"))
                        .child(cell(palette.text_muted, "name"))
                        .child(cell(palette.text_muted, "note")),
                )
                .child(row(None, false))
                .child(row(Some(palette.grid_row_alt), false))
                .child(row(None, true)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::catalog::{CatalogEntry, EditorThemeCatalog, ImportError, UiThemeCatalog, slot};
    use ruui::ThemeDirs;

    /// The slots [`Flagless`] carries; four, so a heading can fall in the
    /// middle of a row as well as at the start of one.
    const FLAGLESS_SLOTS: [Slot; 4] = [
        slot("one", "slot.one", false),
        slot("two", "slot.two", false),
        slot("three", "slot.three", false),
        slot("four", "slot.four", false),
    ];

    /// A host's own catalogue, over a format with no dark/light flag and with
    /// a heading of its own halfway down the list.
    ///
    /// Every path that writes anything refuses: the editor under test is never
    /// saved, and a stub that could write would be a stub that could write
    /// somewhere real.
    struct Flagless;

    impl ThemeCatalog for Flagless {
        fn kind_label_key(&self) -> &'static str {
            "flagless.kind"
        }

        fn element_prefix(&self) -> &'static str {
            "flagless"
        }

        fn delete_confirm_key(&self) -> &'static str {
            "flagless.delete"
        }

        fn entries(&self, _cx: &App) -> Vec<CatalogEntry> {
            Vec::new()
        }

        fn slots(&self) -> &'static [Slot] {
            &FLAGLESS_SLOTS
        }

        fn load(&self, _id: &str, _cx: &App) -> Option<CatalogFile> {
            None
        }

        fn values_of(&self, _file: &CatalogFile) -> (Vec<String>, bool) {
            // `true` on purpose: the editor has no checkbox to read it from, so
            // this is the only place the flag can come from, and it has to
            // survive to `file_from`.
            (vec!["#101010".to_owned(); FLAGLESS_SLOTS.len()], true)
        }

        fn file_from(&self, name: String, values: &[String], dark: bool) -> CatalogFile {
            CatalogFile::Other(std::sync::Arc::new((name, values.to_vec(), dark)))
        }

        fn dir(&self) -> anyhow::Result<std::path::PathBuf> {
            anyhow::bail!("this catalogue has no directory")
        }

        fn default_id(&self) -> String {
            "flagless".to_owned()
        }

        fn generated_id_prefix(&self) -> &'static str {
            "flagless"
        }

        fn save(&self, _id: &str, _file: &CatalogFile) -> anyhow::Result<std::path::PathBuf> {
            anyhow::bail!("this catalogue is never written")
        }

        fn write(&self, _file: &CatalogFile, _path: &Path) -> anyhow::Result<()> {
            anyhow::bail!("this catalogue is never written")
        }

        fn delete(&self, _id: &str) -> anyhow::Result<()> {
            anyhow::bail!("this catalogue is never written")
        }

        fn read(&self, _path: &Path) -> std::result::Result<CatalogFile, ImportError> {
            Err(ImportError::WrongKind("flagless.wrong_kind"))
        }

        fn reload(&self, _cx: &mut App) {}

        fn render_preview(
            &self,
            _id: &str,
            _name: SharedString,
            _values: &[String],
            _dark: bool,
            _cx: &mut App,
        ) -> AnyElement {
            div().into_any_element()
        }

        fn group_headings(&self) -> Vec<(usize, &'static str)> {
            vec![(2, "flagless.second_group")]
        }

        fn has_dark_flag(&self) -> bool {
            false
        }
    }

    /// The editor over [`Flagless`], with nothing else installed.
    fn flagless(cx: &mut App) -> Entity<ThemeEditor> {
        let catalog: Arc<dyn ThemeCatalog> = Arc::new(Flagless);
        let file = CatalogFile::Other(std::sync::Arc::new(()));
        cx.new(|cx| ThemeEditor::new(catalog, "flagless", &file, cx))
    }

    /// The two catalogues that ship here, over a directory nothing writes to.
    fn shipped() -> [Box<dyn ThemeCatalog>; 2] {
        let dirs = || ThemeDirs {
            ui_themes: std::path::PathBuf::from("/nowhere/themes"),
            editor_themes: Some(std::path::PathBuf::from("/nowhere/editor-themes")),
        };
        [
            Box::new(UiThemeCatalog::new(dirs(), "dark")),
            Box::new(EditorThemeCatalog::new(dirs(), "one-dark")),
        ]
    }

    #[test]
    fn the_colour_fields_leave_room_for_their_own_revert_buttons() {
        // Every field takes two indices — its own and the button behind it — so
        // the last one has to stay clear of the footer.
        let widest = shipped()
            .iter()
            .map(|catalog| catalog.slots().len())
            .max()
            .expect("two catalogues");
        let last = tab::FIRST_COLOR + 2 * widest as isize;
        assert!(last < tab::CANCEL, "{last} runs into the footer");
        const { assert!(tab::NAME < tab::DARK && tab::DARK < tab::FIRST_COLOR) };
        const { assert!(tab::CANCEL < tab::SAVE) };
    }

    #[test]
    fn an_optional_group_never_starts_past_the_end_of_the_fields() {
        // The editor splits its list at this index; one past the end would
        // draw a heading over nothing, and one in the middle of the required
        // slots would put the heading over slots the file cannot omit.
        for catalog in shipped() {
            let Some(first) = catalog.optional_group_start() else {
                continue;
            };
            let slots = catalog.slots();
            assert!(first < slots.len(), "the group starts past the last slot");
            assert!(
                slots[..first].iter().all(|slot| !slot.optional),
                "a slot before the group is optional"
            );
            assert!(
                slots[first..].iter().all(|slot| slot.optional),
                "a slot inside the group is required"
            );
        }
    }

    /// The whole of [`ThemeCatalog::has_dark_flag`]: a format that carries no
    /// dark/light flag must not be given a checkbox for one, because the
    /// checkbox would be editing a value the format has nowhere to put.
    #[gpui::test]
    fn a_format_with_no_dark_flag_gets_no_checkbox_for_one(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let editor = flagless(cx);
            editor.update(cx, |editor, cx| {
                assert!(
                    editor.dark_checkbox(cx).is_none(),
                    "a flagless catalogue was given a dark/light checkbox"
                );
                // And the flag `values_of` reported is what `file_from` gets
                // back, rather than a `false` the editor invented.
                assert!(editor.dark, "the catalogue's own flag was overwritten");
            });

            // The two catalogues that do carry one are unaffected.
            let ui: Arc<dyn ThemeCatalog> = Arc::new(UiThemeCatalog::new(
                ThemeDirs {
                    ui_themes: std::path::PathBuf::from("/nowhere/themes"),
                    editor_themes: None,
                },
                "dark",
            ));
            let file = CatalogFile::UiTheme(Box::new(ThemeFile::new("Dark", true, ui_colors(&[]))));
            let editor = cx.new(|cx| ThemeEditor::new(ui, "dark", &file, cx));
            editor.update(cx, |editor, cx| {
                assert!(editor.dark_checkbox(cx).is_some());
            });
        });
    }

    #[test]
    fn a_heading_splits_the_run_of_fields_it_stands_in_front_of() {
        // The default: one run, exactly as the list was drawn before headings
        // existed at all.
        assert_eq!(split(0..4, &[]), vec![Piece::Fields(0..4)]);
        // In the middle, at the very start, and one of each.
        assert_eq!(
            split(0..4, &[(2, "b")]),
            vec![
                Piece::Fields(0..2),
                Piece::Heading("b"),
                Piece::Fields(2..4)
            ]
        );
        assert_eq!(
            split(0..4, &[(0, "a")]),
            vec![Piece::Heading("a"), Piece::Fields(0..4)]
        );
        assert_eq!(
            split(0..4, &[(0, "a"), (3, "b")]),
            vec![
                Piece::Heading("a"),
                Piece::Fields(0..3),
                Piece::Heading("b"),
                Piece::Fields(3..4),
            ]
        );
        // Outside the section, past the last slot, and behind one already
        // drawn: dropped, never drawn over nothing and never drawn backwards.
        assert_eq!(
            split(2..4, &[(0, "a"), (9, "b")]),
            vec![Piece::Fields(2..4)]
        );
        assert_eq!(
            split(0..4, &[(3, "b"), (1, "a")]),
            vec![
                Piece::Fields(0..3),
                Piece::Heading("b"),
                Piece::Fields(3..4)
            ]
        );
    }
}
