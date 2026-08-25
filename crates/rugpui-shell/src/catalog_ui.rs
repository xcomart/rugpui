//! The row of management buttons drawn under a palette picker.
//!
//! Five actions — duplicate, edit, delete, import, export — over any
//! [`ThemeCatalog`], plus the delete confirmation and the one line of status
//! they report through. A settings dialog holds one of these per picker,
//! renders it under the picker, and subscribes to [`CatalogActionEvent`].
//!
//! # What it does not own
//!
//! Two things, and both for the same reason — they belong to the dialog around
//! it:
//!
//! * **The selection.** Which entry is picked is a *form field*, saved with the
//!   rest of the settings, so the dialog owns it and pushes it in with
//!   [`CatalogActions::set_selection`]. When an action moves it — a duplicate
//!   selects the copy, a delete falls back to the default —
//!   [`CatalogActionEvent::Select`] asks the dialog to move it, rather than the
//!   row changing a value it does not own.
//! * **The editor.** [`ThemeEditor`](crate::ThemeEditor) is drawn *instead of*
//!   the settings form, not beside it — see that module for why — so opening
//!   one is [`CatalogActionEvent::Edit`] and the dialog is what swaps its body.
//!
//! # Tab indices
//!
//! A row takes [`CatalogActions::TAB_SPAN`] consecutive indices from the `base`
//! it was built with, whether or not it is currently asking anything: the five
//! buttons in order, then the confirmation's cancel and delete. Fixing the span
//! is what keeps the tab ring from shifting under the user as a confirmation
//! appears.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    Context, EventEmitter, IntoElement, PathPromptOptions, Render, SharedString, Window, div,
    prelude::*, px,
};
use rugpui::{Button, ButtonVariant, theme, theme_store};

use crate::catalog::{CatalogFile, ThemeCatalog};
use crate::inject::{label, text};

/// What a management row can be asked to do.
///
/// Shared by every catalogue; which of them a given selection permits is
/// [`Action::enabled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Copy the selected entry into a new file and open it for editing.
    ///
    /// Also the "save under another name" of the editor: a built-in palette is
    /// duplicated to be edited, and a custom one is duplicated to be varied.
    Duplicate,
    /// Open the selected custom entry for editing.
    Edit,
    /// Remove the selected custom entry's file, once confirmed.
    Delete,
    /// Read files the user picks into the catalogue's own directory.
    Import,
    /// Write the selected entry out to a file the user picks.
    Export,
}

/// The five actions in the order they are drawn.
const ACTIONS: [Action; 5] = [
    Action::Duplicate,
    Action::Edit,
    Action::Delete,
    Action::Import,
    Action::Export,
];

/// Element id fragment and tab offset of the confirmation's "cancel".
const SLOT_CONFIRM_CANCEL: usize = 5;

/// Element id fragment and tab offset of the confirmation's "delete".
const SLOT_CONFIRM_DELETE: usize = 6;

impl Action {
    /// Position of the action within its row, used for both the element id and
    /// the tab index so the two can never drift apart.
    fn slot(self) -> usize {
        match self {
            Self::Duplicate => 0,
            Self::Edit => 1,
            Self::Delete => 2,
            Self::Import => 3,
            Self::Export => 4,
        }
    }

    /// Key of the button's label.
    fn label_key(self) -> &'static str {
        match self {
            Self::Duplicate => "settings.manage.duplicate",
            Self::Edit => "settings.manage.edit",
            Self::Delete => "settings.manage.delete",
            Self::Import => "settings.manage.import",
            Self::Export => "settings.manage.export",
        }
    }

    /// Whether the action applies to the entry currently selected.
    ///
    /// `known` is whether the selected id resolves at all — a hand-edited
    /// settings file can name one that does not — and `custom` whether what it
    /// resolves to came from a file, which is the only kind an application may
    /// rewrite or remove.
    fn enabled(self, known: bool, custom: bool) -> bool {
        match self {
            // Exporting resolves the entry through the registry rather than off
            // the disk, so a built-in one exports as readily as a custom one.
            Self::Duplicate | Self::Export => known,
            Self::Edit | Self::Delete => custom,
            // Importing does not look at the selection at all.
            Self::Import => true,
        }
    }
}

