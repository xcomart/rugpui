//! A catalogue of palettes: what is in it, what one entry's colours are called,
//! and where a file of one goes.
//!
//! Two things in this crate are written against a catalogue rather than against
//! a particular kind of palette: [`ThemeEditor`](crate::ThemeEditor), which
//! edits one entry colour by colour, and
//! [`CatalogActions`](crate::CatalogActions), which duplicates, edits, deletes,
//! imports and exports them. Both work for a chrome theme and for an editor
//! theme without knowing which they have, because everything the two differ in
//! is behind [`ThemeCatalog`]: which slots there are, what a file of this kind
//! parses as, and where such files live.
//!
//! Two implementations ship here — [`UiThemeCatalog`] over [`rugpui::ThemeFile`]
//! and [`EditorThemeCatalog`] over [`rugpui::EditorThemeFile`] — and a host with
//! a third kind of palette of its own writes one in its own crate, over
//! [`CatalogFile::Other`].
//!
//! # Why the refusals are a type
//!
//! [`ThemeCatalog::read`] is where this crate is *stricter* than `rugpui`'s
//! loader. The loader is forgiving because a broken file in the configuration
//! directory must not take the others down with it, whereas an import is a
//! single deliberate act with a person waiting on the answer, and silently
//! installing a palette with half its slots quietly substituted would be the
//! worse outcome. Every refusal is an [`ImportError`], which carries a sentence
//! the user can act on — above all "that is a file of the other kind", since
//! two palette formats look alike enough that picking the wrong row is the
//! mistake most likely to be made.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use gpui::{App, Hsla, SharedString};
use rugpui::{
    EditorThemeColors, EditorThemeFile, EditorThemePicker, EditorThemeRegistry, EditorThemeSwatch,
    ThemeColors, ThemeDirs, ThemeFile, ThemeRegistry, theme_store,
};

use crate::inject::{label, text};

/// Index of the first of the chrome palette's five optional grid slots.
///
/// Load-bearing for [`UiThemeCatalog`]: it is where [`UI_SLOTS`] stops being
/// required and where the derivation starts counting the grid slots from.
const UI_GRID_FIRST: usize = 11;

/// One entry of a catalogue, as the management row needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Stable id, which is also the stem of the file a custom entry lives in.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether the entry ships with the application rather than coming from a
    /// file.
    pub builtin: bool,
}

/// One slot of a palette, as the editor has to know it.
///
/// A slot is a colour with a name, a rule about how many hex digits it takes,
/// and — for a format that lets a key be left out — a rule about what an empty
/// field means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Element id fragment; never translated.
    ///
    /// The field is named after its slot rather than numbered, so that the
    /// element keeps its identity as the editor swaps one catalogue's field
    /// list for another's.
    pub key: &'static str,
    /// Key of the label shown to the left of the field.
    ///
    /// A key rather than the translated words, so that a slot list is a `const`
    /// and needs no [`App`] to be built — the editor translates as it draws,
    /// which is also what makes a language change show without rebuilding the
    /// list.
    pub label_key: &'static str,
    /// Whether this slot accepts an `#RRGGBBAA` value as well as `#RRGGBB`.
    pub alpha: bool,
    /// Whether the file may leave the slot out and have it derived.
    ///
    /// An empty field then means "derive it", and
    /// [`ThemeCatalog::derived_color`] is what says which colour that turned
    /// out to be.
    pub optional: bool,
}

/// A required slot.
pub const fn slot(key: &'static str, label_key: &'static str, alpha: bool) -> Slot {
    Slot {
        key,
        label_key,
        alpha,
        optional: false,
    }
}

/// A slot the file may omit, in which case it is derived.
pub const fn derived_slot(key: &'static str, label_key: &'static str, alpha: bool) -> Slot {
    Slot {
        key,
        label_key,
        alpha,
        optional: true,
    }
}

/// One palette file, in whichever format the catalogue holding it uses.
///
/// The two `rugpui` formats are named because this crate's own two catalogues
/// carry them and the editor's previews are drawn from them. Anything else a
/// host keeps a catalogue of travels as [`CatalogFile::Other`], which the
/// generic code moves around without ever looking inside — every question it
/// could ask of one is a [`ThemeCatalog`] method instead.
///
/// `Clone`, because [`CatalogActionEvent::Edit`](crate::CatalogActionEvent::Edit)
/// hands one to every subscriber through a gpui event, which is delivered as
/// `&Event` rather than by value — a host that wants to keep the file past the
/// callback has nothing to take it from but a clone. [`CatalogFile::Other`]
/// therefore holds an [`Arc`] rather than a [`Box`]: cloning it is what a
/// `Box<dyn Any>` cannot do on its own, and an `Arc`'s clone is cheap and needs
/// nothing of the value it wraps.
#[derive(Clone)]
pub enum CatalogFile {
    /// A chrome theme.
    UiTheme(Box<ThemeFile>),
    /// An editor theme.
    EditorTheme(Box<EditorThemeFile>),
    /// A palette of a kind only the host knows.
    Other(Arc<dyn Any + Send + Sync>),
}

impl std::fmt::Debug for CatalogFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UiTheme(file) => f.debug_tuple("UiTheme").field(file).finish(),
            Self::EditorTheme(file) => f.debug_tuple("EditorTheme").field(file).finish(),
            Self::Other(_) => f.write_str("Other(..)"),
        }
    }
}

