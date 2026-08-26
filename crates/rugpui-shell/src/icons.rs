//! Vector icons, embedded in the binary.
//!
//! gpui's [`svg`](gpui::svg) element resolves its `path` through the
//! [`AssetSource`] the application was built with — an [`IconSet`] here — and
//! paints the result as a *monochrome* sprite: resvg rasterises the file, only
//! the alpha channel survives, and the element's `text_color` supplies the
//! colour. Two things follow, and both are why an icon drawn for this looks the
//! way it does:
//!
//! * the colours written in an icon never reach the screen, only its coverage
//!   does, so a `fill-opacity` below `1` reads as a lighter shade of the tint;
//! * the tint is whatever the *element* asks for, and unlike text it is not
//!   inherited from a parent, so a hover that recolours a button has to reach
//!   the icon through [`group_hover`](gpui::InteractiveElement::group_hover).
//!
//! The bytes come from [`include_bytes!`], not from files read at run time: a
//! release then carries its icons wherever it is unpacked, and packaging has
//! nothing extra to ship. Cargo tracks the embedded files itself, so an edited
//! icon rebuilds the crate without help from `build.rs`.
//!
//! # What is here and what is the application's
//!
//! Only [`WINDOW_CONTROL_ICONS`]: the four caption glyphs a self-drawn title
//! bar needs, which are the same four files in every application that draws
//! one. Everything else an application draws — its own mark, its toolbar, the
//! glyphs of whatever it shows in a tree — belongs to that application, whose
//! table concatenates this one. So does [`rugpui::ICONS`], the two disclosure
//! marks the widget layer draws with; a set that leaves it out has trees with
//! blank arrow columns and dropdowns with no chevron:
//!
//! ```ignore
//! const ICONS: &[(&str, &[u8])] = &[ /* the application's own */ ];
//! static SET: IconSet = IconSet::new(&[rugpui::ICONS, rugpui_shell::WINDOW_CONTROL_ICONS, ICONS]);
//! ```

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, Pixels, Result, SharedString, Styled, Svg, svg};

/// A self-drawn title bar's minimise button.
///
/// The four window-control glyphs are drawn edge to edge of the 24×24 box
/// rather than inset like a toolbar icon: they are painted at half the size of
/// one, and a glyph that kept the usual margin would come out thinner and
/// smaller than the caption buttons of the platform they stand in for.
///
/// They carry a heavier stroke for the same reason — `2.2` against the `1.8` a
/// toolbar icon is usually drawn with. The caption strip renders them at 12 px
/// (`GLYPH_SIZE` in [`rugpui::window_controls`]), which is half the viewBox, so
/// the stroke that reaches the screen is half what the file asks for: `1.8`
/// arrived as 0.9 px, a hairline no row of pixels could hold at full coverage
/// once it had been antialiased, and `2.2` arrives as 1.1 px instead. All four
/// share the value, including both rectangles of [`WINDOW_RESTORE`], so that
/// the strip reads as one set.
pub const WINDOW_MINIMIZE: &str = "icons/window-minimize.svg";

/// A self-drawn title bar's maximise button, while the window is not maximised.
pub const WINDOW_MAXIMIZE: &str = "icons/window-maximize.svg";

/// A self-drawn title bar's maximise button, while the window *is* maximised.
///
/// Two offset squares, the shape every desktop uses for "put it back": the
/// button keeps its place and only the glyph says which way it will go.
pub const WINDOW_RESTORE: &str = "icons/window-restore.svg";

/// A self-drawn title bar's close button.
pub const WINDOW_CLOSE: &str = "icons/window-close.svg";

/// The four caption glyphs, paired with the bytes an [`IconSet`] hands back for
/// them.
///
/// An application concatenates this into its own table rather than copying the
/// four files: see the module docs.
pub const WINDOW_CONTROL_ICONS: &[(&str, &[u8])] = &[
    (
        WINDOW_MINIMIZE,
        include_bytes!("../assets/icons/window-minimize.svg"),
    ),
    (
        WINDOW_MAXIMIZE,
        include_bytes!("../assets/icons/window-maximize.svg"),
    ),
    (
        WINDOW_RESTORE,
        include_bytes!("../assets/icons/window-restore.svg"),
    ),
    (
        WINDOW_CLOSE,
        include_bytes!("../assets/icons/window-close.svg"),
    ),
];

/// The four caption glyphs as [`rugpui::WindowControlIcons`], which is the shape
/// [`crate::window_control_strips`] wants them in.
pub fn window_control_icons() -> rugpui::WindowControlIcons {
    rugpui::WindowControlIcons {
        minimize: WINDOW_MINIMIZE.into(),
        maximize: WINDOW_MAXIMIZE.into(),
        restore: WINDOW_RESTORE.into(),
        close: WINDOW_CLOSE.into(),
    }
}