/// Emitted by [`CatalogActions`].
pub enum CatalogActionEvent {
    /// The host should put the theme editor over `file`, saving under `id`.
    Edit {
        /// The id the edited file is saved back under.
        id: String,
        /// The file to edit.
        file: CatalogFile,
    },
    /// The host should move this picker's selection to `id`.
    Select(String),
    /// Files were written or removed and the registries reloaded; whatever is
    /// already wearing one of them has to be repainted.
    Changed,
}

/// State of one picker's management row.
///
/// One per catalogue, since two rows ask and report independently: a delete
/// waiting to be confirmed under the chrome themes must not disappear because
/// something went wrong under the editor themes.
pub struct CatalogActions {
    /// The catalogue this row manages.
    catalog: Arc<dyn ThemeCatalog>,
    /// The id currently picked, as the host's form holds it.
    selection: String,
    /// First of the row's [`CatalogActions::TAB_SPAN`] tab indices.
    base: isize,
    /// Whether the delete confirmation is showing.
    confirming: bool,
    /// What went wrong the last time this row was used, if anything.
    status: Option<SharedString>,
}

impl CatalogActions {
    /// How many consecutive tab indices a row occupies; see the module docs.
    pub const TAB_SPAN: isize = 7;

    /// A row over `catalog`, taking tab indices from `base`.
    pub fn new(catalog: Arc<dyn ThemeCatalog>, base: isize) -> Self {
        Self {
            catalog,
            selection: String::new(),
            base,
            confirming: false,
            status: None,
        }
    }

    /// The catalogue this row manages.
    pub fn catalog(&self) -> &Arc<dyn ThemeCatalog> {
        &self.catalog
    }

    /// The id the row is currently acting on.
    pub fn selection(&self) -> &str {
        &self.selection
    }

    /// Whether the delete confirmation is showing.
    ///
    /// The host's own `Escape` handler needs this: a keystroke meant to back
    /// the confirmation out must not fall through and close the whole dialog
    /// around it, so the handler asks a row whether it is mid-question before
    /// deciding what `Escape` should do.
    pub fn is_confirming(&self) -> bool {
        self.confirming
    }

    /// Tells the row which entry the picker above it has selected.
    ///
    /// Call it whenever the form's own value changes, including when the form
    /// is first filled in: everything the row offers is about the selection,
    /// and a row that had not been told would grey the wrong buttons out.
    pub fn set_selection(&mut self, id: impl Into<String>, cx: &mut Context<Self>) {
        let id = id.into();
        if self.selection == id {
            return;
        }
        self.selection = id;
        // A confirmation is about the entry that was selected when it was
        // asked; moving the selection out from under it would put the question
        // to a different palette.
        self.confirming = false;
        cx.notify();
    }

    /// Drops whatever the last action had to report.
    pub fn clear_status(&mut self, cx: &mut Context<Self>) {
        if self.status.take().is_some() {
            cx.notify();
        }
    }

    /// Runs one of the management actions.
    ///
    /// Every one of them starts by clearing whatever the last one had to
    /// report, so a message never outlives the situation it described.
    fn run(&mut self, action: Action, cx: &mut Context<Self>) {
        self.status = None;
        match action {
            Action::Duplicate => self.duplicate(cx),
            Action::Edit => self.edit(cx),
            Action::Delete => {
                // Deleting is the one action here that cannot be undone by
                // doing it again, so it asks first.
                self.confirming = true;
                cx.notify();
            }
            Action::Import => self.import(cx),
            Action::Export => self.export(cx),
        }
    }

    /// Reports why an action could not be carried out.
    fn report(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.status = Some(message);
        cx.notify();
    }