/// Why a file the user picked could not be imported.
///
/// A type rather than a bare [`anyhow::Error`] because the three cases read
/// differently to the person who picked the file: one is "this is not a palette
/// file", one is "this is a palette file, but of the other kind", and one is
/// "this is a palette file of the right kind with a bad colour in it". Only the
/// first has anything to gain from the underlying error text.
#[derive(Debug)]
pub enum ImportError {
    /// The file could not be read, or does not parse as this catalogue's
    /// format.
    Unreadable(anyhow::Error),
    /// It parses — as some other catalogue's format, not the one it was offered
    /// to. Carries the key of the sentence saying which row to try instead,
    /// because only the catalogue that refused it knows what it is not.
    WrongKind(&'static str),
    /// A slot holds something that is not an `#RRGGBB` colour; the key of that
    /// slot's own label.
    BadColor(&'static str),
}

impl ImportError {
    /// The sentence shown under the management row, naming `file`.
    ///
    /// `file` is the file's name rather than its whole path: the path is
    /// already in the log, and a management row is not wide enough to print one
    /// without pushing everything else off the edge.
    pub fn message(&self, file: &str, cx: &App) -> SharedString {
        match self {
            Self::Unreadable(error) => text(
                cx,
                "settings.manage.import_unreadable",
                &[("file", file), ("error", &format!("{error:#}"))],
            ),
            Self::WrongKind(key) => text(cx, key, &[("file", file)]),
            Self::BadColor(slot) => text(
                cx,
                "settings.manage.import_bad_color",
                &[("file", file), ("slot", &label(cx, slot))],
            ),
        }
    }
}

/// Everything the editor and the management row need to know about one kind of
/// palette.
///
/// Implementations are stateless apart from the two things they are built with:
/// where the files go, and which entry is selected when the one in hand is
/// deleted.
pub trait ThemeCatalog: 'static {
    /// Key of the heading shown over the editor while one of this catalogue's
    /// entries is being edited.
    fn kind_label_key(&self) -> &'static str;

    /// Prefix of the element ids of this catalogue's management row.
    ///
    /// Static, and never translated: gpui element ids only have to be unique
    /// among their siblings, and two rows are siblings within one form.
    fn element_prefix(&self) -> &'static str;

    /// Key of the question the delete confirmation asks.
    fn delete_confirm_key(&self) -> &'static str;

    /// Every entry, the built-in ones first and then the user's own.
    fn entries(&self, cx: &App) -> Vec<CatalogEntry>;

    /// The palette's slots, in the order [`ThemeCatalog::values_of`] and
    /// [`ThemeCatalog::file_from`] read and write them.
    fn slots(&self) -> &'static [Slot];

    /// The file that would reproduce the entry `id` names.
    ///
    /// Resolved through whatever registry the entries came from rather than
    /// read back off the disk, so a built-in entry — which has no file —
    /// duplicates exactly like one of the user's own.
    fn load(&self, id: &str, cx: &App) -> Option<CatalogFile>;

    /// The value of every slot of `file`, in [`ThemeCatalog::slots`] order,
    /// with whether the palette is a dark one.
    ///
    /// An omitted optional slot becomes an empty string, which is what the
    /// editor reads as "derive it".
    fn values_of(&self, file: &CatalogFile) -> (Vec<String>, bool);

    /// A file carrying `name`, `values` and `dark`.
    ///
    /// The inverse of [`ThemeCatalog::values_of`], and it has to be: what the
    /// editor saves is what it read, minus the edits.
    fn file_from(&self, name: String, values: &[String], dark: bool) -> CatalogFile;

    /// Directory this catalogue's user files live in.
    ///
    /// Not created by this call: a user who has added no palette of their own
    /// has no such directory, and every caller copes with that.
    ///
    /// # Errors
    ///
    /// Fails when the host named no directory for this kind of palette.
    fn dir(&self) -> Result<PathBuf>;

    /// The id selected when the one in hand has just been deleted.
    fn default_id(&self) -> String;

    /// Prefix of the ids made up for an entry whose name yields no slug.
    fn generated_id_prefix(&self) -> &'static str;

    /// Writes `file` into the configuration directory under `id`.
    ///
    /// # Errors
    ///
    /// Fails for an unusable id, one belonging to a built-in entry, a write
    /// that does not go through, or no directory to write into.
    fn save(&self, id: &str, file: &CatalogFile) -> Result<PathBuf>;

    /// Writes `file` to `path`, wherever on the disk that is.
    ///
    /// The counterpart of [`ThemeCatalog::save`], which decides the path itself
    /// from an id: an export goes where the user pointed the save dialog.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be serialized or cannot be written.
    fn write(&self, file: &CatalogFile, path: &Path) -> Result<()>;

    /// Removes the file `id` lives in.
    ///
    /// # Errors
    ///
    /// Fails when `id` has no usable slug, the file cannot be removed, or there
    /// is no configuration directory to remove it from.
    fn delete(&self, id: &str) -> Result<()>;

    /// Parses `path` as one of this catalogue's files, refusing anything that
    /// is not one; see the module docs.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read, does not parse in this format,
    /// parses in another one, or holds a value that is not a colour.
    fn read(&self, path: &Path) -> std::result::Result<CatalogFile, ImportError>;

    /// Reloads whichever registry the entries come from.
    ///
    /// Called after every save and every delete, because the registries are
    /// swapped whole rather than edited in place.
    fn reload(&self, cx: &mut App);

    /// The name `file` carries.
    ///
    /// The default handles the two formats this crate knows; a catalogue over
    /// [`CatalogFile::Other`] overrides it.
    fn name_of(&self, file: &CatalogFile) -> String {
        match file {
            CatalogFile::UiTheme(file) => file.name.clone(),
            CatalogFile::EditorTheme(file) => file.name.clone(),
            CatalogFile::Other(_) => String::new(),
        }
    }

    /// Replaces the name `file` carries.
    fn set_name(&self, file: &mut CatalogFile, name: String) {
        match file {
            CatalogFile::UiTheme(file) => file.name = name,
            CatalogFile::EditorTheme(file) => file.name = name,
            CatalogFile::Other(_) => {}
        }
    }

    /// The colour the palette would derive for the optional slot at `index`.
    ///
    /// `None` for a slot that is not optional, and for a catalogue whose format
    /// has no derivations at all — which is the default.
    fn derived_color(&self, _index: usize, _dark: bool, _values: &[String]) -> Option<Hsla> {
        None
    }

    /// The miniature the editor draws above its fields, over the values as they
    /// stand.
    ///
    /// A palette is judged by what it looks like, not by sixteen hex strings,
    /// and what makes a useful miniature differs entirely between a window
    /// chrome and a syntax palette — so it is the catalogue that draws one.
    fn render_preview(
        &self,
        id: &str,
        name: SharedString,
        values: &[String],
        dark: bool,
        cx: &mut App,
    ) -> gpui::AnyElement;

    /// The index the editor's optional slots start at, if it has any.
    ///
    /// The editor puts a heading over them, because otherwise they would run on
    /// from the required ones with nothing to say that these are the ones the
    /// file may leave out. `None` for a format whose slots are all required.
    fn optional_group_start(&self) -> Option<usize> {
        None
    }

    /// Headings the editor draws inside its field list, each before the slot
    /// its index names.
    ///
    /// `(slot index, label key)`, and the editor looks the key up in the
    /// host's own words like every other string here. The default is no
    /// headings at all, which is what a format with sixteen slots and one
    /// meaning wants; a format whose slots fall into named families — chrome,
    /// grid, diagnostics — names the first slot of each and gets a list a
    /// person can navigate.
    ///
    /// Independent of [`ThemeCatalog::optional_group_start`], which draws its
    /// own heading over the optional tail; an index inside that tail is drawn
    /// there just the same. An index past the last slot is ignored rather than
    /// drawn over nothing, and two headings on one index are drawn in the
    /// order given.
    fn group_headings(&self) -> Vec<(usize, &'static str)> {
        Vec::new()
    }

    /// Whether the format has a dark/light flag at all.
    ///
    /// The editor draws a checkbox for it when it does, and leaves the value
    /// [`ThemeCatalog::values_of`] reported untouched when it does not — a
    /// format that carries no such flag has [`ThemeCatalog::file_from`] handed
    /// back exactly what it gave out, rather than a `false` the editor
    /// invented. Both formats here carry one, so the default is `true`.
    fn has_dark_flag(&self) -> bool {
        true
    }

    /// The entry `id` names, or `None` when nothing answers to it.
    fn entry(&self, id: &str, cx: &App) -> Option<CatalogEntry> {
        self.entries(cx)
            .into_iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(id))
    }