/// An asset source over a set of embedded icon tables.
///
/// Install it with [`Application::with_assets`](gpui::Application::with_assets);
/// without it gpui's default source answers every path with `None` and the
/// icons paint as nothing at all.
///
/// Several tables rather than one, so that an application's own icons, this
/// crate's caption glyphs and [`rugpui::ICONS`] stay `const` slices in the
/// crates they belong to. A path present in more than one is answered by the
/// first table that has it.
#[derive(Debug, Clone, Copy)]
pub struct IconSet {
    /// The tables, searched in order.
    tables: &'static [&'static [(&'static str, &'static [u8])]],
}

impl IconSet {
    /// An asset source over `tables`.
    pub const fn new(tables: &'static [&'static [(&'static str, &'static [u8])]]) -> Self {
        Self { tables }
    }

    /// Every icon in the set, in table order.
    pub fn all(&self) -> impl Iterator<Item = (&'static str, &'static [u8])> + '_ {
        self.tables.iter().flat_map(|table| table.iter().copied())
    }

    /// How many icons the set holds.
    pub fn len(&self) -> usize {
        self.all().count()
    }

    /// Whether the set holds nothing at all.
    pub fn is_empty(&self) -> bool {
        self.all().next().is_none()
    }
}

impl AssetSource for IconSet {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(self
            .all()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(self
            .all()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

/// A square icon, sized and tinted.
///
/// The result is still an [`Svg`], so a caller can go on styling it — which is
/// what a hover state does.
pub fn icon(path: &'static str, size: Pixels, color: Hsla) -> Svg {
    svg().size(size).flex_none().path(path).text_color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set as an application that adds nothing would install it.
    const SET: IconSet = IconSet::new(&[WINDOW_CONTROL_ICONS]);

    #[test]
    fn every_icon_loads_and_is_an_svg() {
        for (name, _) in SET.all() {
            let bytes = SET
                .load(name)
                .expect("loading an embedded icon cannot fail")
                .unwrap_or_else(|| panic!("{name} is missing from the asset source"));
            let text = std::str::from_utf8(&bytes).expect("an icon must be UTF-8");
            assert!(text.contains("<svg"), "{name} is not an SVG");
            // The caption glyphs share one 24×24 box.
            assert!(
                text.contains("viewBox=\"0 0 24 24\""),
                "{name} has the wrong viewBox"
            );
        }
    }

    #[test]
    fn an_unknown_path_is_not_an_error() {
        assert!(
            SET.load("icons/nothing.svg")
                .expect("a missing asset is not a failure")
                .is_none()
        );
    }

    #[test]
    fn listing_returns_the_whole_set() {
        assert_eq!(SET.list("icons/").unwrap().len(), SET.len());
        assert_eq!(SET.len(), 4);
        assert!(!SET.is_empty());
        assert!(IconSet::new(&[]).is_empty());
    }

    #[test]
    fn an_applications_own_table_sits_beside_this_one() {
        const OWN: &[(&str, &[u8])] = &[("icons/app-icon.svg", b"<svg/>")];
        let set = IconSet::new(&[WINDOW_CONTROL_ICONS, OWN]);

        assert_eq!(set.len(), 5);
        assert!(
            set.load("icons/app-icon.svg")
                .expect("loading cannot fail")
                .is_some()
        );
        assert!(
            set.load(WINDOW_CLOSE)
                .expect("loading cannot fail")
                .is_some()
        );
    }

    /// The set an application actually installs: the widget layer's disclosure
    /// marks, the caption glyphs, and its own. Without the first of the three
    /// every tree and dropdown draws its arrow as nothing.
    #[test]
    fn the_widget_layers_own_table_chains_in_too() {
        const OWN: &[(&str, &[u8])] = &[("icons/app-icon.svg", b"<svg/>")];
        let set = IconSet::new(&[rugpui::ICONS, WINDOW_CONTROL_ICONS, OWN]);

        for path in [rugpui::CARET_RIGHT, rugpui::CARET_DOWN] {
            assert!(
                set.load(path).expect("loading cannot fail").is_some(),
                "{path} is not in the set"
            );
        }
        assert_eq!(set.len(), rugpui::ICONS.len() + 5);
    }

    #[test]
    fn the_caption_strip_is_handed_the_four_paths_the_set_answers_to() {
        let icons = window_control_icons();
        for path in [icons.minimize, icons.maximize, icons.restore, icons.close] {
            assert!(
                SET.load(&path).expect("loading cannot fail").is_some(),
                "{path} is not in the set"
            );
        }
    }
}