    /// Copies the selected entry into a file of its own and opens it.
    ///
    /// Works on a built-in entry as readily as on a custom one — that is the
    /// point of it, since the built-in palettes are where a user's own usually
    /// starts, and the store refuses to write over a built-in id.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        let Some(mut file) = catalog.load(&self.selection, cx) else {
            return;
        };

        let name = text(
            cx,
            "settings.manage.copy_name",
            &[("name", &catalog.name_of(&file))],
        )
        .to_string();
        let id = theme_store::unique_id(
            &[name.as_str()],
            catalog.generated_id_prefix(),
            &catalog.taken_ids(cx),
        );
        catalog.set_name(&mut file, name);

        if let Err(err) = catalog.save(&id, &file) {
            log::error!("could not write the duplicated {id}: {err:#}");
            let message = text(
                cx,
                "settings.manage.write_failed",
                &[("error", &format!("{err:#}"))],
            );
            self.report(message, cx);
            return;
        }

        catalog.reload(cx);
        cx.emit(CatalogActionEvent::Changed);
        cx.emit(CatalogActionEvent::Select(id.clone()));
        cx.emit(CatalogActionEvent::Edit { id, file });
    }

    /// Opens the selected custom entry in the editor.
    fn edit(&mut self, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        let Some((id, file)) = catalog
            .entry(&self.selection, cx)
            .filter(|entry| !entry.builtin)
            .and_then(|entry| catalog.load(&entry.id, cx).map(|file| (entry.id, file)))
        else {
            return;
        };
        cx.emit(CatalogActionEvent::Edit { id, file });
    }

    /// Drops the delete confirmation without acting on it.
    ///
    /// The row's own "cancel" button already calls this; it is public so the
    /// host's `Escape` handler can call it too, for a row
    /// [`is_confirming`](CatalogActions::is_confirming) said was mid-question —
    /// backing the question out is what `Escape` should mean there, rather
    /// than closing the settings dialog around it.
    pub fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
        if self.confirming {
            self.confirming = false;
            cx.notify();
        }
    }

    /// Removes the selected custom entry's file.
    ///
    /// The selection then moves to the default id, because the one it held no
    /// longer resolves; the *setting* still names it until the dialog is saved,
    /// which is why [`CatalogActionEvent::Changed`] goes out too — the running
    /// window falls back to the default palette in the same breath as the
    /// picker does.
    fn delete(&mut self, cx: &mut Context<Self>) {
        self.confirming = false;
        let catalog = self.catalog.clone();
        let Some(entry) = catalog
            .entry(&self.selection, cx)
            .filter(|entry| !entry.builtin)
        else {
            cx.notify();
            return;
        };

        if let Err(err) = catalog.delete(&entry.id) {
            log::error!("could not remove {}: {err:#}", entry.id);
            let message = text(
                cx,
                "settings.manage.delete_failed",
                &[("error", &format!("{err:#}"))],
            );
            self.report(message, cx);
            return;
        }

        catalog.reload(cx);
        cx.emit(CatalogActionEvent::Changed);
        cx.emit(CatalogActionEvent::Select(catalog.default_id()));
        cx.notify();
    }

    /// Asks the platform for palette files and installs what it hands back.
    ///
    /// The dialog is a platform call, and on X11 that is the call gpui was
    /// patched over in the first place, so nothing here waits on it: the prompt
    /// hands back a channel straight away, the click that started it returns,
    /// and the answer is picked up on a task of its own. By the time
    /// [`CatalogActions::install`] runs, this update — and the borrow it holds
    /// — is long over.
    fn import(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(label(cx, "settings.manage.import_select")),
        });

        cx.spawn(async move |row, cx| {
            let chosen = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    log::warn!("the file picker could not be opened: {error:#}");
                    return;
                }
            };
            row.update(cx, |row, cx| row.install(chosen, cx)).ok();
        })
        .detach();
    }

    /// Copies `paths` into the catalogue's directory, one file at a time.
    ///
    /// A file that is not a palette file of this kind is counted and skipped
    /// rather than failing the batch: a user who picks a folder full of
    /// palettes should get the ones that parse. Nothing is ever written over —
    /// the id each file lands under comes from its own name, then from its file
    /// name, and is suffixed until it is free, which is why `taken` grows as
    /// the batch goes: two files that would both like to be `one-dark` become
    /// `one-dark` and `one-dark-2`, and neither touches the `one-dark` the user
    /// already had.
    ///
    /// When nothing at all could be installed the first refusal is what gets
    /// reported, which is what makes picking a single file of the wrong kind
    /// say so in as many words instead of counting to one.
    fn install(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        let mut taken = catalog.taken_ids(cx);
        let mut installed = None;
        let mut refused: Option<SharedString> = None;
        let mut skipped = 0usize;

        // Only the first refusal is kept; the rest are in the log. The count is
        // what the user gets for the others, since a management row cannot hold
        // one sentence per file without pushing the pickers off the dialog.
        let refuse = |path: &Path, message: SharedString, first: &mut Option<SharedString>| {
            log::warn!("skipping {}: {message}", path.display());
            if first.is_none() {
                *first = Some(message);
            }
        };

        for path in &paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let file = match catalog.read(path) {
                Ok(file) => file,
                Err(err) => {
                    refuse(path, err.message(name, cx), &mut refused);
                    skipped += 1;
                    continue;
                }
            };
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let id = theme_store::unique_id(
                &[&catalog.name_of(&file), stem],
                catalog.generated_id_prefix(),
                &taken,
            );
            if let Err(err) = catalog.save(&id, &file) {
                let message = text(
                    cx,
                    "settings.manage.write_failed",
                    &[("error", &format!("{err:#}"))],
                );
                refuse(path, message, &mut refused);
                skipped += 1;
                continue;
            }
            taken.push(id.clone());
            installed = Some(id);
        }

        let Some(id) = installed else {
            if let Some(message) = refused {
                self.report(message, cx);
            }
            return;
        };

        catalog.reload(cx);
        if skipped > 0 {
            self.status = Some(text(
                cx,
                "settings.manage.import_skipped",
                &[("count", &skipped.to_string())],
            ));
        }
        cx.emit(CatalogActionEvent::Changed);
        // The last one installed, so that picking a single file selects it —
        // and so that a file which had to be renamed around a collision shows
        // the user which entry it became.
        cx.emit(CatalogActionEvent::Select(id));
        cx.notify();
    }

    /// Writes the selected entry out to a file the user picks.
    ///
    /// Built-in entries included: the palette is resolved from the registry
    /// rather than read off the disk, so exporting one is how a user gets a
    /// starting point they can edit outside the application or hand to somebody
    /// else. Whether an existing file may be replaced is the platform save
    /// dialog's question, not ours.
    ///
    /// Asynchronous for the same reason [`CatalogActions::import`] is, and the
    /// file is collected *before* the prompt so the task owns everything it
    /// needs and never has to reach back into the row except to complain.
    fn export(&mut self, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        let selection = self.selection.clone();
        let Some(file) = catalog.load(&selection, cx) else {
            return;
        };
        let suggested = format!("{selection}.{}", theme_store::FILE_EXTENSION);
        let prompt = cx.prompt_for_new_path(&export_directory(catalog.as_ref()), Some(&suggested));

        cx.spawn(async move |row, cx| {
            let path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    log::warn!("the save dialog could not be opened: {error:#}");
                    return;
                }
            };
            let Err(err) = catalog.write(&file, &path) else {
                return;
            };
            log::error!("could not write {}: {err:#}", path.display());
            row.update(cx, |row, cx| {
                let message = text(
                    cx,
                    "settings.manage.write_failed",
                    &[("error", &format!("{err:#}"))],
                );
                row.report(message, cx);
            })
            .ok();
        })
        .detach();
    }
}