    /// Every id already spoken for, which is what a new one has to dodge.
    fn taken_ids(&self, cx: &App) -> Vec<String> {
        self.entries(cx).into_iter().map(|entry| entry.id).collect()
    }

    /// Refuses a file that parses but holds something which is not a colour.
    ///
    /// Checked against the very table the editor's fields are built from, so a
    /// value the import accepts is one the editor would also let be saved
    /// again. Only the first offending slot is reported: a hand-written file
    /// with one typo is the common case, and listing nineteen slots would bury
    /// it.
    fn validate(&self, file: &CatalogFile) -> std::result::Result<(), ImportError> {
        let (values, _dark) = self.values_of(file);
        for (slot, value) in self.slots().iter().zip(&values) {
            if !valid_hex(value, slot.alpha, slot.optional) {
                return Err(ImportError::BadColor(slot.label_key));
            }
        }
        Ok(())
    }
}

/// Whether `value` is a colour a palette file accepts.
///
/// Stricter than [`rugpui::parse_hex`] on purpose: that helper takes an alpha
/// channel wherever it finds one, while only a handful of slots are ever
/// *drawn* with one, and a stray eighth digit on an opaque slot is a mistake
/// worth pointing at. An `optional` slot also accepts nothing at all, which is
/// how the file says "derive this one".
pub fn valid_hex(value: &str, alpha: bool, optional: bool) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return optional;
    }
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let length_ok = digits.len() == 6 || (alpha && digits.len() == 8);
    length_ok && rugpui::parse_hex(trimmed).is_some()
}

/// The chrome themes of [`ThemeRegistry`], as a catalogue.
pub struct UiThemeCatalog {
    /// Where the user's own files go.
    dirs: ThemeDirs,
    /// The id selected when the one in hand has just been deleted.
    default_id: String,
}

impl UiThemeCatalog {
    /// A catalogue over `dirs`, falling back to `default_id`.
    pub fn new(dirs: ThemeDirs, default_id: impl Into<String>) -> Self {
        Self {
            dirs,
            default_id: default_id.into(),
        }
    }

    /// The file, when it is one of ours.
    fn file(file: &CatalogFile) -> Option<&ThemeFile> {
        match file {
            CatalogFile::UiTheme(file) => Some(file),
            _ => None,
        }
    }
}

/// The editor themes of [`EditorThemeRegistry`], as a catalogue.
pub struct EditorThemeCatalog {
    /// Where the user's own files go.
    dirs: ThemeDirs,
    /// The id selected when the one in hand has just been deleted.
    default_id: String,
}

impl EditorThemeCatalog {
    /// A catalogue over `dirs`, falling back to `default_id`.
    pub fn new(dirs: ThemeDirs, default_id: impl Into<String>) -> Self {
        Self {
            dirs,
            default_id: default_id.into(),
        }
    }

    /// The file, when it is one of ours.
    fn file(file: &CatalogFile) -> Option<&EditorThemeFile> {
        match file {
            CatalogFile::EditorTheme(file) => Some(file),
            _ => None,
        }
    }
}

impl ThemeCatalog for UiThemeCatalog {
    fn kind_label_key(&self) -> &'static str {
        "settings.editor.theme_title"
    }

    fn element_prefix(&self) -> &'static str {
        "settings-ui-theme-action"
    }

    fn delete_confirm_key(&self) -> &'static str {
        "settings.manage.delete_theme_confirm"
    }

    fn entries(&self, cx: &App) -> Vec<CatalogEntry> {
        ThemeRegistry::all(cx)
            .into_iter()
            .map(|entry| CatalogEntry {
                id: entry.id,
                name: entry.name,
                builtin: entry.builtin,
            })
            .collect()
    }

    fn slots(&self) -> &'static [Slot] {
        &UI_SLOTS
    }

    fn load(&self, id: &str, cx: &App) -> Option<CatalogFile> {
        let entry = self.entry(id, cx)?;
        // Through the registry rather than off the disk, which is also why a
        // duplicated chrome theme arrives with its five grid slots spelled out:
        // `ThemeFile::from_theme` writes the values that were derived on the
        // way in, and a copy the user is about to edit should show what it is
        // actually wearing.
        Some(CatalogFile::UiTheme(Box::new(ThemeFile::from_theme(
            entry.name,
            &ThemeRegistry::resolve(id, cx),
        ))))
    }

    fn values_of(&self, file: &CatalogFile) -> (Vec<String>, bool) {
        match Self::file(file) {
            Some(file) => (ui_values(&file.colors), file.dark),
            None => (Vec::new(), false),
        }
    }

    fn file_from(&self, name: String, values: &[String], dark: bool) -> CatalogFile {
        CatalogFile::UiTheme(Box::new(ThemeFile::new(name, dark, ui_colors(values))))
    }

    fn dir(&self) -> Result<PathBuf> {
        Ok(self.dirs.ui_themes.clone())
    }

    fn default_id(&self) -> String {
        self.default_id.clone()
    }

    fn generated_id_prefix(&self) -> &'static str {
        theme_store::GENERATED_THEME_ID
    }

    fn save(&self, id: &str, file: &CatalogFile) -> Result<PathBuf> {
        let file = Self::file(file).ok_or_else(|| anyhow::anyhow!("not a chrome theme"))?;
        theme_store::save_ui_theme(&self.dirs, id, file)
    }

    fn write(&self, file: &CatalogFile, path: &Path) -> Result<()> {
        let file = Self::file(file).ok_or_else(|| anyhow::anyhow!("not a chrome theme"))?;
        theme_store::write_file(path, file)
    }

    fn delete(&self, id: &str) -> Result<()> {
        theme_store::delete_ui_theme(&self.dirs, id)
    }

    fn read(&self, path: &Path) -> std::result::Result<CatalogFile, ImportError> {
        let file = match theme_store::read_file::<ThemeFile>(path) {
            Ok(file) => CatalogFile::UiTheme(Box::new(file)),
            // The two formats can always be told apart because neither one's
            // required keys are a subset of the other's: a chrome palette has
            // to carry `surface`, an editor palette `foreground`. One more read
            // of a file already in the page cache is the difference between
            // "this file is broken" and "this file belongs under the other
            // picker".
            Err(_) if theme_store::read_file::<EditorThemeFile>(path).is_ok() => {
                return Err(ImportError::WrongKind("settings.manage.import_not_a_theme"));
            }
            Err(error) => return Err(ImportError::Unreadable(error)),
        };
        self.validate(&file)?;
        Ok(file)
    }

    fn reload(&self, cx: &mut App) {
        theme_store::reload(&self.dirs, cx);
    }

    fn derived_color(&self, index: usize, dark: bool, values: &[String]) -> Option<Hsla> {
        derived_ui_color(index, dark, values)
    }

    fn optional_group_start(&self) -> Option<usize> {
        Some(UI_GRID_FIRST)
    }

    fn render_preview(
        &self,
        _id: &str,
        name: SharedString,
        values: &[String],
        dark: bool,
        cx: &mut App,
    ) -> gpui::AnyElement {
        crate::theme_editor::render_ui_preview(name, values, dark, cx)
    }
}