impl EventEmitter<CatalogActionEvent> for CatalogActions {}

impl Render for CatalogActions {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = theme(cx);
        let this = cx.entity();
        let prefix = self.catalog.element_prefix();
        let base = self.base;

        let entry = self.catalog.entry(&self.selection, cx);
        let known = entry.is_some();
        let custom = entry.as_ref().is_some_and(|entry| !entry.builtin);
        let confirming = self.confirming;

        let buttons = ACTIONS.map(|action| {
            Button::new((prefix, action.slot()), label(cx, action.label_key()))
                .variant(ButtonVariant::Secondary)
                // Everything is held while the confirmation is up, so that the
                // question can only be answered, not walked away from.
                .disabled(confirming || !action.enabled(known, custom))
                .tab_index(base + action.slot() as isize)
                .on_click({
                    let this = this.clone();
                    move |_, _window, cx| {
                        this.update(cx, |row, cx| row.run(action, cx));
                    }
                })
                .into_any_element()
        });

        let confirm = confirming.then(|| {
            let name = entry.map(|entry| entry.name).unwrap_or_default();
            let question = text(cx, self.catalog.delete_confirm_key(), &[("name", &name)]);

            div()
                .flex()
                .flex_row()
                // Wraps rather than overflowing: a locale that spells the
                // question out at length would otherwise push a button past the
                // edge of the section.
                .flex_wrap()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(chrome.text)
                        .child(question),
                )
                .child(
                    Button::new((prefix, SLOT_CONFIRM_CANCEL), label(cx, "common.cancel"))
                        .variant(ButtonVariant::Secondary)
                        .tab_index(base + SLOT_CONFIRM_CANCEL as isize)
                        .on_click({
                            let this = this.clone();
                            move |_, _window, cx| {
                                this.update(cx, |row, cx| row.cancel_confirm(cx));
                            }
                        }),
                )
                .child(
                    Button::new(
                        (prefix, SLOT_CONFIRM_DELETE),
                        label(cx, "settings.manage.delete"),
                    )
                    .variant(ButtonVariant::Danger)
                    .tab_index(base + SLOT_CONFIRM_DELETE as isize)
                    .on_click({
                        let this = this.clone();
                        move |_, _window, cx| {
                            this.update(cx, |row, cx| row.delete(cx));
                        }
                    }),
                )
        });

        let status = self.status.clone().map(|message| {
            div()
                .text_size(px(11.))
                .text_color(chrome.danger)
                .child(message)
        });

        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(buttons),
            )
            .children(confirm)
            .children(status)
    }
}