impl ThemeCatalog for EditorThemeCatalog {
    fn kind_label_key(&self) -> &'static str {
        "settings.editor.editor_theme_title"
    }

    fn element_prefix(&self) -> &'static str {
        "settings-editor-theme-action"
    }

    fn delete_confirm_key(&self) -> &'static str {
        "settings.manage.delete_editor_theme_confirm"
    }

    fn entries(&self, cx: &App) -> Vec<CatalogEntry> {
        EditorThemeRegistry::all(cx)
            .into_iter()
            .map(|entry| CatalogEntry {
                id: entry.id,
                name: entry.name,
                builtin: entry.builtin,
            })
            .collect()
    }

    fn slots(&self) -> &'static [Slot] {
        &EDITOR_SLOTS
    }

    fn load(&self, id: &str, cx: &App) -> Option<CatalogFile> {
        let entry = self.entry(id, cx)?;
        Some(CatalogFile::EditorTheme(Box::new(
            EditorThemeFile::from_theme(entry.name, &EditorThemeRegistry::resolve(id, cx)),
        )))
    }

    fn values_of(&self, file: &CatalogFile) -> (Vec<String>, bool) {
        match Self::file(file) {
            Some(file) => (editor_values(&file.colors), file.dark),
            None => (Vec::new(), false),
        }
    }

    fn file_from(&self, name: String, values: &[String], dark: bool) -> CatalogFile {
        CatalogFile::EditorTheme(Box::new(EditorThemeFile::new(
            name,
            dark,
            editor_colors(values),
        )))
    }

    fn dir(&self) -> Result<PathBuf> {
        self.dirs
            .editor_themes
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no editor theme directory"))
    }

    fn default_id(&self) -> String {
        self.default_id.clone()
    }

    fn generated_id_prefix(&self) -> &'static str {
        theme_store::GENERATED_EDITOR_THEME_ID
    }

    fn save(&self, id: &str, file: &CatalogFile) -> Result<PathBuf> {
        let file = Self::file(file).ok_or_else(|| anyhow::anyhow!("not an editor theme"))?;
        theme_store::save_editor_theme(&self.dirs, id, file)
    }

    fn write(&self, file: &CatalogFile, path: &Path) -> Result<()> {
        let file = Self::file(file).ok_or_else(|| anyhow::anyhow!("not an editor theme"))?;
        theme_store::write_file(path, file)
    }

    fn delete(&self, id: &str) -> Result<()> {
        theme_store::delete_editor_theme(&self.dirs, id)
    }

    fn read(&self, path: &Path) -> std::result::Result<CatalogFile, ImportError> {
        let file = match theme_store::read_file::<EditorThemeFile>(path) {
            Ok(file) => CatalogFile::EditorTheme(Box::new(file)),
            Err(_) if theme_store::read_file::<ThemeFile>(path).is_ok() => {
                return Err(ImportError::WrongKind(
                    "settings.manage.import_not_an_editor_theme",
                ));
            }
            Err(error) => return Err(ImportError::Unreadable(error)),
        };
        self.validate(&file)?;
        Ok(file)
    }

    fn reload(&self, cx: &mut App) {
        theme_store::reload(&self.dirs, cx);
    }

    fn render_preview(
        &self,
        id: &str,
        name: SharedString,
        values: &[String],
        dark: bool,
        _cx: &mut App,
    ) -> gpui::AnyElement {
        // Rendered by the same widget a settings dialog picks editor themes
        // with, over a single card: a syntax palette is judged by whether its
        // classes can be told apart in an actual statement, and a second
        // preview here would be a second chance to disagree with the picker
        // about that.
        let palette = EditorThemeFile::new("", dark, editor_colors(values)).to_theme();
        gpui::IntoElement::into_any_element(
            EditorThemePicker::new("theme-editor-preview")
                .options([EditorThemeSwatch::new(id.to_owned(), name).preview(palette)])
                .selected(Some(id.to_owned()))
                .columns(1),
        )
    }
}