/// Where the export save dialog should open.
///
/// The catalogue's own directory, since that is where a file the user then
/// wants the application to *load* has to end up — but only once it exists: a
/// save dialog pointed at a directory that has never been created opens
/// somewhere arbitrary on some platforms, so a user who has added no palette of
/// their own yet gets their home directory instead.
pub fn export_directory(catalog: &dyn ThemeCatalog) -> PathBuf {
    catalog
        .dir()
        .ok()
        .filter(|directory| directory.is_dir())
        .or_else(std::env::home_dir)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_takes_its_indices_from_the_base_it_was_given() {
        // Both confirmation buttons have to fall inside the span, or a
        // confirmation would land on an index the form beside it also uses.
        for action in ACTIONS {
            assert!(
                (action.slot() as isize) < CatalogActions::TAB_SPAN,
                "{action:?} falls outside the row"
            );
        }
        assert!((SLOT_CONFIRM_CANCEL as isize) < CatalogActions::TAB_SPAN);
        assert!((SLOT_CONFIRM_DELETE as isize) < CatalogActions::TAB_SPAN);
    }

    #[test]
    fn every_action_has_a_slot_of_its_own() {
        let mut slots: Vec<usize> = ACTIONS.iter().map(|action| action.slot()).collect();
        slots.push(SLOT_CONFIRM_CANCEL);
        slots.push(SLOT_CONFIRM_DELETE);
        let unique: std::collections::BTreeSet<usize> = slots.iter().copied().collect();
        assert_eq!(unique.len(), slots.len(), "two controls share a slot");
    }

    #[test]
    fn only_a_custom_entry_may_be_rewritten_or_removed() {
        // An id that resolves to nothing: everything but importing is off.
        for action in ACTIONS {
            assert_eq!(
                action.enabled(false, false),
                action == Action::Import,
                "{action:?} on an unknown selection"
            );
        }
        // A built-in entry: it can be copied and exported, never edited or
        // deleted.
        assert!(Action::Duplicate.enabled(true, false));
        assert!(Action::Export.enabled(true, false));
        assert!(!Action::Edit.enabled(true, false));
        assert!(!Action::Delete.enabled(true, false));
        // One of the user's own: all five.
        for action in ACTIONS {
            assert!(action.enabled(true, true), "{action:?} on a custom entry");
        }
    }

    /// [`CatalogActions::is_confirming`] and [`CatalogActions::cancel_confirm`]
    /// exist so that a host's own `Escape` handler can ask whether a row is
    /// mid-question and back it out without the handler falling through to
    /// whatever `Escape` does for the dialog around it — see the module docs.
    /// The row's own "cancel" button already exercises `cancel_confirm`
    /// through the click handler; this is the path the host itself takes.
    #[gpui::test]
    fn a_confirmation_can_be_asked_about_and_cancelled_from_outside_the_row(
        cx: &mut gpui::TestAppContext,
    ) {
        let catalog: Arc<dyn ThemeCatalog> = Arc::new(crate::catalog::UiThemeCatalog::new(
            rugpui::ThemeDirs {
                ui_themes: PathBuf::from("/nowhere/themes"),
                editor_themes: None,
            },
            "dark",
        ));

        cx.update(|cx| {
            let row = cx.new(|_cx| CatalogActions::new(catalog, 0));
            row.update(cx, |row, cx| {
                assert!(!row.is_confirming(), "nothing was asked yet");

                row.run(Action::Delete, cx);
                assert!(row.is_confirming(), "delete asks before it acts");

                row.cancel_confirm(cx);
                assert!(!row.is_confirming(), "cancelled, not merely ignored");
                // Cancelling again is a no-op rather than a panic — the host
                // does not have to track whether it already asked.
                row.cancel_confirm(cx);
                assert!(!row.is_confirming());
            });
        });
    }
}