/// The sixteen chrome slots, in the order [`ThemeColors`] declares them.
///
/// The order is load-bearing: [`ui_colors`] reads the fields back by position,
/// and everything from [`UI_GRID_FIRST`] on is optional.
const UI_SLOTS: [Slot; 16] = [
    slot("background", "settings.editor.slot.background", false),
    slot("surface", "settings.editor.slot.surface", false),
    slot("surface_hover", "settings.editor.slot.surface_hover", false),
    slot(
        "surface_active",
        "settings.editor.slot.surface_active",
        false,
    ),
    slot("border", "settings.editor.slot.border", false),
    slot("text", "settings.editor.slot.text", false),
    slot("text_muted", "settings.editor.slot.text_muted", false),
    slot("accent", "settings.editor.slot.accent", false),
    slot("danger", "settings.editor.slot.danger", false),
    slot("success", "settings.editor.slot.success", false),
    // The one required slot that is drawn translucent, and so the one that may
    // carry an eighth and ninth hex digit.
    slot("overlay", "settings.editor.slot.overlay", true),
    // The five the format made optional, so that a theme file written before
    // the result grid existed still loads.
    derived_slot("grid_header", "settings.editor.slot.grid_header", false),
    derived_slot("grid_row_alt", "settings.editor.slot.grid_row_alt", false),
    derived_slot(
        "grid_selection",
        "settings.editor.slot.grid_selection",
        true,
    ),
    derived_slot("grid_null", "settings.editor.slot.grid_null", false),
    derived_slot("grid_pk", "settings.editor.slot.grid_pk", false),
];

/// The nineteen editor slots, in the order [`EditorThemeColors`] declares them.
///
/// As with [`UI_SLOTS`], the order is what [`editor_colors`] reads back. The two
/// bands drawn *behind* text are the ones that may carry alpha: a selection and
/// a current-line highlight both have to let the glyph under them show.
const EDITOR_SLOTS: [Slot; 21] = [
    slot("background", "settings.editor.slot.background", false),
    slot("foreground", "settings.editor.code.foreground", false),
    slot("cursor", "settings.editor.code.cursor", false),
    slot("selection", "settings.editor.code.selection", true),
    slot(
        "line_highlight",
        "settings.editor.code.line_highlight",
        true,
    ),
    slot("gutter", "settings.editor.code.gutter", false),
    slot("gutter_active", "settings.editor.code.gutter_active", false),
    slot("keyword", "settings.editor.code.keyword", false),
    slot("string", "settings.editor.code.string", false),
    slot("number", "settings.editor.code.number", false),
    slot("comment", "settings.editor.code.comment", false),
    slot("function", "settings.editor.code.function", false),
    slot("type", "settings.editor.code.type", false),
    slot("operator", "settings.editor.code.operator", false),
    slot("identifier", "settings.editor.code.identifier", false),
    slot("key", "settings.editor.code.key", false),
    slot("variable", "settings.editor.code.variable", false),
    slot("punctuation", "settings.editor.code.punctuation", false),
    slot("bracket_match", "settings.editor.code.bracket_match", false),
    slot("error", "settings.editor.code.error", false),
    slot("warning", "settings.editor.code.warning", false),
];

/// The current value of every chrome slot, in [`UI_SLOTS`] order.
///
/// An omitted grid slot becomes an empty field, which is what the editor reads
/// as "derive it".
fn ui_values(colors: &ThemeColors) -> Vec<String> {
    let optional = |value: &Option<String>| value.clone().unwrap_or_default();
    vec![
        colors.background.clone(),
        colors.surface.clone(),
        colors.surface_hover.clone(),
        colors.surface_active.clone(),
        colors.border.clone(),
        colors.text.clone(),
        colors.text_muted.clone(),
        colors.accent.clone(),
        colors.danger.clone(),
        colors.success.clone(),
        colors.overlay.clone(),
        optional(&colors.grid_header),
        optional(&colors.grid_row_alt),
        optional(&colors.grid_selection),
        optional(&colors.grid_null),
        optional(&colors.grid_pk),
    ]
}

/// The chrome slots, read back out of the fields in [`UI_SLOTS`] order.
///
/// An empty optional field is written back as an absent key rather than as an
/// empty string: the loader treats a blank the same way it treats a typo, and
/// the whole point of clearing a grid slot is to get the derivation back.
pub(crate) fn ui_colors(values: &[String]) -> ThemeColors {
    let at = |index: usize| values.get(index).cloned().unwrap_or_default();
    let optional = |index: usize| {
        let value = at(index);
        (!value.trim().is_empty()).then_some(value)
    };
    ThemeColors {
        background: at(0),
        surface: at(1),
        surface_hover: at(2),
        surface_active: at(3),
        border: at(4),
        text: at(5),
        text_muted: at(6),
        accent: at(7),
        danger: at(8),
        success: at(9),
        overlay: at(10),
        grid_header: optional(11),
        grid_row_alt: optional(12),
        grid_selection: optional(13),
        grid_null: optional(14),
        grid_pk: optional(15),
    }
}

/// The colour the palette would derive for the grid slot at `index`.
///
/// Worked out by asking [`ThemeFile::to_theme`] the same question the loader
/// asks: the slot is blanked out, the rest of the fields are left as they are,
/// and whatever comes back is what the file would resolve to without that key.
/// `None` for an index that is not one of the five.
fn derived_ui_color(index: usize, dark: bool, values: &[String]) -> Option<Hsla> {
    let grid_slot = index.checked_sub(UI_GRID_FIRST)?;
    let mut values = values.to_vec();
    *values.get_mut(index)? = String::new();
    let palette = ThemeFile::new("", dark, ui_colors(&values)).to_theme();
    match grid_slot {
        0 => Some(palette.grid_header),
        1 => Some(palette.grid_row_alt),
        2 => Some(palette.grid_selection),
        3 => Some(palette.grid_null),
        4 => Some(palette.grid_pk),
        _ => None,
    }
}

/// The current value of every editor slot, in [`EDITOR_SLOTS`] order.
fn editor_values(colors: &EditorThemeColors) -> Vec<String> {
    vec![
        colors.background.clone(),
        colors.foreground.clone(),
        colors.cursor.clone(),
        colors.selection.clone(),
        colors.line_highlight.clone(),
        colors.gutter.clone(),
        colors.gutter_active.clone(),
        colors.keyword.clone(),
        colors.string.clone(),
        colors.number.clone(),
        colors.comment.clone(),
        colors.function.clone(),
        colors.r#type.clone(),
        colors.operator.clone(),
        colors.identifier.clone(),
        colors.key.clone(),
        colors.variable.clone(),
        colors.punctuation.clone(),
        colors.bracket_match.clone(),
        colors.error.clone(),
        colors.warning.clone(),
    ]
}

/// The editor slots, read back out of the fields in [`EDITOR_SLOTS`] order.
pub(crate) fn editor_colors(values: &[String]) -> EditorThemeColors {
    let at = |index: usize| values.get(index).cloned().unwrap_or_default();
    EditorThemeColors {
        background: at(0),
        foreground: at(1),
        cursor: at(2),
        selection: at(3),
        line_highlight: at(4),
        gutter: at(5),
        gutter_active: at(6),
        keyword: at(7),
        string: at(8),
        number: at(9),
        comment: at(10),
        function: at(11),
        r#type: at(12),
        operator: at(13),
        identifier: at(14),
        key: at(15),
        variable: at(16),
        punctuation: at(17),
        bracket_match: at(18),
        error: at(19),
        warning: at(20),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rugpui::{EditorTheme, Theme, to_hex};

    use super::*;

    /// A catalogue over a scratch directory, for the questions that do not
    /// touch the disk at all.
    fn ui() -> UiThemeCatalog {
        UiThemeCatalog::new(dirs(Path::new("/nowhere")), "dark")
    }

    /// The editor counterpart of [`ui`].
    fn editor() -> EditorThemeCatalog {
        EditorThemeCatalog::new(dirs(Path::new("/nowhere")), "one-dark")
    }

    /// Both directories under `root`.
    fn dirs(root: &Path) -> ThemeDirs {
        ThemeDirs {
            ui_themes: root.join("themes"),
            editor_themes: Some(root.join("editor-themes")),
        }
    }

    /// A chrome file worth round-tripping.
    fn chrome_file() -> CatalogFile {
        CatalogFile::UiTheme(Box::new(ThemeFile::from_theme("Mine", &Theme::dracula())))
    }

    /// An editor file worth round-tripping.
    fn editor_file() -> CatalogFile {
        CatalogFile::EditorTheme(Box::new(EditorThemeFile::from_theme(
            "Mine",
            &EditorTheme::one_dark(),
        )))
    }

    #[test]
    fn a_six_digit_colour_is_accepted_everywhere() {
        for value in ["#ff0000", "ff0000", "  #AABBCC  "] {
            for alpha in [false, true] {
                for optional in [false, true] {
                    assert!(valid_hex(value, alpha, optional), "refused {value:?}");
                }
            }
        }
    }

    #[test]
    fn only_a_slot_with_alpha_takes_eight_digits() {
        assert!(valid_hex("#0000009e", true, false));
        assert!(!valid_hex("#0000009e", false, false));
    }

    #[test]
    fn only_an_optional_slot_may_be_left_empty() {
        for value in ["", "   "] {
            assert!(valid_hex(value, false, true), "refused {value:?}");
            assert!(!valid_hex(value, false, false), "accepted {value:?}");
        }
    }

    #[test]
    fn anything_that_is_not_a_colour_is_refused() {
        for value in ["#", "#abc", "#abcde", "#gghhii", "rebeccapurple"] {
            for alpha in [false, true] {
                for optional in [false, true] {
                    assert!(!valid_hex(value, alpha, optional), "accepted {value:?}");
                }
            }
        }
    }

    #[test]
    fn the_chrome_slots_round_trip_through_the_fields() {
        let file = ThemeFile::from_theme("Mine", &Theme::solarized_light());
        let (values, dark) = ui().values_of(&CatalogFile::UiTheme(Box::new(file.clone())));
        assert_eq!(values.len(), UI_SLOTS.len());
        assert_eq!(values.len(), 16);
        assert_eq!(dark, file.dark);
        assert_eq!(ui_colors(&values), file.colors);
    }

    #[test]
    fn the_editor_slots_round_trip_through_the_fields() {
        let file = EditorThemeFile::from_theme("Mine", &EditorTheme::dracula());
        let (values, dark) = editor().values_of(&CatalogFile::EditorTheme(Box::new(file.clone())));
        assert_eq!(values.len(), EDITOR_SLOTS.len());
        assert_eq!(values.len(), 21);
        assert_eq!(dark, file.dark);
        assert_eq!(editor_colors(&values), file.colors);
    }

    #[test]
    fn what_the_editor_saves_is_what_it_read() {
        // `file_from` is the inverse of `values_of`, and it has to be: the
        // editor reads a file into fields and writes the fields back out.
        for (catalog, original) in [
            (Box::new(ui()) as Box<dyn ThemeCatalog>, chrome_file()),
            (Box::new(editor()) as Box<dyn ThemeCatalog>, editor_file()),
        ] {
            let (values, dark) = catalog.values_of(&original);
            let rebuilt = catalog.file_from(catalog.name_of(&original), &values, dark);
            assert_eq!(catalog.name_of(&rebuilt), "Mine");
            assert_eq!(catalog.values_of(&rebuilt), (values, dark));
        }
    }

    #[test]
    fn a_name_can_be_read_and_replaced_whichever_format_it_is() {
        for catalog in [
            Box::new(ui()) as Box<dyn ThemeCatalog>,
            Box::new(editor()) as Box<dyn ThemeCatalog>,
        ] {
            let mut file = if catalog.slots().len() == 16 {
                chrome_file()
            } else {
                editor_file()
            };
            assert_eq!(catalog.name_of(&file), "Mine");
            catalog.set_name(&mut file, "Mine, copy".to_string());
            assert_eq!(catalog.name_of(&file), "Mine, copy");
        }
    }

    #[test]
    fn an_omitted_grid_slot_is_an_empty_field_and_stays_omitted() {
        // The distinction the editor exists to show: a file that leaves the
        // grid out has to open with five empty fields, and saving it again
        // without touching them must not turn those into explicit colours.
        let mut file = ThemeFile::from_theme("Mine", &Theme::dracula());
        file.colors.grid_header = None;
        file.colors.grid_row_alt = None;
        file.colors.grid_selection = None;
        file.colors.grid_null = None;
        file.colors.grid_pk = None;

        let (values, _dark) = ui().values_of(&CatalogFile::UiTheme(Box::new(file.clone())));
        assert!(values[UI_GRID_FIRST..].iter().all(String::is_empty));
        assert_eq!(ui_colors(&values), file.colors);

        // And a whitespace-only field means the same thing as an empty one.
        let mut typed = values.clone();
        typed[UI_GRID_FIRST] = "   ".to_string();
        assert_eq!(ui_colors(&typed).grid_header, None);
    }

    #[test]
    fn an_omitted_grid_slot_still_has_a_colour_to_show() {
        // The swatch must never go blank: an automatic slot shows whatever the
        // palette derived, which is what the loader would have used.
        let mut file = ThemeFile::from_theme("Mine", &Theme::light());
        file.colors.grid_header = None;
        let (values, _dark) = ui().values_of(&CatalogFile::UiTheme(Box::new(file.clone())));

        let derived = ui()
            .derived_color(UI_GRID_FIRST, false, &values)
            .expect("a grid slot");
        assert_eq!(to_hex(derived), to_hex(file.to_theme().grid_header));
        // A required slot has nothing to derive.
        assert_eq!(ui().derived_color(0, false, &values), None);
        // And a format with no optional slots derives nothing at all.
        assert_eq!(editor().derived_color(0, false, &values), None);
        assert_eq!(editor().optional_group_start(), None);
        assert_eq!(ui().optional_group_start(), Some(UI_GRID_FIRST));
    }

    #[test]
    fn a_spelled_out_grid_slot_wins_over_the_derivation() {
        let file = ThemeFile::from_theme("Mine", &Theme::dark());
        let (mut values, _dark) = ui().values_of(&CatalogFile::UiTheme(Box::new(file)));
        values[UI_GRID_FIRST] = "#123456".to_string();
        let palette = ThemeFile::new("", true, ui_colors(&values)).to_theme();
        assert_eq!(to_hex(palette.grid_header), "#123456");
        // And the derivation is still what clearing it would go back to.
        assert_ne!(
            to_hex(
                ui().derived_color(UI_GRID_FIRST, true, &values)
                    .expect("derived")
            ),
            "#123456"
        );
    }

    #[test]
    fn every_builtin_palette_writes_values_its_own_fields_accept() {
        // The alpha flags are a claim about which slots are drawn translucent.
        // A built-in whose file carries an eighth digit in a slot marked opaque
        // would open in the editor already refused, which is the one way this
        // table can be wrong without anybody noticing.
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::solarized_dark(),
            Theme::solarized_light(),
            Theme::gruvbox_dark(),
            Theme::dracula(),
        ] {
            let file = CatalogFile::UiTheme(Box::new(ThemeFile::from_theme("X", &theme)));
            assert!(ui().validate(&file).is_ok(), "{:?}", ui().validate(&file));
        }

        for theme in [
            EditorTheme::one_dark(),
            EditorTheme::one_light(),
            EditorTheme::solarized_dark(),
            EditorTheme::solarized_light(),
            EditorTheme::gruvbox_dark(),
            EditorTheme::dracula(),
        ] {
            let file = CatalogFile::EditorTheme(Box::new(EditorThemeFile::from_theme("X", &theme)));
            assert!(editor().validate(&file).is_ok());
        }
    }

    /// The one thing [`ThemeCatalog::save`] adds over the store it delegates to
    /// is picking the right directory and the right table of reserved ids, and
    /// getting that backwards would let an editor theme called `dracula` — a
    /// chrome id, and a free editor id — be refused.
    #[test]
    fn a_builtin_id_is_refused_by_the_catalogue_that_reserves_it() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let ui = UiThemeCatalog::new(dirs(root.path()), "dark");
        let editor = EditorThemeCatalog::new(dirs(root.path()), "one-dark");

        assert!(
            ui.save("dracula", &chrome_file()).is_err(),
            "a built-in chrome id"
        );
        assert!(
            editor.save("one-dark", &editor_file()).is_err(),
            "a built-in editor id"
        );
        // A name with nothing to slug cannot become a file name either.
        assert!(ui.save("   ", &chrome_file()).is_err());
        assert!(editor.save("테마", &editor_file()).is_err());
        // A free id, on the other hand, lands where the catalogue says it does.
        let written = ui.save("mine", &chrome_file()).expect("a free chrome id");
        assert!(written.starts_with(ui.dir().expect("a chrome directory")));
        let written = editor
            .save("mine", &editor_file())
            .expect("a free editor id");
        assert!(written.starts_with(editor.dir().expect("an editor directory")));
    }

    /// A catalogue refuses a file of the other format outright, so that a
    /// `save` cannot quietly write a chrome palette into the editor directory.
    #[test]
    fn a_catalogue_never_writes_a_file_of_the_other_kind() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let ui = UiThemeCatalog::new(dirs(root.path()), "dark");
        let editor = EditorThemeCatalog::new(dirs(root.path()), "one-dark");

        assert!(ui.save("mine", &editor_file()).is_err());
        assert!(editor.save("mine", &chrome_file()).is_err());
        let path = root.path().join("out.json");
        assert!(ui.write(&editor_file(), &path).is_err());
        assert!(editor.write(&chrome_file(), &path).is_err());
    }

    #[test]
    fn an_exported_file_reads_back_as_the_entry_it_came_from() {
        // The whole point of the pair: what an export writes is what an import
        // takes, for both catalogues and without a trip through the settings
        // directory. A built-in is used on purpose — exporting one is how a
        // user gets a starting point, so it has to survive the round trip.
        let directory = tempfile::tempdir().expect("a temporary directory");

        let ui = ui();
        let editor = editor();
        let cases: [(&dyn ThemeCatalog, CatalogFile); 2] =
            [(&ui, chrome_file()), (&editor, editor_file())];
        for (catalog, original) in cases {
            let path = directory
                .path()
                .join(format!("{}.json", catalog.element_prefix()));
            catalog.write(&original, &path).expect("the export");

            let read = catalog.read(&path).expect("the import");
            assert_eq!(catalog.name_of(&read), catalog.name_of(&original));
            assert_eq!(catalog.values_of(&read), catalog.values_of(&original));
        }
    }

    #[test]
    fn a_file_of_the_other_catalogue_is_refused_by_name() {
        // The refusal this module exists for: the two formats are close enough
        // to look interchangeable, and installing half a palette because the
        // absent keys defaulted would be the worst outcome of the two.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("theme.json");
        let ui = ui();
        let editor = editor();

        ui.write(&chrome_file(), &path).expect("the export");
        match editor.read(&path) {
            Err(ImportError::WrongKind(key)) => {
                assert_eq!(key, "settings.manage.import_not_an_editor_theme");
            }
            other => panic!("the editor catalogue accepted a chrome file: {other:?}"),
        }

        editor.write(&editor_file(), &path).expect("the export");
        match ui.read(&path) {
            Err(ImportError::WrongKind(key)) => {
                assert_eq!(key, "settings.manage.import_not_a_theme");
            }
            other => panic!("the chrome catalogue accepted an editor file: {other:?}"),
        }
    }

    #[test]
    fn a_file_that_is_not_a_theme_at_all_is_refused_rather_than_panicking() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("broken.json");
        let ui = ui();
        let editor = editor();
        let catalogs: [&dyn ThemeCatalog; 2] = [&ui, &editor];

        // Not JSON, valid JSON of the wrong shape, JSON that stops halfway, an
        // empty file, and a path with nothing behind it at all.
        for contents in [
            "not json at all",
            "[]",
            "{\"name\": \"Mine\", \"colors\":",
            "{}",
            "",
        ] {
            fs::write(&path, contents).expect("the fixture");
            for catalog in catalogs {
                assert!(
                    matches!(catalog.read(&path), Err(ImportError::Unreadable(_))),
                    "{} accepted {contents:?}",
                    catalog.element_prefix()
                );
            }
        }

        let missing = directory.path().join("no-such-file.json");
        assert!(matches!(ui.read(&missing), Err(ImportError::Unreadable(_))));
    }

    #[test]
    fn a_slot_that_is_not_a_colour_is_refused_by_the_slot_it_belongs_to() {
        // `ThemeFile::to_theme` would happily substitute its fallback here,
        // which is right for a directory scan and wrong for one deliberate
        // import: the user has to be told the file is not what it claims.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("theme.json");
        let ui = ui();
        let editor = editor();

        let CatalogFile::UiTheme(mut chrome) = chrome_file() else {
            unreachable!("a chrome file");
        };
        chrome.colors.accent = "rebeccapurple".to_string();
        ui.write(&CatalogFile::UiTheme(chrome), &path)
            .expect("write");
        match ui.read(&path) {
            Err(ImportError::BadColor(slot)) => {
                assert_eq!(slot, "settings.editor.slot.accent");
            }
            other => panic!("accepted a colour that is not one: {other:?}"),
        }

        // An eighth digit on a slot that is never drawn translucent is refused
        // by the same table the editor's fields are built from.
        let CatalogFile::EditorTheme(mut file) = editor_file() else {
            unreachable!("an editor file");
        };
        file.colors.keyword = "#11223344".to_string();
        editor
            .write(&CatalogFile::EditorTheme(file), &path)
            .expect("write");
        match editor.read(&path) {
            Err(ImportError::BadColor(slot)) => {
                assert_eq!(slot, "settings.editor.code.keyword");
            }
            other => panic!("accepted an opaque slot with alpha: {other:?}"),
        }

        // But a grid slot the file simply leaves out is not a bad colour; it is
        // the one thing an empty slot is allowed to mean.
        let CatalogFile::UiTheme(mut chrome) = chrome_file() else {
            unreachable!("a chrome file");
        };
        chrome.colors.grid_header = None;
        chrome.colors.grid_selection = None;
        ui.write(&CatalogFile::UiTheme(chrome), &path)
            .expect("write");
        assert!(ui.read(&path).is_ok());
    }

    #[test]
    fn an_imported_id_that_is_taken_is_renamed_rather_than_written_over() {
        // The id an install picks, asserted through the very call it makes: the
        // file's own name first, its stem second, and a suffix until the id is
        // free — of built-ins and of the files installed earlier in the same
        // batch alike.
        let taken = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        let prefix = editor().generated_id_prefix();

        assert_eq!(
            theme_store::unique_id(&["One Dark", "downloaded"], prefix, &taken(&[])),
            "one-dark"
        );
        // A built-in of the same name is never written over.
        assert_eq!(
            theme_store::unique_id(&["One Dark", "downloaded"], prefix, &taken(&["one-dark"])),
            "one-dark-2"
        );
        // Nor is the file installed a moment ago in the same batch.
        assert_eq!(
            theme_store::unique_id(
                &["One Dark", "downloaded"],
                prefix,
                &taken(&["one-dark", "one-dark-2"])
            ),
            "one-dark-3"
        );
        // A name with nothing to slug falls back to the file's own stem, and
        // then to a made-up id — never to an empty one.
        assert_eq!(
            theme_store::unique_id(&["테마", "downloaded"], prefix, &taken(&[])),
            "downloaded"
        );
        assert_eq!(
            theme_store::unique_id(&["테마", "테마"], prefix, &taken(&[])),
            format!("{prefix}-1")
        );
    }

    #[test]
    fn the_two_catalogues_never_share_a_name_of_their_own() {
        let ui = ui();
        let editor = editor();
        // An element prefix shared would collide two sibling rows' ids; a
        // generated id prefix shared would let a chrome theme take an editor
        // theme's made-up id; a directory shared would put every chrome theme
        // in the editor theme picker.
        assert_ne!(ui.element_prefix(), editor.element_prefix());
        assert_ne!(ui.generated_id_prefix(), editor.generated_id_prefix());
        assert_ne!(ui.kind_label_key(), editor.kind_label_key());
        assert_ne!(ui.delete_confirm_key(), editor.delete_confirm_key());
        assert_ne!(
            ui.dir().expect("a chrome directory"),
            editor.dir().expect("an editor directory")
        );
    }

    #[test]
    fn an_editorless_host_has_no_editor_directory_to_write_to() {
        // `ThemeDirs::editor_themes` is optional because the code editor is: an
        // application that embeds the chrome widgets and no editor has no
        // second palette, and the catalogue has to say so rather than invent a
        // place.
        let catalog = EditorThemeCatalog::new(
            ThemeDirs {
                ui_themes: PathBuf::from("/nowhere/themes"),
                editor_themes: None,
            },
            "one-dark",
        );
        assert!(catalog.dir().is_err());
        assert!(catalog.save("mine", &editor_file()).is_err());
    }

    #[test]
    fn every_slot_has_a_key_and_a_label_of_its_own() {
        for slots in [&UI_SLOTS[..], &EDITOR_SLOTS[..]] {
            let keys: std::collections::BTreeSet<&str> =
                slots.iter().map(|slot| slot.key).collect();
            assert_eq!(keys.len(), slots.len(), "two slots share an element id");
            let labels: std::collections::BTreeSet<&str> =
                slots.iter().map(|slot| slot.label_key).collect();
            assert_eq!(labels.len(), slots.len(), "two slots share a label");
            for slot in slots {
                assert!(slot.label_key.starts_with("settings.editor."));
            }
        }
        // Only the chrome palette has optional slots, and they are the tail.
        assert!(UI_SLOTS[..UI_GRID_FIRST].iter().all(|slot| !slot.optional));
        assert!(UI_SLOTS[UI_GRID_FIRST..].iter().all(|slot| slot.optional));
        assert!(EDITOR_SLOTS.iter().all(|slot| !slot.optional));
    }
}
